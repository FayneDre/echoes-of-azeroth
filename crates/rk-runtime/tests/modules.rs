//! Module set changes against the real catalog (no MySQL needed).

use std::path::PathBuf;

use rk_engine::install::{self, Layout, Network, NewInstance};
use rk_runtime::modules::set_modules;

fn catalog() -> rk_core::Catalog {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
    rk_catalog::load_dir(&root).expect("catalog loads")
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn removing_a_world_changing_module_schedules_the_world_rebuild() {
    let catalog = catalog();
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    let request = NewInstance {
        id: "t".into(),
        emulator: "azerothcore".into(),
        flavor: None,
        modules: strings(&["mod-ah-bot", "mod-individual-progression"]),
        network: Network::Solo,
    };
    install::open_or_create(&catalog, &layout, &request).unwrap();

    // AHBot's SQL stays harmless while disabled: nothing to rebuild.
    let (record, _) = set_modules(&catalog, &layout, "t", &[], &strings(&["mod-ah-bot"])).unwrap();
    assert_eq!(record.modules, strings(&["mod-individual-progression"]));
    assert!(record.regenerate.is_empty());

    // Individual Progression changed `world` (and player databases, kept).
    let ip = strings(&["mod-individual-progression"]);
    let (record, old) = set_modules(&catalog, &layout, "t", &[], &ip).unwrap();
    assert!(record.modules.is_empty());
    assert_eq!(record.regenerate, strings(&["acore_world"]));
    assert_eq!(old.modules, ip);

    // The same command again (resuming) keeps the same plan.
    let (again, _) = set_modules(&catalog, &layout, "t", &[], &ip).unwrap();
    assert_eq!(again, record);
    let (saved, _) = install::open_existing(&layout, "t").unwrap();
    assert_eq!(saved, record);

    // Once applied, the request is cleared.
    let done = rk_runtime::modules::finished(&layout, &record).unwrap();
    assert!(done.regenerate.is_empty());

    // Mandatory modules are refused.
    let error = set_modules(&catalog, &layout, "t", &[], &strings(&["mod-playerbots"]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("cannot be removed"), "{error}");
}
