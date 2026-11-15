//! Module assets (ADR 0013): files a module ships in its own repository,
//! selected by the player's option choices.
//!
//! - Each archive is downloaded from the module's pinned commit into the
//!   store (sha256-verified), then unpacked once into
//!   `assets/<archive sha256>/` (content-addressed, shared by instances).
//! - Server files are overlaid on the shared server data in
//!   `instances/<id>/data/`: directories without an override are linked
//!   (junctions on Windows), the others are rebuilt with hard links to the
//!   shared files plus the overrides. `DataDir` points there only while an
//!   override exists.
//! - Client files are copied into the player's client by `eoa-client`,
//!   right before a launch.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eoa_core::manifest::{Asset, OptionValue};
use eoa_core::{Catalog, Resolution};
use eoa_store::{FetchRequest, Store};

use super::{Layout, PlanError};
use crate::steps::Fetch;
use crate::{fsutil, DirSwap, EngineError, Step, StepContext, StepFuture};

/// An asset chosen for an instance, with where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedAsset {
    pub module: String,
    pub asset: Asset,
    pub url: String,
}

impl SelectedAsset {
    /// Where its archive is unpacked.
    pub fn dir(&self, layout: &Layout) -> PathBuf {
        layout.assets().join(&self.asset.source.sha256)
    }
}

/// The assets selected by the option values of the resolved modules.
pub fn selected(
    catalog: &Catalog,
    resolution: &Resolution,
    options: &BTreeMap<String, BTreeMap<String, OptionValue>>,
) -> Result<Vec<SelectedAsset>, PlanError> {
    let entry = catalog
        .emulator(&resolution.emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{}'", resolution.emulator)))?;
    let mut out = Vec::new();
    for id in &resolution.modules {
        let Some(module) = entry.modules.get(id) else {
            continue;
        };
        for asset in module.selected_assets(options.get(id)) {
            let url = module.source.raw_url(&asset.source.path).ok_or_else(|| {
                PlanError::Unknown(format!(
                    "download location of {id}/{} (only GitHub repositories)",
                    asset.id
                ))
            })?;
            out.push(SelectedAsset {
                module: id.clone(),
                asset: asset.clone(),
                url,
            });
        }
    }
    // Two assets writing the same destination would depend on their order.
    let mut seen: BTreeMap<(&str, &str), &str> = BTreeMap::new();
    for a in &out {
        let files = a
            .asset
            .client
            .iter()
            .map(|f| ("client", f))
            .chain(a.asset.server.iter().map(|f| ("server", f)));
        for (side, file) in files {
            if let Some(other) = seen.insert((side, file.to.as_str()), a.asset.id.as_str()) {
                return Err(PlanError::Unknown(format!(
                    "combination: assets '{other}' and '{}' both write {side} file '{}'",
                    a.asset.id, file.to
                )));
            }
        }
    }
    Ok(out)
}

/// `download-<sha>` + `unpack-<sha>` for each archive of the selected
/// assets (one per archive, even when several assets share it).
pub fn asset_steps<C: Sync + 'static>(
    layout: &Layout,
    store: &Arc<Store>,
    assets: &[SelectedAsset],
) -> Vec<Box<dyn Step<C>>> {
    let mut archives: BTreeMap<&str, (&SelectedAsset, Vec<String>)> = BTreeMap::new();
    for a in assets {
        let members = a
            .asset
            .client
            .iter()
            .chain(&a.asset.server)
            .map(|f| f.from.clone());
        let entry = archives
            .entry(a.asset.source.sha256.as_str())
            .or_insert_with(|| (a, Vec::new()));
        entry.1.extend(members);
    }
    let mut steps: Vec<Box<dyn Step<C>>> = Vec::new();
    for (sha, (a, mut members)) in archives {
        members.sort();
        members.dedup();
        let short = &sha[..12];
        let source = &a.asset.source;
        steps.push(Box::new(Fetch::new(
            format!("download-{short}"),
            store.clone(),
            FetchRequest {
                urls: vec![a.url.clone()],
                size: source.size,
                sha256: source.sha256.clone(),
            },
        )));
        steps.push(Box::new(UnpackAsset {
            id: format!("unpack-{short}"),
            archive: store.blob_path(sha),
            target: a.dir(layout),
            seven_zip: source.path.ends_with(".7z"),
            members,
        }));
    }
    steps
}

/// Server overrides of the selected assets: (unpacked file, destination
/// relative to the data directory).
pub fn server_files(layout: &Layout, assets: &[SelectedAsset]) -> Vec<(PathBuf, String)> {
    assets
        .iter()
        .flat_map(|a| {
            let dir = a.dir(layout);
            a.asset
                .server
                .iter()
                .map(move |f| (dir.join(&f.from), f.to.clone()))
        })
        .collect()
}

