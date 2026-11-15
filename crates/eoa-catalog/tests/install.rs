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
    let options = BTreeMap::new();
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
        options: &options,
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

#[test]
fn ahbot_account_and_character_come_from_its_post_install_bindings() {
    use eoa_engine::install::bindings::Bindings;
    let plans = plan(&["mod-ah-bot"]);
    let ahbot = plans
        .iter()
        .find(|p| p.file == "modules/mod_ahbot.conf")
        .unwrap();
    // Before post-install: the keys keep their upstream default (absent).
    let before = Bindings::default().resolve(&ahbot.layers).unwrap();
    assert!(before
        .iter()
        .all(|l| !l.values.contains_key("AuctionHouseBot.Account")));
    let mut bindings = Bindings::default();
    bindings.set("mod-ah-bot", "ahbot_account.id", 104);
    bindings.set("mod-ah-bot", "ahbot_character.guid", 1002);
    let after = bindings.resolve(&ahbot.layers).unwrap();
    let value = |key: &str| {
        after
            .iter()
            .filter(|l| l.kind == LayerKind::Launcher)
            .find_map(|l| l.values.get(key).cloned())
    };
    assert_eq!(
        value("AuctionHouseBot.Account"),
        Some(ConfigValue::Integer(104))
    );
    assert_eq!(
        value("AuctionHouseBot.GUID"),
        Some(ConfigValue::Integer(1002))
    );
    // Not chosen: no binding layer at all.
    let off = plan(&[]);
    let off = off
        .iter()
        .find(|p| p.file == "modules/mod_ahbot.conf")
        .unwrap();
    assert!(off
        .layers
        .iter()
        .all(|l| !l.values.contains_key("AuctionHouseBot.GUID")));
}

/// Individual Progression's choices select its upstream files (ADR 0013).
#[test]
fn option_choices_select_module_assets() {
    use eoa_core::manifest::OptionValue;
    use eoa_engine::install::assets::selected;

    let catalog = catalog();
    let module = "mod-individual-progression";
    let resolution = eoa_core::resolve(&catalog, "azerothcore", None, &[module]).unwrap();
    let none = selected(&catalog, &resolution, &BTreeMap::new()).unwrap();
    assert!(none.is_empty(), "defaults install no file");

    let options = BTreeMap::from([(
        module.to_owned(),
        BTreeMap::from([
            (
                "mana-costs".to_owned(),
                OptionValue::Choice("era-adjusted".into()),
            ),
            (
                "vanilla-visuals".to_owned(),
                OptionValue::Choice("on".into()),
            ),
            (
                "classic-spells".to_owned(),
                OptionValue::Choice("on".into()),
            ),
        ]),
    )]);
    let chosen = selected(&catalog, &resolution, &options).unwrap();
    let ids: Vec<&str> = chosen.iter().map(|a| a.asset.id.as_str()).collect();
    assert_eq!(ids, ["patch-v", "vanilla-visuals", "classic-spells"]);
    let commit = &catalog.emulator("azerothcore").unwrap().modules[module]
        .source
        .commit;
    assert_eq!(
        chosen[0].url,
        format!(
            "https://raw.githubusercontent.com/ZhengPeiRu21/mod-individual-progression/{commit}/optional/patch-V.7z"
        )
    );
    let server: Vec<&str> = chosen
        .iter()
        .flat_map(|a| a.asset.server.iter().map(|f| f.to.as_str()))
        .collect();
    assert!(server.contains(&"dbc/Spell.dbc"));
    assert!(server.contains(&"dbc/SkillLine.dbc"));

    // A choice that no longer exists falls back to the default.
    let stale = BTreeMap::from([(
        module.to_owned(),
        BTreeMap::from([("mana-costs".to_owned(), OptionValue::Choice("gone".into()))]),
    )]);
    assert!(selected(&catalog, &resolution, &stale).unwrap().is_empty());
}
