use std::fs::File;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

use corqty_worker::check_alive;
use tempfile::TempDir;

/// Makes the file at `path` look `age` old.
fn make_old(path: &Path, age: Duration) {
    let file = File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - age).unwrap();
}

#[test]
fn the_alive_file_counts_for_10_seconds() {
    let directory = TempDir::new().unwrap();
    let alive_file = directory.path().join("corqty-worker.alive");
    assert!(check_alive(&alive_file).is_err());

    File::create(&alive_file).unwrap();
    assert_eq!(check_alive(&alive_file), Ok(()));
    make_old(&alive_file, Duration::from_secs(9));
    assert_eq!(check_alive(&alive_file), Ok(()));
    make_old(&alive_file, Duration::from_secs(11));
    let stale = format!("{} is 11 s old", alive_file.display());
    assert_eq!(check_alive(&alive_file), Err(stale));
}

#[test]
fn the_healthcheck_subcommand_checks_the_alive_file_in_the_temporary_directory() {
    let directory = TempDir::new().unwrap();
    let healthcheck = || -> Output {
        Command::new(env!("CARGO_BIN_EXE_corqty-worker"))
            .arg("healthcheck")
            // The healthcheck needs no database. std::env::temp_dir() reads TMPDIR on unix
            // and TMP on Windows.
            .env_remove("DATABASE_URL_WORKER")
            .env("TMPDIR", directory.path())
            .env("TMP", directory.path())
            .output()
            .unwrap()
    };
    let missing = healthcheck();
    assert!(!missing.status.success());

    File::create(directory.path().join("corqty-worker.alive")).unwrap();
    let fresh = healthcheck();
    let stderr = String::from_utf8_lossy(&fresh.stderr);
    assert!(fresh.status.success(), "{stderr}");
}
