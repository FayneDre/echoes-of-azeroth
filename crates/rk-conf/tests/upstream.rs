//! Round-trip check against real upstream `.conf.dist` files.
//!
//! Skipped unless `RK_CONF_SAMPLES` lists files (separated by the platform path
//! separator), e.g. the fork's `worldserver.conf.dist` and module configs.

use rk_conf::{manual_changes, ConfFile};

#[test]
fn upstream_files_round_trip() {
    let Some(samples) = std::env::var_os("RK_CONF_SAMPLES") else {
        eprintln!("RK_CONF_SAMPLES not set, skipping");
        return;
    };
    for path in std::env::split_paths(&samples) {
        let text = std::fs::read_to_string(&path).unwrap();
        let normalized: String = text.lines().map(|l| format!("{l}\n")).collect();
        let file = ConfFile::parse(&text);
        assert_eq!(file.render(), normalized, "{}", path.display());
        assert!(manual_changes(&text, &text).is_empty());
        assert!(
            file.keys().count() > 0,
            "{}: no keys parsed",
            path.display()
        );
        eprintln!("{}: {} keys", path.display(), file.keys().count());
    }
}
