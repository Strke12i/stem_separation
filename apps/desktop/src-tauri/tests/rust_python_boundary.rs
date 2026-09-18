use local_music_analyzer_desktop::supervisor::{WorkerLaunch, WorkerSupervisor};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

fn worker_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../python/analysis-worker")
}

fn worker_python() -> OsString {
    let executable = if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    };
    worker_root()
        .join(".venv")
        .join(executable)
        .into_os_string()
}

fn source_worker_args() -> Vec<OsString> {
    let source = worker_root().join("src");
    let command = format!(
        "import sys;sys.path.insert(0,{source:?});from music_analyzer_worker.__main__ import main;main()"
    );
    vec!["-c".into(), command.into()]
}

#[tokio::test]
async fn rust_spawns_python_then_pings_and_shuts_it_down() {
    let mut launch = WorkerLaunch::new(
        worker_python(),
        source_worker_args(),
        Duration::from_secs(5),
    );
    launch.request_timeout = Duration::from_secs(5);
    launch.shutdown_timeout = Duration::from_secs(5);

    let mut worker = WorkerSupervisor::start(launch)
        .await
        .expect("Rust must accept the Python worker hello");
    assert_eq!(worker.info().name, "analysis");
    worker.ping().await.expect("Python worker must return pong");
    worker
        .shutdown()
        .await
        .expect("Python worker must exit cleanly");
}
