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
            claim(&self.db, cx.run_id())?;
            if self.db.is_initialized() {
                return Ok(()); // ours, finished before an interruption
            }
            Ok(self.db.initialize().await?)
        })
    }
    fn undo<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move { release(&self.db, cx.run_id()).await })
    }
}

/// Marks the database root as created by `run_id`. Refuses a database (or a
/// root) that another execution created: an existing instance's database,
/// which may hold player data, is never overwritten.
fn claim(db: &eoa_db::Instance, run_id: &str) -> Result<(), StepError> {
    let root = db.root();
    let marker = root.join(CREATED_BY);
    if std::fs::read_to_string(&marker).is_ok_and(|r| r == run_id) {
        return Ok(());
    }
    let occupied = db.datadir().exists() || db.is_initialized();
    if occupied {
        return Err(Box::new(EngineError::InvalidOperation(format!(
            "a database already exists in {}; it is never overwritten",
            root.display()
        ))));
    }
    std::fs::create_dir_all(root).map_err(EngineError::io(root))?;
    Ok(fsutil::write_atomic(&marker, run_id.as_bytes())?)
}

/// Undo of [`claim`]: stops and deletes the database only if `run_id`
/// created it (a new instance holds no player data yet).
async fn release(db: &eoa_db::Instance, run_id: &str) -> Result<(), StepError> {
    let marker = db.root().join(CREATED_BY);
    if std::fs::read_to_string(&marker).is_ok_and(|r| r == run_id) {
        db.stop().await?;
        fsutil::remove_dir_all(db.root())?;
    }
    Ok(())
}

/// `db-claim`: reserves the database root for a restore from a snapshot
/// (ADR 0009), with the same rule as [`DbInit`]. Comes before the
/// extraction, so a snapshot never replaces an existing database.
pub struct DbClaim {
    pub db: Arc<eoa_db::Instance>,
}

impl<C: Sync> Step<C> for DbClaim {
    fn id(&self) -> &str {
        "db-claim"
    }
    fn kind(&self) -> &str {
        "db-claim"
    }
    fn fingerprint(&self) -> String {
        self.db.root().display().to_string()
    }
    fn run<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move { claim(&self.db, cx.run_id()) })
    }
    fn undo<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move { release(&self.db, cx.run_id()).await })
    }
}

/// `db-adopt`: takes over the extracted snapshot (new root password, first
/// start with an init file). Leaves MySQL running, like `db-start`.
pub struct DbAdopt {
    pub db: Arc<eoa_db::Instance>,
}

impl<C: Sync> Step<C> for DbAdopt {
    fn id(&self) -> &str {
        "db-adopt"
    }
    fn kind(&self) -> &str {
        "db-adopt"
    }
    fn fingerprint(&self) -> String {
        format!(
            "{} port {}",
            self.db.root().display(),
            self.db.settings().port
        )
    }
    fn run<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            if self.db.is_initialized() {
                return Ok(self.db.start().await?); // adopted before an interruption
            }
            Ok(self.db.adopt().await?)
        })
    }
    fn undo<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move { Ok(self.db.stop().await?) })
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

/// `db-backup`: dumps the instance's databases before a migration
/// (principle 4). The dump is kept after success (it holds player data).
///
/// `undo` puts every database back as it was: each one is dropped, then the
/// dump recreates it (dropping first also removes tables a newer migration
/// created). Resumable: a finished dump is not made again.
pub struct DbBackup {
    pub db: Arc<eoa_db::Instance>,
    pub databases: Vec<String>,
    pub out: PathBuf,
}

impl<C: Sync> Step<C> for DbBackup {
    fn id(&self) -> &str {
        "db-backup"
    }
    fn kind(&self) -> &str {
        "db-backup"
    }
    fn fingerprint(&self) -> String {
        format!("{:?} {:?}", self.databases, self.out)
    }
    /// `out` only exists while the operation is in flight: `finalize` renames
    /// it with a timestamp and a successful `undo` deletes it. A file found
    /// here is therefore this operation's own dump (written atomically, so
    /// complete), never a stale one that would lose later play on rollback.
    fn run<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            if self.out.is_file() {
                return Ok(());
            }
            if let Some(dir) = self.out.parent() {
                std::fs::create_dir_all(dir).map_err(EngineError::io(dir))?;
            }
            let databases: Vec<&str> = self.databases.iter().map(String::as_str).collect();
            Ok(self.db.backup(&databases, &self.out).await?)
        })
    }
    fn undo<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            if !self.out.is_file() {
                return Ok(()); // nothing was changed after a missing dump
            }
            self.db.start().await?;
            for name in &self.databases {
                eoa_db::validate_identifier(name)?;
                self.db
                    .execute(&format!("DROP DATABASE IF EXISTS `{name}`;"))
                    .await?;
            }
            self.db.restore(&self.out).await?;
            // The databases now hold exactly this dump: drop it, so that a
            // later attempt dumps the state of that time.
            std::fs::remove_file(&self.out).map_err(EngineError::io(&self.out))?;
            Ok(())
        })
    }
    /// Keeps the dump under a unique name (`<name>-<unix seconds>.sql`).
    fn finalize<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            if !self.out.is_file() {
                return Ok(()); // finalize resumed after the rename
            }
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let stem = self
                .out
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let kept = self.out.with_file_name(format!("{stem}-{secs}.sql"));
            std::fs::rename(&self.out, &kept).map_err(EngineError::io(&self.out))?;
            Ok(())
        })
    }
}