/// Client files of the selected assets: (unpacked file, destination
/// relative to the client patch directory).
pub fn client_files(layout: &Layout, assets: &[SelectedAsset]) -> Vec<(PathBuf, String)> {
    assets
        .iter()
        .flat_map(|a| {
            let dir = a.dir(layout);
            a.asset
                .client
                .iter()
                .map(move |f| (dir.join(&f.from), f.to.clone()))
        })
        .collect()
}

/// `unpack-<sha>`: unpacks a downloaded archive (`.zip` or `.7z`) into its
/// content-addressed directory. Nothing to undo: the directory is a cache
/// keyed by the archive hash, complete once renamed in place.
pub struct UnpackAsset {
    id: String,
    pub archive: PathBuf,
    pub target: PathBuf,
    pub seven_zip: bool,
    /// Members that must be present afterwards.
    pub members: Vec<String>,
}

impl UnpackAsset {
    fn complete(&self) -> bool {
        self.members.iter().all(|m| self.target.join(m).is_file())
    }

    fn unpack(&self) -> Result<(), String> {
        let partial = self.target.with_extension("partial");
        let _ = std::fs::remove_dir_all(&partial);
        std::fs::create_dir_all(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
        if self.seven_zip {
            let at = |e: sevenz_rust2::Error| format!("{}: {e}", self.archive.display());
            let mut reader =
                sevenz_rust2::ArchiveReader::open(&self.archive, sevenz_rust2::Password::empty())
                    .map_err(at)?;
            let mut failure = None;
            reader
                .for_each_entries(|entry, data| {
                    let name = entry.name().replace('\\', "/");
                    let unsafe_name = name.starts_with('/')
                        || name.contains(':')
                        || name.split('/').any(|s| s == "..");
                    if unsafe_name {
                        failure = Some(format!("unsafe path '{name}' in the archive"));
                        return Ok(false);
                    }
                    let out = partial.join(&name);
                    if entry.is_directory() {
                        std::fs::create_dir_all(&out)?;
                        return Ok(true);
                    }
                    if let Some(parent) = out.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let mut file = std::fs::File::create(&out)?;
                    std::io::copy(data, &mut file)?;
                    Ok(true)
                })
                .map_err(at)?;
            if let Some(failure) = failure {
                return Err(format!("{}: {failure}", self.archive.display()));
            }
        } else {
            let file = std::fs::File::open(&self.archive)
                .map_err(|e| format!("{}: {e}", self.archive.display()))?;
            let mut zip = zip::ZipArchive::new(file)
                .map_err(|e| format!("{}: {e}", self.archive.display()))?;
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
                let Some(name) = entry.enclosed_name() else {
                    return Err(format!("unsafe path in {}", self.archive.display()));
                };
                let out = partial.join(name);
                if entry.is_dir() {
                    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                    continue;
                }
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                std::fs::write(&out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;
            }
        }
        for member in &self.members {
            if !partial.join(member).is_file() {
                return Err(format!(
                    "'{member}' is not in {} (catalog out of date?)",
                    self.archive.display()
                ));
            }
        }
        let _ = std::fs::remove_dir_all(&self.target);
        std::fs::rename(&partial, &self.target)
            .map_err(|e| format!("{}: {e}", self.target.display()))
    }
}

impl<C: Sync> Step<C> for UnpackAsset {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> &str {
        "extract"
    }
    fn fingerprint(&self) -> String {
        format!("{:?} {:?} {:?}", self.archive, self.target, self.members)
    }
    fn run<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            if self.complete() {
                return Ok(());
            }
            let this = UnpackAsset {
                id: self.id.clone(),
                archive: self.archive.clone(),
                target: self.target.clone(),
                seven_zip: self.seven_zip,
                members: self.members.clone(),
            };
            tokio::task::spawn_blocking(move || this.unpack())
                .await
                .map_err(|e| e.to_string())??;
            Ok(())
        })
    }
}

/// `server-data`: builds `instances/<id>/data/` = shared server data + the
/// selected server overrides, swapped in atomically ([`DirSwap`]). With no
/// override, `DataDir` points at the shared data and finalize removes the
/// overlay left by an earlier choice.
pub struct ServerOverlay {
    pub shared: PathBuf,
    pub target: PathBuf,
    /// (source file, destination relative to the data directory).
    pub files: Vec<(PathBuf, String)>,
}

impl ServerOverlay {
    fn swap<C>(&self, cx: &StepContext<'_, C>) -> Result<DirSwap, EngineError> {
        DirSwap::new(&self.target, cx.run_id())
    }

