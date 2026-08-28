use local_music_analyzer_desktop::supervisor::{SupervisorError, WorkerLaunch, WorkerSupervisor};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

fn python() -> OsString {
    std::env::var_os("PYTHON").unwrap_or_else(|| OsString::from("python"))
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_worker.py")
}

fn launch(mode: &str) -> WorkerLaunch {
    let mut launch = WorkerLaunch::new(
        python(),
        vec![fixture().into_os_string(), mode.into()],
        Duration::from_millis(300),
    );
    launch.request_timeout = Duration::from_millis(300);
    launch.shutdown_timeout = Duration::from_millis(300);
    launch
}

#[tokio::test]
async fn accepts_valid_handshake_and_shutdowns_gracefully() {
    let mut worker = WorkerSupervisor::start(launch("valid"))
        .await
        .expect("valid worker should start");
    worker.ping().await.expect("worker should pong");
    worker.shutdown().await.expect("worker should exit cleanly");
}

#[tokio::test]
async fn detects_exit_before_handshake() {
    let error = startup_error("exit_before_hello").await;
    assert!(matches!(error, SupervisorError::MissingHandshake));
}

#[tokio::test]
async fn detects_invalid_handshake_version() {
    let error = startup_error("invalid_version").await;
    assert!(matches!(error, SupervisorError::Protocol(_)));
}

#[tokio::test]
async fn detects_invalid_stdout_json() {
    let error = startup_error("invalid_json").await;
    assert!(matches!(error, SupervisorError::MalformedJson(_)));
}

#[tokio::test]
async fn times_out_when_worker_hangs_during_startup() {
    let error = startup_error("hang").await;
    assert!(matches!(
        error,
        SupervisorError::Timeout { phase: "startup" }
    ));
}

async fn startup_error(mode: &str) -> SupervisorError {
    match WorkerSupervisor::start(launch(mode)).await {
        Ok(_) => panic!("{mode} worker unexpectedly became healthy"),
        Err(error) => error,
    }
}

#[tokio::test]
async fn detects_worker_death_after_healthy_handshake() {
    let mut worker = WorkerSupervisor::start(launch("die_after_healthy"))
        .await
        .expect("worker sends a valid hello");
    let error = worker
        .ping()
        .await
        .expect_err("worker must not answer ping");
    assert!(matches!(error, SupervisorError::WorkerExited { .. }));
}
