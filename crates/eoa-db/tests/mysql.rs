//! Full lifecycle against a real MySQL distribution.
//!
//! Skipped unless `EOA_MYSQL_BASEDIR` points to an unpacked MySQL 8.4 ZIP
//! (the directory containing `bin/mysqld`).

use std::net::TcpListener;

use eoa_db::{generate_password, Binaries, Instance, ServerSettings};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test(flavor = "multi_thread")]
async fn lifecycle_init_provision_backup_restore_restart() {
    let Some(basedir) = std::env::var_os("EOA_MYSQL_BASEDIR") else {
        eprintln!("EOA_MYSQL_BASEDIR not set, skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let instance = Instance::new(
        dir.path().join("mysql"),
        Binaries::from_basedir(basedir),
        ServerSettings {
            port: free_port(),
            buffer_pool_mib: 128,
            ..Default::default()
        },
    );

    instance.initialize().await.unwrap();
    assert!(instance.is_initialized());
    assert!(instance.initialize().await.is_err(), "second init refused");
    assert!(!instance.ping().await);

    instance.start().await.unwrap();
    instance.start().await.unwrap(); // idempotent
    assert!(instance.ping().await);

    // Provision like an AzerothCore realm.
    for db in ["acore_auth", "acore_characters"] {
        instance.create_database(db).await.unwrap();
    }
    let password = generate_password(24).unwrap();
    instance
        .ensure_user("acore", &password, &["acore_auth", "acore_characters"])
        .await
        .unwrap();
    instance
        .ensure_user("acore", &password, &["acore_auth"])
        .await
        .unwrap(); // idempotent
    instance
        .execute(
            "CREATE TABLE acore_characters.characters (guid INT PRIMARY KEY, name VARCHAR(12)) ENGINE=InnoDB;\n\
             INSERT INTO acore_characters.characters VALUES (1, 'Thrall'), (2, 'Jaina');",
        )
        .await
        .unwrap();

    // Backup, lose the data, restore.
    let dump = dir.path().join("characters.sql");
    instance.backup(&["acore_characters"], &dump).await.unwrap();
    assert!(std::fs::metadata(&dump).unwrap().len() > 0);
    instance
        .execute("DROP DATABASE acore_characters;")
        .await
        .unwrap();
    assert!(!instance.database_exists("acore_characters").await.unwrap());
    instance.restore(&dump).await.unwrap();
    let names = instance
        .execute("SELECT name FROM acore_characters.characters ORDER BY guid;")
        .await
        .unwrap();
    assert_eq!(names.lines().collect::<Vec<_>>(), vec!["Thrall", "Jaina"]);

    // Clean stop, then data survives a restart.
    instance.stop().await.unwrap();
    assert!(!instance.ping().await);
    instance.stop().await.unwrap(); // idempotent
    instance.start().await.unwrap();
    assert!(instance.database_exists("acore_auth").await.unwrap());
    instance.stop().await.unwrap();
    assert!(
        instance
            .log_tail(5)
            .to_lowercase()
            .contains("shutdown complete"),
        "{}",
        instance.log_tail(5)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn occupied_port_is_reported() {
    let Some(basedir) = std::env::var_os("EOA_MYSQL_BASEDIR") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let blocker = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = blocker.local_addr().unwrap().port();
    let instance = Instance::new(
        dir.path().join("mysql"),
        Binaries::from_basedir(basedir),
        ServerSettings {
            port,
            buffer_pool_mib: 128,
            ..Default::default()
        },
    );
    instance.initialize().await.unwrap();
    let error = instance.start().await.unwrap_err();
    assert!(error.to_string().contains(&port.to_string()), "{error}");
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupted_initialisation_is_redone() {
    let Some(basedir) = std::env::var_os("EOA_MYSQL_BASEDIR") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("mysql");
    // State left by a crash during bootstrap: marker, credentials, partial data.
    std::fs::create_dir_all(root.join("data")).unwrap();
    std::fs::write(root.join("data").join("ibdata1"), b"partial").unwrap();
    std::fs::write(root.join("init.pending"), b"").unwrap();
    std::fs::write(root.join("client.cnf"), b"[client]\nuser=root\n").unwrap();
    let instance = Instance::new(
        &root,
        Binaries::from_basedir(basedir),
        ServerSettings {
            port: free_port(),
            buffer_pool_mib: 128,
            ..Default::default()
        },
    );
    assert!(!instance.is_initialized());
    instance.initialize().await.unwrap();
    assert!(instance.is_initialized());
    assert!(!root.join("init.pending").exists());
    instance.start().await.unwrap();
    assert!(instance.ping().await, "new credentials work");
    instance.stop().await.unwrap();
}

/// Copies a stopped data directory the way the CI snapshot does (ADR 0009):
/// no server identity, keys, temporary tablespaces or spare redo files.
fn copy_snapshot(from: &std::path::Path, to: &std::path::Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let excluded = name == "auto.cnf"
            || name == "#innodb_temp"
            || name.ends_with(".pem")
            || name.ends_with("_tmp");
        if excluded {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_snapshot(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_is_adopted_with_new_credentials() {
    let Some(basedir) = std::env::var_os("EOA_MYSQL_BASEDIR") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let settings = |port| ServerSettings {
        port,
        buffer_pool_mib: 128,
        ..Default::default()
    };
    let source = Instance::new(
        dir.path().join("source"),
        Binaries::from_basedir(&basedir),
        settings(free_port()),
    );
    source.initialize().await.unwrap();
    source.start().await.unwrap();
    source
        .execute(
            "CREATE DATABASE acore_world;\n\
             CREATE TABLE acore_world.updates (name VARCHAR(200) PRIMARY KEY) ENGINE=InnoDB;\n\
             INSERT INTO acore_world.updates VALUES ('2026_01_01_00.sql');\n\
             SET GLOBAL innodb_fast_shutdown = 0;",
        )
        .await
        .unwrap();
    let source_uuid = source.execute("SELECT @@server_uuid;").await.unwrap();
    source.stop().await.unwrap();

    let restored = Instance::new(
        dir.path().join("restored"),
        Binaries::from_basedir(&basedir),
        settings(free_port()),
    );
    std::fs::create_dir_all(restored.datadir()).unwrap();
    copy_snapshot(&source.datadir(), &restored.datadir());
    assert!(!restored.is_initialized());
    assert!(
        restored.start().await.is_err(),
        "not startable before adoption"
    );

    restored.adopt().await.unwrap();
    assert!(restored.is_initialized());
    assert!(restored.ping().await);
    assert!(!dir.path().join("restored").join("init.sql").exists());
    assert!(restored.adopt().await.is_err(), "adopted only once");
    let names = restored
        .execute("SELECT name FROM acore_world.updates;")
        .await
        .unwrap();
    assert_eq!(names.trim(), "2026_01_01_00.sql");
    let uuid = restored.execute("SELECT @@server_uuid;").await.unwrap();
    assert_ne!(uuid, source_uuid, "new server identity");
    let credentials =
        |name: &str| std::fs::read_to_string(dir.path().join(name).join("client.cnf")).unwrap();
    assert_ne!(credentials("source"), credentials("restored"));

    restored.stop().await.unwrap();
    restored.start().await.unwrap(); // a normal instance from now on
    restored.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupted_adoption_is_resumed() {
    let Some(basedir) = std::env::var_os("EOA_MYSQL_BASEDIR") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let source = Instance::new(
        dir.path().join("source"),
        Binaries::from_basedir(&basedir),
        ServerSettings {
            port: free_port(),
            buffer_pool_mib: 128,
            ..Default::default()
        },
    );
    source.initialize().await.unwrap();
    let restored_root = dir.path().join("restored");
    std::fs::create_dir_all(restored_root.join("data")).unwrap();
    copy_snapshot(&source.datadir(), &restored_root.join("data"));
    // A crash after the credentials were saved, before the server started.
    std::fs::write(restored_root.join("init.pending"), b"").unwrap();
    std::fs::write(
        restored_root.join("client.cnf"),
        b"[client]\nuser=root\npassword=SavedPassword123\nhost=127.0.0.1\n",
    )
    .unwrap();
    let restored = Instance::new(
        &restored_root,
        Binaries::from_basedir(&basedir),
        ServerSettings {
            port: free_port(),
            buffer_pool_mib: 128,
            ..Default::default()
        },
    );
    restored.adopt().await.unwrap();
    assert!(restored.ping().await);
    let cnf = std::fs::read_to_string(restored_root.join("client.cnf")).unwrap();
    assert!(
        cnf.contains("password=SavedPassword123"),
        "saved password reused"
    );
    restored.stop().await.unwrap();
}
