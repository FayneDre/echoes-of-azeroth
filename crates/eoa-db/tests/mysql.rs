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
