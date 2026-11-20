//! Integration tests against the real catalog shipped in the repository.

use std::path::PathBuf;

use rk_core::{resolve, ResolveError};

fn catalog() -> rk_core::Catalog {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
    rk_catalog::load_dir(&root).expect("catalog loads")
}

#[test]
fn repository_catalog_is_consistent() {
    let issues = rk_catalog::validate(&catalog());
    assert!(
        issues.is_empty(),
        "catalog issues:\n{}",
        issues
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn playerbots_is_always_installed() {
    let resolution = resolve(&catalog(), "azerothcore", None, &[]).expect("resolves");
    assert_eq!(resolution.flavor, "playerbot");
    assert_eq!(
        resolution.modules,
        vec!["mod-playerbots", "mod-realmkeeper"]
    );
}

#[test]
fn playerbots_is_mandatory() {
    let catalog = catalog();
    assert_eq!(
        catalog.mandatory_modules("azerothcore", "playerbot"),
        vec!["mod-playerbots", "mod-realmkeeper"]
    );
    assert!(
        !catalog
            .module("azerothcore", "mod-playerbots")
            .unwrap()
            .removable
    );
}

#[test]
fn optional_modules_resolve_with_core_overrides() {
    let resolution = resolve(
        &catalog(),
        "azerothcore",
        None,
        &["mod-ah-bot", "mod-individual-progression"],
    )
    .expect("resolves");
    assert_eq!(
        resolution.modules,
        vec![
            "mod-ah-bot",
            "mod-individual-progression",
            "mod-playerbots",
            "mod-realmkeeper"
        ]
    );
    let worldserver = &resolution.core_overrides["worldserver.conf"];
    assert_eq!(
        worldserver["EnablePlayerSettings"].1,
        "mod-individual-progression"
    );
    assert!(worldserver.contains_key("DBC.EnforceItemAttributes"));
}

#[test]
fn unknown_module_is_rejected() {
    let error = resolve(&catalog(), "azerothcore", None, &["mod-does-not-exist"]).unwrap_err();
    assert_eq!(
        error,
        ResolveError::UnknownModule("mod-does-not-exist".into())
    );
}

#[test]
fn modules_pulled_in_by_a_required_module_must_not_be_removable() {
    let mut catalog = catalog();
    let entry = catalog.emulators.get_mut("azerothcore").unwrap();
    // Make Playerbots depend on a removable optional module.
    entry
        .modules
        .get_mut("mod-playerbots")
        .unwrap()
        .requires
        .push("mod-ah-bot".into());
    let issues = rk_catalog::validate(&catalog);
    assert!(
        issues.iter().any(|i| i
            .message
            .contains("'mod-ah-bot' is required through a required module")),
        "{issues:?}"
    );
}

#[test]
fn instance_placeholders_are_checked() {
    use rk_core::manifest::ConfigValue;
    let mut catalog = catalog();
    let emu = &mut catalog.emulators.get_mut("azerothcore").unwrap().manifest;
    let world = emu.instance.config.get_mut("worldserver.conf").unwrap();
    world.insert("A".into(), ConfigValue::Text("${dir.nowhere}".into()));
    world.insert("B".into(), ConfigValue::Text("${port.missing}".into()));
    world.insert("C".into(), ConfigValue::Text("${dir.logs".into()));
    emu.instance.connection.push_str(";${dir.logs}");
    let messages: Vec<String> = rk_catalog::validate(&catalog)
        .into_iter()
        .map(|i| i.message)
        .collect();
    for expected in [
        "worldserver.conf A: unknown placeholder '${dir.nowhere}'",
        "worldserver.conf B: unknown placeholder '${port.missing}'",
        "worldserver.conf C: unterminated placeholder",
        "instance.connection: unknown placeholder '${dir.logs}'",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(expected)),
            "{expected} in {messages:?}"
        );
    }
}

#[test]
fn server_data_comes_from_a_download_or_an_extraction() {
    use rk_core::manifest::ServerDataSource;
    let mut catalog = catalog();
    assert!(matches!(
        catalog.server_data_source("azerothcore"),
        Some(ServerDataSource::Download(_))
    ));
    assert!(matches!(
        catalog.server_data_source("pandaria"),
        Some(ServerDataSource::Extract(_))
    ));

    let emu = &mut catalog.emulators.get_mut("pandaria").unwrap().manifest;
    let extraction = emu.data_extraction.as_mut().unwrap();
    extraction.locale = "deDE".into();
    let step = extraction.steps[0].clone();
    extraction.steps.push(step);
    extraction.steps[1].args.push("${client.nowhere}".into());
    catalog.clients.get_mut("wow-3-3-5a").unwrap().server_data = None;
    let messages: Vec<String> = rk_catalog::validate(&catalog)
        .into_iter()
        .map(|i| i.message)
        .collect();
    for expected in [
        "needs locale 'deDE'",
        "extraction step 'maps' declared twice",
        "uses unknown '${client.nowhere}'",
        "no server data: client 'wow-3-3-5a'",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(expected)),
            "{expected} in {messages:?}"
        );
    }
}

/// ADR 0025: the party module offers a tank, a healer and damage for the
/// classes that have those specs, with Playerbots' premade spec numbers.
#[test]
fn party_offers_every_role() {
    use rk_core::manifest::PartyRole;
    let catalog = catalog();
    let party = catalog
        .module("azerothcore", "mod-realmkeeper")
        .and_then(|m| m.party.as_ref())
        .expect("party");
    assert_eq!(party.size, 5);
    assert_eq!(party.classes.len(), 10);
    for role in [PartyRole::Tank, PartyRole::Healer, PartyRole::Dps] {
        assert!(party
            .classes
            .iter()
            .any(|c| c.specs.iter().any(|s| s.role == role)));
    }
    let warrior = party.classes.iter().find(|c| c.id == "warrior").unwrap();
    let prot = warrior
        .specs
        .iter()
        .find(|s| s.role == PartyRole::Tank)
        .unwrap();
    assert_eq!(prot.number, 2, "Playerbots.PremadeSpecName.1.2 = prot pve");
    let dk = party
        .classes
        .iter()
        .find(|c| c.id == "death-knight")
        .unwrap();
    assert_eq!(
        (dk.class_id, dk.token.as_str(), dk.min_level),
        (6, "dk", 55)
    );
}
