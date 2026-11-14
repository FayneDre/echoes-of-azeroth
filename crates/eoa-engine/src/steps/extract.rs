use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use thiserror::Error;
use zip::ZipArchive;

use super::with_progress;
use crate::{CancelToken, Cancelled, DirSwap, Step, StepContext, StepError, StepFuture};

/// Leading path components to drop from archive entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strip {
    None,
    Components(usize),
    /// The archive's single top-level directory (e.g. `mysql-8.4.13-winx64/`);
    /// an archive without one is an error.
    SingleRootDir,
}

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("archive {path} is invalid: {source}")]
    Archive {
        path: PathBuf,
        source: zip::result::ZipError,
    },
    #[error("archive entry '{0}' is unsafe (absolute path, '..' or link)")]
    UnsafeEntry(String),
    #[error("archive entry '{0}' is extracted twice")]
    Duplicate(String),
    #[error("archive has no single root directory to strip")]
    NoSingleRoot,
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ExtractError + '_ {
    move |source| ExtractError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// `extract`: unpacks a ZIP archive into a directory, atomically.
///
/// The archive is extracted into a staging directory by several threads
/// (largest files first), every entry's CRC-32 is checked while reading, then
/// [`DirSwap`] puts it in place. `undo` restores the previous directory;
/// `finalize` drops it. Entries that would escape the target (absolute paths,
/// `..`, links) fail the step before anything is written.
pub struct Extract {
    id: String,
    archive: PathBuf,
    target: PathBuf,
    strip: Strip,
    threads: usize,
}

impl Extract {
    pub fn new(
        id: impl Into<String>,
        archive: impl Into<PathBuf>,
        target: impl Into<PathBuf>,
    ) -> Self {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
        Self {
            id: id.into(),
            archive: archive.into(),
            target: target.into(),
            strip: Strip::None,
            threads,
        }
    }

    pub fn strip(mut self, strip: Strip) -> Self {
        self.strip = strip;
        self
    }

    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    fn swap(&self, cx: &StepContext<'_, impl Sync>) -> Result<DirSwap, StepError> {
        Ok(DirSwap::new(&self.target, cx.run_id())?)
    }
}

/// Runs blocking filesystem work off the async threads.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, StepError> + Send + 'static,
) -> Result<T, StepError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| -> StepError { format!("extraction thread failed: {e}").into() })?
}

impl<C: Sync> Step<C> for Extract {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> &str {
        "extract"
    }

    fn fingerprint(&self) -> String {
        format!(
            "{} -> {} ({:?})",
            self.archive.display(),
            self.target.display(),
            self.strip
        )
    }

    fn run<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            let swap = self.swap(cx)?;
            let done = Arc::new(AtomicU64::new(0));
            let total = Arc::new(AtomicU64::new(0));
            let job = {
                let (archive, strip, threads) = (self.archive.clone(), self.strip, self.threads);
                let (done, total, cancel) = (done.clone(), total.clone(), cx.cancel_token());
                move || -> Result<bool, StepError> {
                    let staging = swap.prepare()?;
                    let finished =
                        unzip(&archive, &staging, strip, threads, &done, &total, &cancel)?;
                    if finished {
                        swap.commit()?;
                    }
                    Ok(finished)
                }
            };
            // Not cancellable from outside: the thread stops itself at a safe point.
            let finished = with_progress(cx, &done, &total, false, blocking(job))
                .await
                .expect("not cancellable")?;
            if finished {
                Ok(())
            } else {
                Err(Box::new(Cancelled).into())
            }
        })
    }

    fn undo<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            let swap = self.swap(cx)?;
            blocking(move || Ok(swap.undo()?)).await
        })
    }

    fn finalize<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            let swap = self.swap(cx)?;
            blocking(move || Ok(swap.finalize()?)).await
        })
    }
}

struct FileEntry {
    index: usize,
    out: PathBuf,
    size: u64,
}

type Archive = ZipArchive<BufReader<File>>;

