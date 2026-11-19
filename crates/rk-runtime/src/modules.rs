//! Module lifecycle on an existing instance (architecture §3).
//!
//! [`apply_modules_operation`] brings an instance in line with its record:
//!
//! ```text
//! servers-stopped → [download/unpack assets] → server-data → db-start
//!   → post-install → [regenerate-<db>] → render-config → db-import → db-stop
//! ```
//!
//! - assets: the files the option choices select (ADR 0013), downloaded from
//!   the module's pinned commit; `server-data` overlays their server files
//!   on the shared data (`DataDir`); client files wait for `rk play`.
//!
//! - `post-install` runs the manifest steps of every chosen module once
//!   (accounts, characters), through the driver; results go to
//!   `bindings.json`, which `set-config` values reference. Resumable: an account or character created
//!   just before a crash is found again by name.
//! - `regenerate-<db>` (only after removing a module that changed a content
//!   database, ADR 0003): its tables move aside, dbimport rebuilds it from
//!   the base SQL and the remaining modules; the old copy is dropped at
//!   finalize, or restored on rollback. Player data is never touched.
//! - `render-config` re-renders every config with the module set, expanding
//!   the bindings; `db-import` applies the chosen modules' SQL.
//!
//! The same operation finishes `rk install` and every module change; call
//! [`finished`] once it completes.

use std::path::Path;
use std::sync::Arc;

use rk_engine::install::assets::{self, ServerOverlay};
use rk_store::Store;

use rk_core::manifest::{EmulatorManifest, PostInstallStep, Readiness, REALM_OPTIONS};
use rk_core::Catalog;
use rk_engine::install::bindings::Bindings;
use rk_engine::install::{
    self, DbBackup, DbRegenerate, DbRestore, DbStart, DbStop, InstanceRecord, Layout, Secrets,
};
use rk_engine::steps::Extract;
use rk_engine::{Operation, Step, StepContext, StepFuture};

use crate::driver::{DriverContext, EmulatorDriver, NewAccount, NewCharacter};
use crate::process::port_open;
use crate::RuntimeError;

/// Length of the random passwords of service accounts nobody logs into.
const SERVICE_PASSWORD_LENGTH: usize = 16;

/// Checks a new module set and saves it in `instance.json`. Returns the
/// previous record, to restore if the following operation is rolled back.
///
/// Removing a module whose SQL changed a content database (`playerData:
/// false`) schedules that database for regeneration (`regenerate`).
pub fn set_modules(
    catalog: &Catalog,
    layout: &Layout,
    id: &str,
    enable: &[String],
    disable: &[String],
) -> Result<(InstanceRecord, InstanceRecord), RuntimeError> {
    let (old, _) = install::open_existing(layout, id)?;
    // An interrupted update must finish first (binaries and SQL half-swapped).
    install::ensure_finished(layout, &[install::instance_operation_id(id)])?;
    let entry = catalog
        .emulator(&old.emulator)
        .ok_or_else(|| install::PlanError::Unknown(format!("emulator '{}'", old.emulator)))?;
    let module_error = |module: &str, message: &str| RuntimeError::Module {
        module: module.to_owned(),
        message: message.to_owned(),
    };
    let old_selected: Vec<&str> = old.modules.iter().map(String::as_str).collect();
    let old_resolution = rk_core::resolve(catalog, &old.emulator, Some(&old.flavor), &old_selected)
        .map_err(install::PlanError::from)?;
    let all_databases = install::databases(catalog, &old_resolution)?;
    let mut regenerate = old.regenerate.clone();
    for module in disable {
        let manifest = entry
            .modules
            .get(module)
            .ok_or_else(|| module_error(module, "unknown module"))?;
        if !manifest.removable {
            return Err(module_error(module, "this module cannot be removed"));
        }
        // Already off (e.g. the same command again to resume an interrupted
        // operation): nothing new to schedule.
        if !old.modules.contains(module) {
            continue;
        }
        // Its SQL stays harmless while disabled: nothing to rebuild.
        if manifest.database.apply_when_disabled {
            continue;
        }
        // Content databases it changed are rebuilt without it; player data
        // keeps its tables, inert.
        for touched in &manifest.database.touches {
            let db = all_databases.iter().find(|d| &d.id == touched);
            if let Some(db) = db.filter(|d| !d.player_data) {
                if !regenerate.contains(&db.name) {
                    regenerate.push(db.name.clone());
                }
            }
        }
    }
    let mut new = old.clone();
    new.modules.retain(|m| !disable.contains(m));
    for module in enable {
        if !new.modules.contains(module) {
            new.modules.push(module.clone());
        }
    }
    new.modules.sort();
    new.regenerate = regenerate;
    // Rejects unknown modules, conflicts and incompatible flavors.
    let selected: Vec<&str> = new.modules.iter().map(String::as_str).collect();
    rk_core::resolve(catalog, &new.emulator, Some(&new.flavor), &selected)
        .map_err(install::PlanError::from)?;
    install::save_record(layout, &new)?;
    Ok((new, old))
}

