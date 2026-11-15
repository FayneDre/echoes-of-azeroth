//! Module lifecycle on an existing instance (architecture §3).
//!
//! [`apply_modules_operation`] brings an instance in line with its record:
//!
//! ```text
//! db-start → post-install → render-config → db-import → db-stop
//! ```
//!
//! - `post-install` runs the manifest steps of every chosen module once
//!   (accounts, characters), through the driver; results go to
//!   `bindings.json`, which `set-config` values reference. It refuses to run
//!   while the servers are up. Resumable: an account or character created
//!   just before a crash is found again by name.
//! - `render-config` re-renders every config with the module set, expanding
//!   the bindings; `db-import` applies the chosen modules' SQL.
//!
//! The same operation finishes `eoa install` and every module change.
//! Removing a module whose SQL changed the `world` database needs that
//! database regenerated (ADR 0003): not supported yet, refused up front.

use std::sync::Arc;

use eoa_core::manifest::{EmulatorManifest, PostInstallStep};
use eoa_core::Catalog;
use eoa_engine::install::bindings::Bindings;
use eoa_engine::install::{self, DbStart, DbStop, InstanceRecord, Layout, Secrets};
use eoa_engine::{Operation, Step, StepContext, StepFuture};

use crate::driver::{DriverContext, EmulatorDriver, NewAccount, NewCharacter};
use crate::process::port_open;
use crate::RuntimeError;

/// Length of the random passwords of service accounts nobody logs into.
const SERVICE_PASSWORD_LENGTH: usize = 16;

/// Checks a new module set and saves it in `instance.json`. Returns the
/// previous record, to restore if the following operation is rolled back.
pub fn set_modules(
    catalog: &Catalog,
    layout: &Layout,
    id: &str,
    enable: &[String],
    disable: &[String],
) -> Result<(InstanceRecord, InstanceRecord), RuntimeError> {
    let (old, _) = install::open_existing(layout, id)?;
    let entry = catalog
        .emulator(&old.emulator)
        .ok_or_else(|| install::PlanError::Unknown(format!("emulator '{}'", old.emulator)))?;
    let module_error = |module: &str, message: &str| RuntimeError::Module {
        module: module.to_owned(),
        message: message.to_owned(),
    };
    for module in disable {
        let manifest = entry
            .modules
            .get(module)
            .ok_or_else(|| module_error(module, "unknown module"))?;
        if !manifest.removable {
            return Err(module_error(module, "this module cannot be removed"));
        }
        if !manifest.database.apply_when_disabled {
            return Err(module_error(
                module,
                "removing it needs the world database rebuilt, which is not supported yet",
            ));
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
    // Rejects unknown modules, conflicts and incompatible flavors.
    let selected: Vec<&str> = new.modules.iter().map(String::as_str).collect();
    eoa_core::resolve(catalog, &new.emulator, Some(&new.flavor), &selected)
        .map_err(install::PlanError::from)?;
    install::save_record(layout, &new)?;
    Ok((new, old))
}

/// `instance-<id>`: post-install steps, configs and module SQL of the
/// instance's current module set (see the module docs).
pub fn apply_modules_operation(
    catalog: &Catalog,
    layout: &Layout,
    record: &InstanceRecord,
    secrets: &Secrets,
    driver: Arc<dyn EmulatorDriver>,
) -> Result<Operation<()>, RuntimeError> {
    let plan = install::instance_plan(catalog, layout, record, secrets)?;
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
    let post_install = PostInstall {
        driver,
        db: plan.db.clone(),
        emulator: entry.manifest.clone(),
        record: record.clone(),
        bindings: layout.instance(&record.id).bindings,
        modules: steps,
    };
    Ok(Operation::new(format!("instance-{}", record.id))
        .step(DbStart {
            db: plan.db.clone(),
        })
        .step(post_install)
        .step(plan.render)
        .step(plan.import)
        .step(DbStop { db: plan.db }))
}

/// Runs the chosen modules' `create-account` / `create-character` steps.
struct PostInstall {
    driver: Arc<dyn EmulatorDriver>,
    db: Arc<eoa_db::Instance>,
    emulator: EmulatorManifest,
    record: InstanceRecord,
    bindings: std::path::PathBuf,
    /// Module id -> its steps, in resolution order.
    modules: Vec<(String, Vec<PostInstallStep>)>,
}

impl PostInstall {
    async fn run_steps(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // The ports the servers listen on once ready.
        let ports = self
            .emulator
            .processes
            .iter()
            .filter_map(|p| match &p.readiness {
                eoa_core::manifest::Readiness::Tcp { port } => self.record.ports.get(port),
                eoa_core::manifest::Readiness::LogLine { .. } => None,
            });
        for port in ports {
            if port_open(*port).await {
                return Err(format!(
                    "the server is running (port {port} is open): stop it before changing modules"
                )
                .into());
            }
        }
        let cx = DriverContext {
            db: &self.db,
            emulator: &self.emulator,
            record: &self.record,
            realm_address: std::net::Ipv4Addr::LOCALHOST,
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
                                    password: eoa_db::generate_password(SERVICE_PASSWORD_LENGTH)?,
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