fn open(path: &Path) -> Result<Archive, ExtractError> {
    let file = File::open(path).map_err(io(path))?;
    ZipArchive::new(BufReader::with_capacity(1 << 20, file)).map_err(|source| {
        ExtractError::Archive {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// Extracts `archive` into the empty directory `dest`. Returns `false` when
/// stopped by a cancellation (the content is then incomplete).
pub(crate) fn unzip(
    archive: &Path,
    dest: &Path,
    strip: Strip,
    threads: usize,
    done: &AtomicU64,
    total: &AtomicU64,
    cancel: &CancelToken,
) -> Result<bool, ExtractError> {
    let invalid = |source| ExtractError::Archive {
        path: archive.to_path_buf(),
        source,
    };
    let mut zip = open(archive)?;

    // 1. Validate every entry before writing anything.
    let mut entries = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let entry = zip.by_index_raw(index).map_err(invalid)?;
        let name = entry
            .name()
            .map(|n| n.into_owned())
            .unwrap_or_else(|_| format!("#{index} (undecodable name)"));
        // Own check on the raw name first (the library may silently rewrite an
        // absolute name), then the library's as a second guard.
        let path = match entry.enclosed_name() {
            Some(path) if is_relative_name(&name) && !entry.is_symlink() => path,
            _ => return Err(ExtractError::UnsafeEntry(name)),
        };
        let parts: Vec<PathBuf> = path
            .components()
            .map(|c| match c {
                Component::Normal(part) => Ok(PathBuf::from(part)),
                _ => Err(ExtractError::UnsafeEntry(name.clone())),
            })
            .collect::<Result<_, _>>()?;
        entries.push((index, parts, entry.is_dir(), entry.size()));
    }
    let skip = match strip {
        Strip::None => 0,
        Strip::Components(n) => n,
        Strip::SingleRootDir => {
            let roots: BTreeSet<&PathBuf> = entries.iter().filter_map(|e| e.1.first()).collect();
            let files_nested = entries
                .iter()
                .all(|(_, parts, is_dir, _)| *is_dir || parts.len() > 1);
            if roots.len() != 1 || !files_nested {
                return Err(ExtractError::NoSingleRoot);
            }
            1
        }
    };

    let mut dirs = BTreeSet::new();
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, parts, is_dir, size) in entries {
        let Some(relative) = parts.get(skip..).filter(|p| !p.is_empty()) else {
            continue; // a stripped root directory
        };
        let out: PathBuf = std::iter::once(dest.to_path_buf())
            .chain(relative.iter().cloned())
            .collect();
        if is_dir {
            dirs.insert(out);
            continue;
        }
        if !seen.insert(out.clone()) {
            return Err(ExtractError::Duplicate(out.display().to_string()));
        }
        if let Some(parent) = out.parent() {
            dirs.insert(parent.to_path_buf());
        }
        files.push(FileEntry { index, out, size });
    }

    // 2. Directories, then files in parallel, largest first (better balance).
    for dir in &dirs {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
    }
    total.store(files.iter().map(|f| f.size).sum(), Ordering::Relaxed);
    files.sort_by_key(|f| std::cmp::Reverse(f.size));

    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let failure: Mutex<Option<ExtractError>> = Mutex::new(None);
    let worker = || {
        let result = (|| -> Result<(), ExtractError> {
            let mut zip = open(archive)?;
            let mut buf = vec![0u8; 256 * 1024];
            while !stop.load(Ordering::Relaxed) && !cancel.is_cancelled() {
                let Some(file) = files.get(next.fetch_add(1, Ordering::Relaxed)) else {
                    break;
                };
                write_entry(&mut zip, archive, file, &mut buf, done, cancel)?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            stop.store(true, Ordering::Relaxed);
            failure.lock().unwrap().get_or_insert(e);
        }
    };
    std::thread::scope(|s| {
        for _ in 0..threads.clamp(1, files.len().max(1)) {
            s.spawn(worker);
        }
    });
    if let Some(e) = failure.into_inner().unwrap() {
        return Err(e);
    }
    Ok(!cancel.is_cancelled())
}

/// A plain relative name: no root (`/x`, `\x`, `\\server`), no drive (`C:`),
/// no `..`, whatever the separator.
fn is_relative_name(name: &str) -> bool {
    !name.starts_with(['/', '\\'])
        && name
            .split(['/', '\\'])
            .all(|part| part != ".." && !part.contains(':'))
}

fn write_entry(
    zip: &mut Archive,
    archive: &Path,
    file: &FileEntry,
    buf: &mut [u8],
    done: &AtomicU64,
    cancel: &CancelToken,
) -> Result<(), ExtractError> {
    let mut entry = zip
        .by_index(file.index)
        .map_err(|source| ExtractError::Archive {
            path: archive.to_path_buf(),
            source,
        })?;
    let mut out = File::create(&file.out).map_err(io(&file.out))?;
    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }
        // A CRC-32 mismatch surfaces here as an InvalidData read error.
        let n = entry.read(buf).map_err(io(&file.out))?;
        if n == 0 {
            return Ok(());
        }
        out.write_all(&buf[..n]).map_err(io(&file.out))?;
        done.fetch_add(n as u64, Ordering::Relaxed);
    }
}