/// `modules-<id>`: post-install steps, configs and module SQL of the
/// instance's current module set (see the module docs).
pub fn apply_modules_operation(
    catalog: &Catalog,
    layout: &Layout,
    record: &InstanceRecord,
    secrets: &Secrets,
    driver: Arc<dyn EmulatorDriver>,
    store: &Arc<Store>,
) -> Result<Operation<()>, RuntimeError> {
    let plan = install::instance_plan(catalog, layout, record, secrets)?;
    let (stopped, post_install) = common_steps(catalog, layout, record, &plan, driver)?;
    // Its own journal: an install interrupted after `instance-<id>` resumes
    // this one without disturbing the finished install journal.
    let mut op = Operation::new(install::modules_operation_id(&record.id)).step(stopped);
    for step in assets::asset_steps(layout, store, &plan.assets) {
        op.push(step);
    }
    let op = op
        .step(overlay(layout, record, &plan))
        .step(DbStart {
            db: plan.db.clone(),
        })
        .step(post_install);
    let op = record.regenerate.iter().fold(op, |op, name| {
        op.step(DbRegenerate::new(plan.db.clone(), name))
    });
    Ok(op
        .step(plan.render)
        .maybe_step(plan.import)
        .step(DbStop { db: plan.db }))
}

/// `instance-<id>`: replaces the server build of an instance (a new catalog
/// release: core or module pins changed) and migrates its databases.
///
/// ```text
/// servers-stopped → db-start → db-backup → extract-bin → extract-sql
///   → post-install → render-config → db-import → db-stop
/// ```
///
/// Every database is dumped first (kept in `backups/<id>/`); a failure puts
/// the old binaries, SQL tree and configs back and restores the dump.
///
/// `label` names the dump (`before-<label>.sql`) and makes the plan unique:
/// a repair with the same build is a new operation, a resumed one is not.
#[allow(clippy::too_many_arguments)] // the update inputs, each distinct
pub fn update_operation(
    catalog: &Catalog,
    layout: &Layout,
    record: &InstanceRecord,
    secrets: &Secrets,
    driver: Arc<dyn EmulatorDriver>,
    store: &Arc<Store>,
    server_artifact: &Path,
    label: &str,
) -> Result<Operation<()>, RuntimeError> {
    // An interrupted module change (tables moved aside) must finish first.
    install::ensure_finished(layout, &[install::modules_operation_id(&record.id)])?;
    let plan = install::instance_plan(catalog, layout, record, secrets)?;
    let (stopped, post_install) = common_steps(catalog, layout, record, &plan, driver)?;
    let paths = layout.instance(&record.id);
    let backup = DbBackup {
        db: plan.db.clone(),
        databases: plan.databases.clone(),
        out: layout
            .backups(&record.id)
            .join(format!("before-{label}.sql")),
    };
    let mut op = Operation::new(install::instance_operation_id(&record.id)).step(stopped);
    // The catalog of the new release may pin other asset files.
    for step in assets::asset_steps(layout, store, &plan.assets) {
        op.push(step);
    }
    Ok(op
        .step(DbStart {
            db: plan.db.clone(),
        })
        .step(backup)
        .step(Extract::new("extract-bin", server_artifact, &paths.bin).subdir("dist"))
        .step(Extract::new("extract-sql", server_artifact, &paths.sql).subdir("sql"))
        .step(overlay(layout, record, &plan))
        .step(post_install)
        .step(plan.render)
        .maybe_step(plan.import)
        .step(DbStop { db: plan.db }))
}