/// `regenerate-<database>`: empties a content database (never one with
/// `playerData`) so that the following dbimport rebuilds it from the base
/// SQL and the remaining modules' SQL (ADR 0003, module removal).
///
/// The tables are moved, not dropped: one atomic `RENAME TABLE` into
/// `<name>_eoa_previous`, which is instant whatever their size. `undo` puts
/// them back (after dropping what dbimport wrote), `finalize` drops the old
/// copy. Resumable: the state of the two schemas tells how far it went.
pub struct DbRegenerate {
    pub db: Arc<eoa_db::Instance>,
    pub database: String,
    id: String,
}

impl DbRegenerate {
    pub fn new(db: Arc<eoa_db::Instance>, database: impl Into<String>) -> Self {
        let database = database.into();
        Self {
            id: format!("regenerate-{database}"),
            db,
            database,
        }
    }

    fn previous(&self) -> String {
        format!("{}_eoa_previous", self.database)
    }

    async fn tables(&self, schema: &str) -> Result<Vec<String>, StepError> {
        let out = self
            .db
            .execute(&format!(
                "SELECT TABLE_NAME FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = '{schema}' ORDER BY TABLE_NAME;"
            ))
            .await?;
        let tables: Vec<String> = out.lines().map(str::to_owned).collect();
        if let Some(bad) = tables.iter().find(|t| t.contains('`')) {
            return Err(format!("unexpected table name {bad:?} in {schema}").into());
        }
        Ok(tables)
    }

    /// One statement, so all tables move or none does.
    async fn move_tables(&self, from: &str, to: &str) -> Result<(), StepError> {
        let tables = self.tables(from).await?;
        if tables.is_empty() {
            return Ok(());
        }
        let pairs: Vec<String> = tables
            .iter()
            .map(|t| format!("`{from}`.`{t}` TO `{to}`.`{t}`"))
            .collect();
        self.db
            .execute(&format!(
                "SET FOREIGN_KEY_CHECKS = 0;\nRENAME TABLE {};\n",
                pairs.join(", ")
            ))
            .await?;
        Ok(())
    }
}

impl<C: Sync> Step<C> for DbRegenerate {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> &str {
        "db-regenerate"
    }
    fn fingerprint(&self) -> String {
        self.database.clone()
    }
    fn run<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            eoa_db::validate_identifier(&self.database)?;
            let (name, previous) = (&self.database, self.previous());
            let kept = self.tables(&previous).await?;
            let current = self.tables(name).await?;
            match (kept.is_empty(), current.is_empty()) {
                // Moved before a crash; dbimport has not run yet (a later
                // step, so it only runs once this one is done).
                (false, true) => Ok(()),
                // A copy left by an earlier, completed regeneration whose
                // cleanup failed: older than the live tables, drop it.
                (false, false) => {
                    self.db
                        .execute(&format!("DROP DATABASE `{previous}`;"))
                        .await?;
                    self.db.create_database(&previous).await?;
                    self.move_tables(name, &previous).await
                }
                (true, _) => {
                    self.db.create_database(&previous).await?;
                    self.move_tables(name, &previous).await
                }
            }
        })
    }
    fn undo<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            let (name, previous) = (&self.database, self.previous());
            // A rollback may resume after a restart, with MySQL stopped.
            self.db.start().await?;
            if self.tables(&previous).await?.is_empty() {
                return Ok(()); // never moved
            }
            // What dbimport rebuilt goes; the original tables come back.
            self.db.execute(&format!("DROP DATABASE `{name}`;")).await?;
            self.db.create_database(name).await?;
            self.move_tables(&previous, name).await?;
            self.db
                .execute(&format!("DROP DATABASE `{previous}`;"))
                .await?;
            Ok(())
        })
    }
    fn finalize<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            // Runs after db-stop: start MySQL for the cleanup, stop it again.
            let was_running = self.db.ping().await;
            self.db.start().await?;
            let dropped = self
                .db
                .execute(&format!("DROP DATABASE IF EXISTS `{}`;", self.previous()))
                .await;
            if !was_running {
                self.db.stop().await?;
            }
            dropped?;
            Ok(())
        })
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
    /// `bindings.json` of the instance: expands `set-config` placeholders
    /// at run time (they are only known once post-install steps have run).
    pub bindings: Option<PathBuf>,
}

impl RenderConfigs {
    fn swap(&self, cx: &StepContext<'_, impl Sync>) -> Result<DirSwap, StepError> {
        Ok(DirSwap::new(&self.target, cx.run_id())?)
    }

    fn render_into(&self, staging: &Path) -> Result<(), StepError> {
        let bindings = match &self.bindings {
            Some(path) => super::bindings::Bindings::load(path)?,
            None => Default::default(),
        };
        for plan in &self.files {
            let layers = bindings.resolve(&plan.layers)?;
            let dist = self.dist_dir.join(&plan.dist);
            let text = std::fs::read_to_string(&dist).map_err(|e| {
                EngineError::io(&dist)(std::io::Error::new(
                    e.kind(),
                    format!("missing from the server build: {e}"),
                ))
            })?;
            let rendered = eoa_conf::render(&text, &layers)?;
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
        let plan = format!("{:?} {:?} {:?}", self.target, self.files, self.bindings);
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
