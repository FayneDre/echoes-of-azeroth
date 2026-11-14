//! Instance config plan (ADR 0007) against the real catalog.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eoa_conf::LayerKind;
use eoa_core::manifest::ConfigValue;
use eoa_engine::install::{databases, plan_configs, ConfigFilePlan, InstanceValues};

fn catalog() -> eoa_core::Catalog {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
    eoa_catalog::load_dir(&root).expect("catalog loads")
}

fn plan(modules: &[&str]) -> Vec<ConfigFilePlan> {
    let catalog = catalog();
    let resolution = eoa_core::resolve(&catalog, "azerothcore", None, modules).unwrap();
    let ports = BTreeMap::from([
        ("auth".to_owned(), 3724),
        ("world".to_owned(), 8085),
        ("soap".to_owned(), 7878),
    ]);
    let values = InstanceValues {
        root: Path::new(r"C:\eoa\instances\solo"),
        logs: Path::new(r"C:\eoa\instances\solo\logs"),
        sql: Path::new(r"C:\eoa\instances\solo\sql"),
        data: Path::new(r"C:\eoa\serverdata\wow-3-3-5a-v20.0"),
        mysql_client: Path::new(r"C:\eoa\components\mysql-8-4\bin\mysql.exe"),
        ports: &ports,
        bind: "127.0.0.1",
        db_port: 3307,
        db_user: "eoa",
        db_password: "s3cret",
    };
    plan_configs(&catalog, &resolution, &values).unwrap()
}

/// Final value of `key` in `file` and the kind of layer that decided it.
fn effective(plans: &[ConfigFilePlan], file: &str, key: &str) -> Option<(ConfigValue, LayerKind)> {
    let plan = plans.iter().find(|p| p.file == file)?;
    let mut layers: Vec<_> = plan.layers.iter().collect();
    layers.sort_by_key(|l| l.kind);
    layers
        .iter()
        .rev()
        .find_map(|l| l.values.get(key).map(|v| (v.clone(), l.kind)))
}

fn text(s: &str) -> ConfigValue {
    ConfigValue::Text(s.to_owned())
}

#[test]
fn every_file_of_the_full_build_is_planned() {
    let plans = plan(&[]);
    let files: Vec<(&str, &str)> = plans
        .iter()
        .map(|p| (p.file.as_str(), p.dist.as_str()))
        .collect();
    for expected in [
        ("authserver.conf", "authserver.conf.dist"),
        ("dbimport.conf", "dbimport.conf.dist"),
        ("worldserver.conf", "worldserver.conf.dist"),
        ("modules/playerbots.conf", "modules/playerbots.conf.dist"),
        ("modules/mod_ahbot.conf", "modules/mod_ahbot.conf.dist"),
        (
            "modules/individualProgression.conf",
            "modules/individualProgression.conf.dist",
        ),
    ] {
        assert!(files.contains(&expected), "{expected:?} in {files:?}");
    }
}

#[test]
fn launcher_values_are_locked_and_typed() {
    let plans = plan(&[]);
    let launcher = |file, key| {
        let (value, kind) = effective(&plans, file, key).unwrap_or_else(|| panic!("{file} {key}"));
        assert_eq!(kind, LayerKind::Launcher, "{file} {key}");
        value
    };
    assert_eq!(
        launcher("worldserver.conf", "DataDir"),
        text("C:/eoa/serverdata/wow-3-3-5a-v20.0")
    );
    assert_eq!(
        launcher("worldserver.conf", "WorldServerPort"),
        ConfigValue::Integer(8085)
    );
    assert_eq!(
        launcher("authserver.conf", "RealmServerPort"),
        ConfigValue::Integer(3724)
    );
    assert_eq!(launcher("worldserver.conf", "BindIP"), text("127.0.0.1"));
    // Only the launcher migrates databases (after a backup).
    assert_eq!(
        launcher("worldserver.conf", "Updates.EnableDatabases"),
        ConfigValue::Integer(0)
    );
    assert_eq!(
        launcher("authserver.conf", "Updates.EnableDatabases"),
        ConfigValue::Integer(0)
    );
    assert_eq!(
        launcher("dbimport.conf", "MySQLExecutable"),
        text("C:/eoa/components/mysql-8-4/bin/mysql.exe")
    );
    for (file, key, db) in [
        ("authserver.conf", "LoginDatabaseInfo", "acore_auth"),
        (
            "worldserver.conf",
            "CharacterDatabaseInfo",
            "acore_characters",
        ),
        ("dbimport.conf", "WorldDatabaseInfo", "acore_world"),
        (
            "modules/playerbots.conf",
            "PlayerbotsDatabaseInfo",
            "acore_playerbots",
        ),
    ] {
        assert_eq!(
            launcher(file, key),
            text(&format!("127.0.0.1;3307;eoa;s3cret;{db}"))
        );
    }
}

