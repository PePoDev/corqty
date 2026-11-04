use std::process::{Child, Command, Stdio};
use std::time::Duration;

use corqty_test_support::{TestDatabase, WORKER_URL};
use serial_test::serial;
use tempfile::TempDir;
use tokio::time::{sleep, timeout};

/// A worker process, killed when dropped.
struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts the worker binary with `CORQTY_SHELL_EXECUTOR` set to `switch`, or unset, and
/// returns the executors it registers.
async fn registered_executors(switch: Option<&str>) -> Vec<String> {
    let database = TestDatabase::reset().await;
    // The worker's alive file goes to its temporary directory. std::env::temp_dir() reads
    // TMPDIR on unix and TMP on Windows.
    let directory = TempDir::new().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_corqty-worker"));
    command
        .env("DATABASE_URL_WORKER", WORKER_URL)
        .env_remove("CORQTY_SHELL_EXECUTOR")
        .env("TMPDIR", directory.path())
        .env("TMP", directory.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(switch) = switch {
        command.env("CORQTY_SHELL_EXECUTOR", switch);
    }
    let _worker = Process(command.spawn().unwrap());
    let registered = async {
        loop {
            let executors: Option<Vec<String>> =
                sqlx::query_scalar("SELECT executors FROM worker_instances")
                    .fetch_optional(&database.owner)
                    .await
                    .unwrap();
            if let Some(executors) = executors {
                return executors;
            }
            sleep(Duration::from_millis(50)).await;
        }
    };
    timeout(Duration::from_secs(10), registered)
        .await
        .expect("the worker registers within 10 s")
}

#[tokio::test]
#[serial]
async fn only_the_operator_switch_gives_workers_the_shell_executor() {
    assert_eq!(registered_executors(None).await, ["mock@1.0.0"]);
    assert_eq!(registered_executors(Some("true")).await, ["mock@1.0.0"]);
    assert_eq!(
        registered_executors(Some("enabled")).await,
        ["mock@1.0.0", "shell@1.0.0"]
    );
}
