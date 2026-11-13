//! Content-addressed store with parallel, resumable, verified downloads.
//!
//! Every file the launcher installs (server artifacts, MySQL, upstream server
//! data) enters through [`Store::fetch`], keyed by its sha256:
//!
//! ```text
//! <root>/blobs/<first 2 hex>/<sha256>     verified, immutable content
//! <root>/partial/<sha256>.part            download in progress (preallocated)
//! <root>/partial/<sha256>.json            resume state (completed segments)
//! <root>/partial/<sha256>.lock            held while a fetch of that blob runs
//! ```
//!
//! - **Parallel:** when the server supports HTTP ranges, the file is split into
//!   segments downloaded over several connections.
//! - **Resumable:** completed segments are persisted; an interrupted download
//!   (crash, network loss, launcher closed) restarts only missing segments,
//!   even from another mirror (same sha256 = same bytes).
//! - **Mirrors:** sources are tried in order until one delivers the right bytes
//!   (a mirror serving corrupted data is skipped like an unreachable one).
//! - **Concurrent-safe:** fetches of the same blob (tasks or processes) are
//!   serialised by an OS file lock; the second one finds the blob ready.
//! - **Verified:** size and sha256 are checked before a blob becomes visible;
//!   a mismatch deletes the partial data. Nothing unverified is ever executed.

#![forbid(unsafe_code)]

mod download;
mod error;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

pub use error::StoreError;

/// What to fetch: identical bytes available from one or more mirrors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    /// Mirrors, tried in order.
    pub urls: Vec<String>,
    /// Exact size in bytes.
    pub size: u64,
    /// Lowercase hex sha256.
    pub sha256: String,
}

/// Download progress sink (UI progress bars, logs). Must be cheap and thread-safe.
pub trait Progress: Send + Sync {
    /// `bytes` more bytes were received.
    fn advance(&self, bytes: u64);
    /// `bytes` previously reported were discarded (failed attempt, retried later).
    fn retract(&self, _bytes: u64) {}
}

/// Ignores progress.
pub struct NoProgress;

impl Progress for NoProgress {
    fn advance(&self, _bytes: u64) {}
}

/// Tuning knobs. Defaults suit large downloads on consumer connections.
#[derive(Debug, Clone)]
pub struct StoreOptions {
    /// Parallel connections per file.
    pub connections: usize,
    /// Segment size for ranged downloads (resume granularity).
    pub segment_size: u64,
    /// Attempts per segment before giving up on a source.
    pub attempts: u32,
    /// Initial retry delay (doubled at each attempt).
    pub retry_delay: Duration,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            connections: 8,
            segment_size: 16 * 1024 * 1024,
            attempts: 4,
            retry_delay: Duration::from_millis(500),
        }
    }
}

/// The content-addressed store.
pub struct Store {
    root: PathBuf,
    client: reqwest::Client,
    options: StoreOptions,
}

