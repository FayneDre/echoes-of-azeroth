//! Download strategies: segmented (HTTP ranges, parallel, resumable) or single stream.

use std::io::{Read, SeekFrom};
use std::path::Path;

use futures_util::{stream, StreamExt};
use reqwest::header::{CONTENT_RANGE, RANGE};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use crate::{Progress, Store, StoreError};

/// Persisted resume state of a segmented download.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct PartialState {
    size: u64,
    segment_size: u64,
    done: Vec<bool>,
}

impl PartialState {
    fn new(size: u64, segment_size: u64) -> Self {
        let count = size.div_ceil(segment_size).max(1) as usize;
        Self {
            size,
            segment_size,
            done: vec![false; count],
        }
    }

    fn range(&self, index: usize) -> (u64, u64) {
        let start = index as u64 * self.segment_size;
        let end = (start + self.segment_size).min(self.size) - 1;
        (start, end)
    }

    fn done_bytes(&self) -> u64 {
        (0..self.done.len())
            .filter(|i| self.done[*i])
            .map(|i| self.range(i).1 - self.range(i).0 + 1)
            .sum()
    }
}

/// Downloads `url` into `part` (exactly `size` bytes), resuming from `state` when possible.
pub(crate) async fn fetch_to(
    store: &Store,
    url: &str,
    size: u64,
    part: &Path,
    state_path: &Path,
    progress: &dyn Progress,
) -> Result<(), StoreError> {
    let ranged = size > store.options.segment_size && supports_ranges(store, url, size).await?;
    if ranged {
        segmented(store, url, size, part, state_path, progress).await
    } else {
        single(store, url, size, part, progress).await
    }
}

/// Probes `bytes=0-0`: 206 + a total matching `size` means ranged downloads work.
async fn supports_ranges(store: &Store, url: &str, size: u64) -> Result<bool, StoreError> {
    let response = store
        .client
        .get(url)
        .header(RANGE, "bytes=0-0")
        .send()
        .await?;
    match response.status() {
        StatusCode::PARTIAL_CONTENT => {
            let total = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|v| v.parse::<u64>().ok());
            match total {
                Some(total) if total != size => Err(StoreError::SizeMismatch {
                    expected: size,
                    actual: total,
                }),
                _ => Ok(true),
            }
        }
        status if status.is_success() => Ok(false),
        status => Err(StoreError::Status {
            status: status.as_u16(),
        }),
    }
}

async fn segmented(
    store: &Store,
    url: &str,
    size: u64,
    part: &Path,
    state_path: &Path,
    progress: &dyn Progress,
) -> Result<(), StoreError> {
    let segment_size = store.options.segment_size;
    let mut state = std::fs::read(state_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PartialState>(&bytes).ok())
        .filter(|s| {
            s.size == size && s.segment_size == segment_size && part_len(part) == Some(size)
        })
        .unwrap_or_else(|| PartialState::new(size, segment_size));

    if state.done.iter().all(|d| !d) {
        // Fresh start: preallocate so segments can be written at their offset.
        let file = std::fs::File::create(part).map_err(StoreError::io(part))?;
        file.set_len(size).map_err(StoreError::io(part))?;
        save_state(state_path, &state)?;
    }
    progress.advance(state.done_bytes());

    let pending: Vec<usize> = (0..state.done.len()).filter(|i| !state.done[*i]).collect();
    let ranges: Vec<(usize, (u64, u64))> = pending.iter().map(|i| (*i, state.range(*i))).collect();
    let mut results = stream::iter(ranges)
        .map(|(index, (start, end))| async move {
            (
                index,
                segment_with_retries(store, url, part, start, end, progress).await,
            )
        })
        .buffer_unordered(store.options.connections);

    while let Some((index, result)) = results.next().await {
        // Stop at the first failure: in-flight segments are dropped (the caller
        // retracts their progress) and the next mirror resumes from the state.
        result?;
        state.done[index] = true;
        save_state(state_path, &state)?;
    }
    Ok(())
}

async fn segment_with_retries(
    store: &Store,
    url: &str,
    part: &Path,
    start: u64,
    end: u64,
    progress: &dyn Progress,
) -> Result<(), StoreError> {
    with_retries(store, || segment(store, url, part, start, end, progress)).await
}