/// Puts the player databases back from a backup (ADR 0016):
///
/// ```text
/// servers-stopped → db-start → db-backup → db-restore → db-import → db-stop
/// ```
///
/// Every database is dumped first (`before-<label>.sql`): a failure, or a
/// later change of mind, gets the current state back. Only the player
/// databases the dump holds (`databases`) are replaced; content databases
/// stay as the current build and module set made them, and dbimport
/// migrates the restored ones to the current build.
#[allow(clippy::too_many_arguments)] // the restore inputs, each distinct
pub fn restore_operation(
    catalog: &Catalog,
    layout: &Layout,
    record: &InstanceRecord,
    secrets: &Secrets,
    driver: Arc<dyn EmulatorDriver>,
    dump: &Path,
    databases: Vec<String>,
    label: &str,
) -> Result<Operation<()>, RuntimeError> {
    install::ensure_finished(layout, &[install::modules_operation_id(&record.id)])?;
    let plan = install::instance_plan(catalog, layout, record, secrets)?;
    let (stopped, _) = common_steps(catalog, layout, record, &plan, driver)?;
    Ok(Operation::new(install::instance_operation_id(&record.id))
        .step(stopped)
        .step(DbStart {
            db: plan.db.clone(),
        })
        .step(DbBackup {
            db: plan.db.clone(),
            databases: plan.databases.clone(),
            out: layout
                .backups(&record.id)
                .join(format!("before-{label}.sql")),
        })
        .step(DbRestore {
            db: plan.db.clone(),
            databases,
            dump: dump.to_path_buf(),
            user: secrets.db_user.clone(),
            password: secrets.db_password.clone(),
        })
        .maybe_step(plan.import)
        .step(DbStop { db: plan.db }))
}

/// Every module running on an instance: the flavor's mandatory ones and
/// the chosen ones, in resolution order.
pub fn enabled_modules(
    catalog: &Catalog,
    record: &InstanceRecord,
) -> Result<Vec<String>, RuntimeError> {
    let selected: Vec<&str> = record.modules.iter().map(String::as_str).collect();
    let resolution = rk_core::resolve(catalog, &record.emulator, Some(&record.flavor), &selected)
        .map_err(install::PlanError::from)?;
    Ok(resolution.modules)
}

/// Names of the player databases (`playerData: true`) of an instance.
pub fn player_databases(
    catalog: &Catalog,
    record: &InstanceRecord,
) -> Result<Vec<String>, RuntimeError> {
    let selected: Vec<&str> = record.modules.iter().map(String::as_str).collect();
    let resolution = rk_core::resolve(catalog, &record.emulator, Some(&record.flavor), &selected)
        .map_err(install::PlanError::from)?;
    Ok(install::databases(catalog, &resolution)?
        .into_iter()
        .filter(|d| d.player_data)
        .map(|d| d.name)
        .collect())
}

/// `server-data`: the instance overlay of the selected server files.
fn overlay(
    layout: &Layout,
    record: &InstanceRecord,
    plan: &install::InstancePlan,
) -> ServerOverlay {
    ServerOverlay {
        shared: plan.server_data.clone(),
        target: layout.instance(&record.id).data,
        files: assets::server_files(layout, &plan.assets),
    }
}