impl Store {
    /// Opens (and creates if needed) a store rooted at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        Self::with_options(root, StoreOptions::default())
    }

    pub fn with_options(
        root: impl Into<PathBuf>,
        options: StoreOptions,
    ) -> Result<Self, StoreError> {
        let root = root.into();
        for dir in [root.join("blobs"), root.join("partial")] {
            std::fs::create_dir_all(&dir).map_err(StoreError::io(&dir))?;
        }
        let client = reqwest::Client::builder()
            .user_agent(concat!("EchoesOfAzeroth/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            root,
            client,
            options: StoreOptions {
                connections: options.connections.max(1),
                segment_size: options.segment_size.max(1),
                attempts: options.attempts.max(1),
                ..options
            },
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a verified blob lives (whether or not it is present). The hash is
    /// case-insensitive.
    pub fn blob_path(&self, sha256: &str) -> PathBuf {
        let sha256 = sha256.to_ascii_lowercase();
        let prefix = sha256.get(..2).unwrap_or("00").to_owned();
        self.root.join("blobs").join(prefix).join(sha256)
    }

    /// True when the blob is already in the store.
    pub fn contains(&self, sha256: &str) -> bool {
        self.blob_path(sha256).is_file()
    }

    /// Returns the path of the verified blob, downloading it first if needed.
    pub async fn fetch(
        &self,
        request: &FetchRequest,
        progress: &dyn Progress,
    ) -> Result<PathBuf, StoreError> {
        let sha256 = normalize_sha256(&request.sha256)?;
        let blob = self.blob_path(&sha256);
        if blob.is_file() {
            progress.advance(request.size);
            return Ok(blob);
        }
        if request.urls.is_empty() {
            return Err(StoreError::AllSourcesFailed(vec!["no source URL".into()]));
        }

        let partial = self.root.join("partial");
        let _lock = lock(&partial.join(format!("{sha256}.lock"))).await?;
        if blob.is_file() {
            // Another fetch completed it while we waited for the lock.
            progress.advance(request.size);
            return Ok(blob);
        }

        let part = partial.join(format!("{sha256}.part"));
        let state = partial.join(format!("{sha256}.json"));
        let mut failures = Vec::new();
        for url in &request.urls {
            let tracked = Tracked::new(progress);
            let result = async {
                download::fetch_to(self, url, request.size, &part, &state, &tracked).await?;
                let actual = download::sha256_file(&part).await?;
                if actual != sha256 {
                    // Corrupted source: discard everything it wrote, try the next one.
                    let _ = std::fs::remove_file(&part);
                    let _ = std::fs::remove_file(&state);
                    return Err(StoreError::HashMismatch {
                        expected: sha256.clone(),
                        actual,
                    });
                }
                Ok(())
            }
            .await;
            match result {
                Ok(()) => {
                    let parent = blob.parent().expect("blob path has a parent");
                    std::fs::create_dir_all(parent).map_err(StoreError::io(parent))?;
                    std::fs::rename(&part, &blob).map_err(StoreError::io(&blob))?;
                    let _ = std::fs::remove_file(&state);
                    return Ok(blob);
                }
                Err(error) => {
                    // Whatever this source reported is either kept on disk and
                    // reported again by the next attempt, or gone.
                    tracked.rollback();
                    failures.push(format!("{url}: {error}"));
                }
            }
        }
        Err(StoreError::AllSourcesFailed(failures))
    }
}

/// Counts what one attempt reports so it can be undone if the attempt fails.
struct Tracked<'a> {
    inner: &'a dyn Progress,
    net: AtomicI64,
}

impl<'a> Tracked<'a> {
    fn new(inner: &'a dyn Progress) -> Self {
        Self {
            inner,
            net: AtomicI64::new(0),
        }
    }

    fn rollback(&self) {
        let net = self.net.swap(0, Ordering::SeqCst);
        if net > 0 {
            self.inner.retract(net as u64);
        }
    }
}

impl Progress for Tracked<'_> {
    fn advance(&self, bytes: u64) {
        self.net.fetch_add(bytes as i64, Ordering::SeqCst);
        self.inner.advance(bytes);
    }

    fn retract(&self, bytes: u64) {
        self.net.fetch_sub(bytes as i64, Ordering::SeqCst);
        self.inner.retract(bytes);
    }
}

/// Takes an exclusive OS lock (released when the returned file is dropped, also
/// if the process dies), waiting asynchronously while another fetch holds it.
async fn lock(path: &Path) -> Result<std::fs::File, StoreError> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(StoreError::io(path))?;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                tokio::time::sleep(Duration::from_millis(200)).await
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(StoreError::io(path)(error)),
        }
    }
}

fn normalize_sha256(value: &str) -> Result<String, StoreError> {
    let lower = value.to_ascii_lowercase();
    if lower.len() == 64 && lower.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(lower)
    } else {
        Err(StoreError::InvalidHash(value.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_paths_are_sharded() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let sha = "ab".repeat(32);
        assert_eq!(
            store.blob_path(&sha),
            dir.path().join("blobs").join("ab").join(&sha)
        );
    }

    #[test]
    fn rejects_invalid_hashes() {
        assert!(normalize_sha256("xyz").is_err());
        assert_eq!(normalize_sha256(&"AB".repeat(32)).unwrap(), "ab".repeat(32));
    }
}
