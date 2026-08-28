use crate::supervisor::{SupervisorError, WorkerLaunch, WorkerSupervisor};
use analyzer_protocol::PROTOCOL_VERSION;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub desktop_core: ComponentStatus,
    pub analysis_worker: ComponentStatus,
    pub protocol_version: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentStatus {
    pub ok: bool,
    pub detail: String,
}

pub struct WorkerManager {
    launch: WorkerLaunch,
    supervisor: Mutex<Option<WorkerSupervisor>>,
}

impl WorkerManager {
    #[must_use]
    pub fn development() -> Self {
        Self::new(development_worker_launch())
    }

    #[must_use]
    pub fn new(launch: WorkerLaunch) -> Self {
        Self {
            launch,
            supervisor: Mutex::new(None),
        }
    }

    pub async fn doctor(&self) -> DoctorReport {
        let mut supervisor = self.supervisor.lock().await;
        let result = ensure_healthy(&mut supervisor, &self.launch).await;

        match result {
            Ok(()) => DoctorReport {
                desktop_core: ok("Rust host is running"),
                analysis_worker: ok("healthy; hello and ping validated"),
                protocol_version: PROTOCOL_VERSION,
            },
            Err(error) => {
                warn!(error = %error, "analysis worker doctor check failed");
                *supervisor = None;
                DoctorReport {
                    desktop_core: ok("Rust host is running"),
                    analysis_worker: failed(error.to_string()),
                    protocol_version: PROTOCOL_VERSION,
                }
            }
        }
    }

    pub async fn restart(&self) -> DoctorReport {
        let mut supervisor = self.supervisor.lock().await;
        if let Some(mut active) = supervisor.take() {
            if let Err(error) = active.shutdown().await {
                warn!(error = %error, "worker did not shut down cleanly before restart");
            }
        }

        info!("starting analysis worker after explicit restart");
        self.doctor_with_guard(&mut supervisor).await
    }

    async fn doctor_with_guard(&self, supervisor: &mut Option<WorkerSupervisor>) -> DoctorReport {
        match ensure_healthy(supervisor, &self.launch).await {
            Ok(()) => DoctorReport {
                desktop_core: ok("Rust host is running"),
                analysis_worker: ok("healthy; hello and ping validated"),
                protocol_version: PROTOCOL_VERSION,
            },
            Err(error) => {
                *supervisor = None;
                DoctorReport {
                    desktop_core: ok("Rust host is running"),
                    analysis_worker: failed(error.to_string()),
                    protocol_version: PROTOCOL_VERSION,
                }
            }
        }
    }

    pub async fn shutdown(&self) {
        let mut supervisor = self.supervisor.lock().await;
        if let Some(mut active) = supervisor.take() {
            if let Err(error) = active.shutdown().await {
                warn!(error = %error, "worker shutdown during app exit failed");
            }
        }
    }
}

async fn ensure_healthy(
    supervisor: &mut Option<WorkerSupervisor>,
    launch: &WorkerLaunch,
) -> Result<(), SupervisorError> {
    if supervisor.is_none() {
        *supervisor = Some(WorkerSupervisor::start(launch.clone()).await?);
    }

    if let Some(active) = supervisor.as_mut() {
        active.ping().await?;
    }
    Ok(())
}

fn ok(detail: impl Into<String>) -> ComponentStatus {
    ComponentStatus {
        ok: true,
        detail: detail.into(),
    }
}

fn failed(detail: impl Into<String>) -> ComponentStatus {
    ComponentStatus {
        ok: false,
        detail: detail.into(),
    }
}

fn development_worker_launch() -> WorkerLaunch {
    if let Some(program) = std::env::var_os("LOCAL_MUSIC_ANALYZER_WORKER") {
        return WorkerLaunch::new(program, Vec::new(), Duration::from_secs(8));
    }

    let worker_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../python/analysis-worker");
    WorkerLaunch::new(
        "uv",
        vec![
            "run".into(),
            "--directory".into(),
            worker_root.to_string_lossy().into_owned().into(),
            "python".into(),
            "-m".into(),
            "music_analyzer_worker".into(),
        ],
        Duration::from_secs(8),
    )
}