/// Sets module option values (`<option>=<value>` already parsed per
/// option) and saves them in `instance.json`. Returns (new, previous)
/// records, like [`set_modules`]; the module must be chosen.
pub fn set_options(
    catalog: &Catalog,
    layout: &Layout,
    id: &str,
    module: &str,
    values: &[(String, String)],
) -> Result<(InstanceRecord, InstanceRecord), RuntimeError> {
    let (old, _) = install::open_existing(layout, id)?;
    install::ensure_finished(layout, &[install::instance_operation_id(id)])?;
    let module_error = |message: String| RuntimeError::Module {
        module: module.to_owned(),
        message,
    };
    let entry = catalog
        .emulator(&old.emulator)
        .ok_or_else(|| module_error("unknown emulator".into()))?;
    // The realm settings, or the options of an enabled module.
    let specs: Vec<&rk_core::manifest::ModuleOption> = if module == REALM_OPTIONS {
        entry
            .manifest
            .settings
            .iter()
            .flat_map(|s| &s.options)
            .collect()
    } else {
        // Mandatory modules are not in `old.modules` but are always on.
        if !enabled_modules(catalog, &old)?.iter().any(|m| m == module) {
            return Err(module_error(
                "this module is not enabled on the instance".into(),
            ));
        }
        let manifest = entry
            .modules
            .get(module)
            .ok_or_else(|| module_error("unknown module".into()))?;
        manifest.options.iter().collect()
    };
    let mut new = old.clone();
    for (option, text) in values {
        let spec = specs
            .iter()
            .find(|o| o.id() == option)
            .ok_or_else(|| module_error(format!("no option '{option}'")))?;
        let value = spec
            .parse_value(text)
            .map_err(|e| module_error(format!("{option}: {e}")))?;
        let options = new.options.entry(module.to_owned()).or_default();
        if value == spec.default_value() {
            options.remove(option);
        } else {
            options.insert(option.clone(), value);
        }
    }
    new.options.retain(|_, v| !v.is_empty());
    install::save_record(layout, &new)?;
    Ok((new, old))
}

/// Backup label of an update from one build key to another.
pub fn update_label(from: Option<&str>, to: &str) -> String {
    let short = |key: &str| key[..key.len().min(16)].to_owned();
    format!(
        "update-{}-to-{}",
        short(from.unwrap_or("unknown")),
        short(to)
    )
}

fn common_steps(
    catalog: &Catalog,
    layout: &Layout,
    record: &InstanceRecord,
    plan: &install::InstancePlan,
    driver: Arc<dyn EmulatorDriver>,
) -> Result<(ServersStopped, PostInstall), RuntimeError> {
    let entry = catalog
        .emulator(&record.emulator)
        .ok_or_else(|| install::PlanError::Unknown(format!("emulator '{}'", record.emulator)))?;
    let steps = plan
        .resolution
        .modules
        .iter()
        .filter_map(|id| entry.modules.get(id))
        .filter(|m| !m.post_install.is_empty())
        .map(|m| (m.id.clone(), m.post_install.clone()))
        .collect();
    // The ports the servers listen on once ready.
    let ports = entry
        .manifest
        .processes
        .iter()
        .filter_map(|p| match &p.readiness {
            Readiness::Tcp { port } => record.ports.get(port).copied(),
            Readiness::LogLine { .. } => None,
        })
        .collect();
    Ok((
        ServersStopped { ports },
        PostInstall {
            driver,
            db: plan.db.clone(),
            emulator: entry.manifest.clone(),
            record: record.clone(),
            bindings: layout.instance(&record.id).bindings,
            configs: layout.instance(&record.id).configs,
            modules: steps,
        },
    ))
}

/// `servers-stopped`: refuses to change an instance while its servers run
/// (binaries in use, guids allocated by worldserver, configs read at start).
/// Callers also hold [`install::lock_instance`], which `Server::start`
/// takes too: this check only catches servers started outside the launcher.
struct ServersStopped {
    ports: Vec<u16>,
}

