//! Instance steps: database lifecycle, config rendering, server tools.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use super::config::ConfigFilePlan;
use crate::{fsutil, DirSwap, EngineError, Step, StepContext, StepError, StepFuture};

/// Marker in the database root: which execution created it (undo removes the
/// database only when this execution did, never an existing instance's).
const CREATED_BY: &str = "created-by";

/// `db-init`: initialises the instance's private MySQL (root password, my.ini).
///
/// Refuses a database it did not create. `undo` stops it and deletes it only
/// if this execution created it: a new instance holds no player data yet.
pub struct DbInit {
    pub db: Arc<eoa_db::Instance>,
}

impl<C: Sync> Step<C> for DbInit {
    fn id(&self) -> &str {
        "db-init"
    }
    fn kind(&self) -> &str {
        "db-init"
    }
    fn fingerprint(&self) -> String {
        format!(
            "{} port {}",
            self.db.root().display(),
            self.db.settings().port
        )
    }
    fn run<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            let marker = self.db.root().join(CREATED_BY);
            let ours = std::fs::read_to_string(&marker).is_ok_and(|r| r == cx.run_id());
            if self.db.is_initialized() {
                return if ours {
                    Ok(())
                } else {
                    Err(Box::new(EngineError::InvalidOperation(format!(
                        "a database already exists in {}; it is never overwritten",
                        self.db.root().display()
                    ))) as StepError)
                };
            }
            if !ours {
                let root = self.db.root();
                std::fs::create_dir_all(root).map_err(EngineError::io(root))?;
                fsutil::write_atomic(&marker, cx.run_id().as_bytes())?;
            }
            Ok(self.db.initialize().await?)
        })
    }
    fn undo<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            let marker = self.db.root().join(CREATED_BY);
            if std::fs::read_to_string(&marker).is_ok_and(|r| r == cx.run_id()) {
                self.db.stop().await?;
                fsutil::remove_dir_all(self.db.root())?;
            }
            Ok(())
        })
    }
}

/// `db-start`: starts MySQL; `undo` stops it cleanly.
pub struct DbStart {
    pub db: Arc<eoa_db::Instance>,
}

impl<C: Sync> Step<C> for DbStart {
    fn id(&self) -> &str {
        "db-start"
    }
    fn kind(&self) -> &str {
        "db-start"
    }
    fn run<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move { Ok(self.db.start().await?) })
    }
    fn undo<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move { Ok(self.db.stop().await?) })
    }
}

/// `db-provision`: creates the databases and the service account.
pub struct DbProvision {
    pub db: Arc<eoa_db::Instance>,
    pub databases: Vec<String>,
    pub user: String,
    pub password: String,
}

impl<C: Sync> Step<C> for DbProvision {
    fn id(&self) -> &str {
        "db-provision"
    }
    fn kind(&self) -> &str {
        "db-provision"
    }
    fn fingerprint(&self) -> String {
        // The password is not part of the plan identity (and never journaled).
        format!("{} as {}", self.databases.join(","), self.user)
    }
    fn run<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            for name in &self.databases {
                self.db.create_database(name).await?;
            }
            let names: Vec<&str> = self.databases.iter().map(String::as_str).collect();
            Ok(self
                .db
                .ensure_user(&self.user, &self.password, &names)
                .await?)
        })
    }
}

/// `db-stop`: clean shutdown at the end of the operation.
pub struct DbStop {
    pub db: Arc<eoa_db::Instance>,
}

impl<C: Sync> Step<C> for DbStop {
    fn id(&self) -> &str {
        "db-stop"
    }
    fn kind(&self) -> &str {
        "db-stop"
    }
    fn run<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move { Ok(self.db.stop().await?) })
    }
}