/// Runs `operation` up to `attempts` times with exponential backoff, giving up
/// immediately on errors that cannot fix themselves.
async fn with_retries<F, Fut>(store: &Store, mut operation: F) -> Result<(), StoreError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), StoreError>>,
{
    let mut delay = store.options.retry_delay;
    let mut attempt = 1;
    loop {
        match operation().await {
            Ok(()) => return Ok(()),
            Err(error) if error.is_permanent() || attempt >= store.options.attempts => {
                return Err(error)
            }
            Err(_) => {
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(MAX_RETRY_DELAY);
                attempt += 1;
            }
        }
    }
}

const MAX_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(30);

async fn segment(
    store: &Store,
    url: &str,
    part: &Path,
    start: u64,
    end: u64,
    progress: &dyn Progress,
) -> Result<(), StoreError> {
    let expected = end - start + 1;
    let mut written = 0u64;
    let result = async {
        let response = store
            .client
            .get(url)
            .header(RANGE, format!("bytes={start}-{end}"))
            .send()
            .await?;
        if response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(StoreError::Status {
                status: response.status().as_u16(),
            });
        }
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(part)
            .await
            .map_err(StoreError::io(part))?;
        file.seek(SeekFrom::Start(start))
            .await
            .map_err(StoreError::io(part))?;
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            let take = (chunk.len() as u64).min(expected - written) as usize;
            file.write_all(&chunk[..take])
                .await
                .map_err(StoreError::io(part))?;
            written += take as u64;
            progress.advance(take as u64);
            if written == expected {
                break;
            }
        }
        file.flush().await.map_err(StoreError::io(part))?;
        if written != expected {
            return Err(StoreError::Incomplete {
                expected,
                got: written,
            });
        }
        Ok(())
    }
    .await;
    if result.is_err() {
        progress.retract(written);
    }
    result
}

/// Whole-file download for servers without range support (no resume possible).
async fn single(
    store: &Store,
    url: &str,
    size: u64,
    part: &Path,
    progress: &dyn Progress,
) -> Result<(), StoreError> {
    with_retries(store, || async {
        let mut written = 0u64;
        let result = async {
            let response = store.client.get(url).send().await?;
            if !response.status().is_success() {
                return Err(StoreError::Status {
                    status: response.status().as_u16(),
                });
            }
            if let Some(length) = response.content_length() {
                if length != size {
                    return Err(StoreError::SizeMismatch {
                        expected: size,
                        actual: length,
                    });
                }
            }
            let mut file = tokio::fs::File::create(part)
                .await
                .map_err(StoreError::io(part))?;
            let mut body = response.bytes_stream();
            while let Some(chunk) = body.next().await {
                let chunk = chunk?;
                if written + chunk.len() as u64 > size {
                    return Err(StoreError::SizeMismatch {
                        expected: size,
                        actual: written + chunk.len() as u64,
                    });
                }
                file.write_all(&chunk).await.map_err(StoreError::io(part))?;
                written += chunk.len() as u64;
                progress.advance(chunk.len() as u64);
            }
            file.flush().await.map_err(StoreError::io(part))?;
            if written != size {
                return Err(StoreError::Incomplete {
                    expected: size,
                    got: written,
                });
            }
            Ok(())
        }
        .await;
        if result.is_err() {
            progress.retract(written);
        }
        result
    })
    .await
}

fn part_len(part: &Path) -> Option<u64> {
    std::fs::metadata(part).ok().map(|m| m.len())
}

/// Writes the state atomically (temp file + rename) so a crash never corrupts it.
fn save_state(path: &Path, state: &PartialState) -> Result<(), StoreError> {
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(state).expect("state serializes");
    std::fs::write(&tmp, bytes).map_err(StoreError::io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(StoreError::io(path))
}

/// Streams a file through sha256 on a blocking thread.
pub(crate) async fn sha256_file(path: &Path) -> Result<String, StoreError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path).map_err(StoreError::io(&path))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(StoreError::io(&path))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(hex::encode(hasher.finalize()))
    })
    .await
    .expect("hashing task panicked")
}

#[cfg(test)]
mod tests {
    use super::PartialState;

    #[test]
    fn segments_cover_the_file_exactly() {
        let state = PartialState::new(10, 4);
        assert_eq!(state.done.len(), 3);
        assert_eq!(state.range(0), (0, 3));
        assert_eq!(state.range(2), (8, 9));
        let mut state = state;
        state.done[2] = true;
        assert_eq!(state.done_bytes(), 2);
    }
}
