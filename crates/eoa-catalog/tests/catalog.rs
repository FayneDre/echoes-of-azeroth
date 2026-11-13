//! Integration tests against the real catalog shipped in the repository.

use std::path::PathBuf;

use eoa_core::{resolve, ResolveError};

fn catalog() -> eoa_core::Catalog {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
    eoa_catalog::load_dir(&root).expect("catalog loads")
}

#[test]
fn repository_catalog_is_consistent() {
    let issues = eoa_catalog::validate(&catalog());
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
    assert_eq!(resolution.modules, vec!["mod-playerbots"]);
}

#[test]
fn playerbots_is_mandatory() {
    let catalog = catalog();
    assert_eq!(
        catalog.mandatory_modules("azerothcore", "playerbot"),
        vec!["mod-playerbots"]
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
        vec!["mod-ah-bot", "mod-individual-progression", "mod-playerbots"]
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
    let issues = eoa_catalog::validate(&catalog);
    assert!(
        issues.iter().any(|i| i
            .message
            .contains("'mod-ah-bot' is required through a required module")),
        "{issues:?}"
    );
}