/// `render-config`: renders every config file into `configs/`, atomically.
///
/// Reads each `.conf.dist` from the build (`dist_dir`), applies the layers
/// with `eoa_conf::render`, and swaps the whole directory in. A copy of what
/// was written is kept in `configs/.eoa-rendered/`, the reference for
/// importing hand edits later (`eoa_conf::manual_changes`).
pub struct RenderConfigs {
    pub dist_dir: PathBuf,
    pub target: PathBuf,
    pub files: Vec<ConfigFilePlan>,
}

impl RenderConfigs {
    fn swap(&self, cx: &StepContext<'_, impl Sync>) -> Result<DirSwap, StepError> {
        Ok(DirSwap::new(&self.target, cx.run_id())?)
    }

    fn render_into(&self, staging: &Path) -> Result<(), StepError> {
        for plan in &self.files {
            let dist = self.dist_dir.join(&plan.dist);
            let text = std::fs::read_to_string(&dist).map_err(|e| {
                EngineError::io(&dist)(std::io::Error::new(
                    e.kind(),
                    format!("missing from the server build: {e}"),
                ))
            })?;
            let rendered = eoa_conf::render(&text, &plan.layers)?;
            for out in [
                staging.join(&plan.file),
                staging.join(".eoa-rendered").join(&plan.file),
            ] {
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent).map_err(EngineError::io(parent))?;
                }
                fsutil::write_atomic(&out, rendered.text.as_bytes())?;
            }
        }
        Ok(())
    }
}

impl<C: Sync> Step<C> for RenderConfigs {
    fn id(&self) -> &str {
        "render-config"
    }
    fn kind(&self) -> &str {
        "render-config"
    }
    /// Hash of the layers (they contain the service password: never stored raw).
    fn fingerprint(&self) -> String {
        let plan = format!("{:?} {:?}", self.target, self.files);
        hex::encode(Sha256::digest(plan.as_bytes()))
    }
    fn run<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            let swap = self.swap(cx)?;
            let staging = swap.prepare()?;
            self.render_into(&staging)?;
            Ok(swap.commit()?)
        })
    }
    fn undo<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move { Ok(self.swap(cx)?.undo()?) })
    }
    fn finalize<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move { Ok(self.swap(cx)?.finalize()?) })
    }
}

/// `run-tool`: runs a server tool (dbimport...) with cwd = instance root,
/// output appended to a log file. A non-zero exit fails the step with the end
/// of the log.
pub struct RunTool {
    pub id: String,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub log: PathBuf,
}

#[derive(Debug, thiserror::Error)]
#[error("{tool} exited with {code}; end of {log}:\n{tail}")]
pub struct ToolFailed {
    pub tool: String,
    pub code: String,
    pub log: PathBuf,
    pub tail: String,
}

impl<C: Sync> Step<C> for RunTool {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> &str {
        "run-tool"
    }
    fn fingerprint(&self) -> String {
        format!(
            "{} {:?} in {}",
            self.executable.display(),
            self.args,
            self.cwd.display()
        )
    }
    fn run<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            if let Some(dir) = self.log.parent() {
                std::fs::create_dir_all(dir).map_err(EngineError::io(dir))?;
            }
            let open = || {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.log)
                    .map_err(EngineError::io(&self.log))
            };
            let (out, err) = (open()?, open()?);
            let status = tokio::process::Command::new(&self.executable)
                .args(&self.args)
                .current_dir(&self.cwd)
                .stdin(Stdio::null())
                .stdout(out)
                .stderr(err)
                .status()
                .await
                .map_err(EngineError::io(&self.executable))?;
            if status.success() {
                return Ok(());
            }
            let text = std::fs::read_to_string(&self.log).unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            let tail = lines[lines.len().saturating_sub(30)..].join("\n");
            Err(Box::new(ToolFailed {
                tool: self.executable.display().to_string(),
                code: status
                    .code()
                    .map_or_else(|| "no exit code".to_owned(), |c| format!("code {c}")),
                log: self.log.clone(),
                tail,
            }) as StepError)
        })
    }
}