#[test]
fn modules_not_chosen_are_disabled_and_their_sql_not_allowed() {
    let plans = plan(&[]);
    // AHBot reads its own tables even while disabled (ADR 0008); Individual
    // Progression's SQL changes game content, so it is never applied unchosen.
    assert_eq!(
        effective(&plans, "dbimport.conf", "Updates.AllowedModules")
            .unwrap()
            .0,
        text("mod-playerbots,mod-ah-bot")
    );
    assert_eq!(
        effective(
            &plans,
            "modules/mod_ahbot.conf",
            "AuctionHouseBot.EnableSeller"
        ),
        Some((ConfigValue::Integer(0), LayerKind::CoreOverride))
    );
    assert_eq!(
        effective(
            &plans,
            "modules/individualProgression.conf",
            "IndividualProgression.Enable"
        ),
        Some((ConfigValue::Integer(0), LayerKind::CoreOverride))
    );
    // Its core overrides only apply when chosen.
    assert_eq!(
        effective(&plans, "worldserver.conf", "EnablePlayerSettings"),
        None
    );
    // Mandatory module options get their defaults (never reduced, principle 2).
    assert_eq!(
        effective(
            &plans,
            "modules/playerbots.conf",
            "Playerbots.MaxRandomBots"
        ),
        Some((ConfigValue::Integer(500), LayerKind::ModuleDefault))
    );
}

#[test]
fn chosen_modules_are_enabled_with_their_overrides() {
    let plans = plan(&["mod-ah-bot", "mod-individual-progression"]);
    let allowed = effective(&plans, "dbimport.conf", "Updates.AllowedModules")
        .unwrap()
        .0;
    let ConfigValue::Text(allowed) = allowed else {
        panic!()
    };
    let mut allowed: Vec<&str> = allowed.split(',').collect();
    allowed.sort();
    assert_eq!(
        allowed,
        ["mod-ah-bot", "mod-individual-progression", "mod-playerbots"]
    );
    assert_eq!(
        effective(
            &plans,
            "modules/mod_ahbot.conf",
            "AuctionHouseBot.EnableSeller"
        ),
        Some((ConfigValue::Integer(1), LayerKind::ModuleDefault))
    );
    assert_eq!(
        effective(&plans, "worldserver.conf", "EnablePlayerSettings"),
        Some((ConfigValue::Integer(1), LayerKind::CoreOverride))
    );
    assert_eq!(
        effective(&plans, "worldserver.conf", "DBC.EnforceItemAttributes"),
        Some((ConfigValue::Integer(0), LayerKind::CoreOverride))
    );
}

#[test]
fn databases_include_module_ones() {
    let catalog = catalog();
    let resolution = eoa_core::resolve(&catalog, "azerothcore", None, &[]).unwrap();
    let names: Vec<String> = databases(&catalog, &resolution)
        .unwrap()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(
        names,
        [
            "acore_auth",
            "acore_characters",
            "acore_world",
            "acore_playerbots"
        ]
    );
}