    fn build(&self, staging: &Path) -> Result<(), String> {
        let overrides: Vec<&str> = self.files.iter().map(|(_, to)| to.as_str()).collect();
        mirror(&self.shared, staging, "", &overrides)?;
        for (from, to) in &self.files {
            let out = staging.join(to);
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::copy(from, &out).map_err(|e| format!("{}: {e}", from.display()))?;
        }
        Ok(())
    }
}

/// Recreates `src` in `dst`: a directory without an override below it is
/// linked, one with overrides is rebuilt with hard links (copies if the
/// volume refuses them); overridden files are left out.
fn mirror(src: &Path, dst: &Path, prefix: &str, overrides: &[&str]) -> Result<(), String> {
    let io = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
    let mut entries: Vec<_> = std::fs::read_dir(src)
        .map_err(|e| io(src, e))?
        .collect::<Result<_, _>>()
        .map_err(|e| io(src, e))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let from = entry.path();
        let to = dst.join(&name);
        let is_dir = entry.file_type().map_err(|e| io(&from, e))?.is_dir();
        if is_dir {
            let below = format!("{rel}/");
            let touched = overrides.iter().any(|o| {
                o.to_ascii_lowercase()
                    .starts_with(&below.to_ascii_lowercase())
            });
            if touched {
                std::fs::create_dir(&to).map_err(|e| io(&to, e))?;
                mirror(&from, &to, &rel, overrides)?;
            } else {
                link_dir(&from, &to).map_err(|e| io(&to, e))?;
            }
        } else if !overrides.iter().any(|o| o.eq_ignore_ascii_case(&rel))
            && std::fs::hard_link(&from, &to).is_err()
        {
            std::fs::copy(&from, &to).map_err(|e| io(&from, e))?;
        }
    }
    Ok(())
}

/// A directory link that needs no privilege: a junction on Windows.
#[cfg(windows)]
fn link_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    junction::create(target, link)
}

#[cfg(not(windows))]
fn link_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

impl<C: Sync> Step<C> for ServerOverlay {
    fn id(&self) -> &str {
        "server-data"
    }
    fn kind(&self) -> &str {
        "overlay-server-file"
    }
    fn fingerprint(&self) -> String {
        format!("{:?} {:?} {:?}", self.shared, self.target, self.files)
    }
    fn run<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            if self.files.is_empty() {
                return Ok(());
            }
            let swap = self.swap(cx)?;
            let staging = swap.prepare()?;
            self.build(&staging)?;
            swap.commit()?;
            Ok(())
        })
    }
    fn undo<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            if self.files.is_empty() {
                return Ok(());
            }
            Ok(self.swap(cx)?.undo()?)
        })
    }
    fn finalize<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            if self.files.is_empty() {
                // Configs now point at the shared data: drop the old overlay
                // (junctions are removed, never followed).
                fsutil::remove_dir_all(&self.target)?;
                return Ok(());
            }
            Ok(self.swap(cx)?.finalize()?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_links_untouched_dirs_and_replaces_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let shared = tmp.path().join("shared");
        std::fs::create_dir_all(shared.join("dbc")).unwrap();
        std::fs::create_dir_all(shared.join("maps")).unwrap();
        std::fs::write(shared.join("dbc/Spell.dbc"), b"original").unwrap();
        std::fs::write(shared.join("dbc/Item.dbc"), b"item").unwrap();
        std::fs::write(shared.join("maps/0001.map"), b"map").unwrap();
        let patch = tmp.path().join("Spell.dbc");
        std::fs::write(&patch, b"patched").unwrap();
        let overlay = ServerOverlay {
            shared: shared.clone(),
            target: tmp.path().join("data"),
            files: vec![(patch, "dbc/Spell.dbc".into())],
        };
        let out = tmp.path().join("staging");
        std::fs::create_dir(&out).unwrap();
        overlay.build(&out).unwrap();
        assert_eq!(
            std::fs::read(out.join("dbc/Spell.dbc")).unwrap(),
            b"patched"
        );
        assert_eq!(std::fs::read(out.join("dbc/Item.dbc")).unwrap(), b"item");
        assert_eq!(std::fs::read(out.join("maps/0001.map")).unwrap(), b"map");
        // Removing the overlay never touches the shared data behind links.
        fsutil::remove_dir_all(&out).unwrap();
        assert_eq!(std::fs::read(shared.join("maps/0001.map")).unwrap(), b"map");
        assert_eq!(
            std::fs::read(shared.join("dbc/Spell.dbc")).unwrap(),
            b"original"
        );
    }
}