impl<C: Sync> Step<C> for ServersStopped {
    fn id(&self) -> &str {
        "servers-stopped"
    }
    fn kind(&self) -> &str {
        "check"
    }
    fn fingerprint(&self) -> String {
        format!("{:?}", self.ports)
    }
    fn run<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            for port in &self.ports {
                if port_open(*port).await {
                    return Err(format!(
                        "the server is running (port {port} is open): stop it first"
                    )
                    .into());
                }
            }
            Ok(())
        })
    }
}

/// Clears the regeneration request once the operation has completed.
pub fn finished(layout: &Layout, record: &InstanceRecord) -> Result<InstanceRecord, RuntimeError> {
    let mut record = record.clone();
    if !record.regenerate.is_empty() {
        record.regenerate.clear();
        install::save_record(layout, &record)?;
    }
    Ok(record)
}

/// Runs the chosen modules' `create-account` / `create-character` steps.
struct PostInstall {
    driver: Arc<dyn EmulatorDriver>,
    db: Arc<rk_db::Instance>,
    emulator: EmulatorManifest,
    record: InstanceRecord,
    bindings: std::path::PathBuf,
    configs: std::path::PathBuf,
    /// Module id -> its steps, in resolution order.
    modules: Vec<(String, Vec<PostInstallStep>)>,
}

impl PostInstall {
    async fn run_steps(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let cx = DriverContext {
            db: &self.db,
            emulator: &self.emulator,
            record: &self.record,
            realm_address: std::net::Ipv4Addr::LOCALHOST,
            configs: &self.configs,
            spawns: &[],
        };
        let mut bindings = Bindings::load(&self.bindings)?;
        for (module, steps) in &self.modules {
            for step in steps {
                match step {
                    PostInstallStep::CreateAccount {
                        binding,
                        username,
                        gm_level,
                    } => {
                        let key = format!("{binding}.id");
                        if bindings.get(module, &key).is_some() {
                            continue;
                        }
                        let id = match self.driver.find_account(&cx, username).await? {
                            Some(id) => id,
                            None => {
                                let account = NewAccount {
                                    username: username.clone(),
                                    password: rk_db::generate_password(SERVICE_PASSWORD_LENGTH)?,
                                    gm_level: *gm_level,
                                };
                                self.driver.create_account(&cx, &account).await?
                            }
                        };
                        bindings.set(module, &key, u64::from(id));
                        bindings.save(&self.bindings)?;
                    }
                    PostInstallStep::CreateCharacter {
                        binding,
                        account,
                        name,
                        race,
                        class,
                    } => {
                        let key = format!("{binding}.guid");
                        if bindings.get(module, &key).is_some() {
                            continue;
                        }
                        let account_id = bindings
                            .get(module, &format!("{account}.id"))
                            .ok_or_else(|| format!("{module}: account '{account}' not created"))?
                            as u32;
                        let guid = match self.driver.find_character(&cx, name).await? {
                            Some((owner, guid)) if owner == account_id => guid,
                            Some(_) => {
                                return Err(format!(
                                "{module}: the character name '{name}' belongs to another account"
                            )
                                .into())
                            }
                            None => {
                                let character = NewCharacter {
                                    account_id,
                                    name: name.clone(),
                                    race: race.clone(),
                                    class: class.clone(),
                                };
                                self.driver.create_character(&cx, &character).await?
                            }
                        };
                        bindings.set(module, &key, u64::from(guid));
                        bindings.save(&self.bindings)?;
                    }
                    // Applied by render-config from the bindings.
                    PostInstallStep::SetConfig { .. } => {}
                }
            }
        }
        Ok(())
    }
}

impl<C: Sync> Step<C> for PostInstall {
    fn id(&self) -> &str {
        "post-install"
    }
    fn kind(&self) -> &str {
        "post-install"
    }
    fn fingerprint(&self) -> String {
        format!("{:?}", self.modules)
    }
    /// Accounts and characters are kept on rollback: they live with player
    /// data and are reused by name the next time.
    fn run<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(self.run_steps())
    }
}
