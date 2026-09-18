use crate::supervisor::{SupervisorError, WorkerLaunch, WorkerSupervisor};
use analyzer_domain::JobId;
use analyzer_protocol::{Method, PROTOCOL_VERSION};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

const MAX_AUTOMATIC_RESTARTS: u8 = 3;
// Demucs runs multiple overlapping chunks and can legitimately take many minutes on CPU.
// Keep the default timeout short for health checks and lightweight analyses.
const SEPARATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);

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
    restart_failures: Mutex<u8>,
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
            restart_failures: Mutex::new(0),
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
        *self.restart_failures.lock().await = 0;
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

    pub async fn separate(
        &self,
        job_id: JobId,
        params: Value,
    ) -> Result<(Value, Vec<analyzer_protocol::Event>), SupervisorError> {
        self.request_job_with_timeout(
            Method::Separate,
            job_id,
            params,
            "separation",
            SEPARATION_REQUEST_TIMEOUT,
        )
        .await
    }

    pub async fn separate_with_progress<F>(
        &self,
        job_id: JobId,
        params: Value,
        on_event: F,
    ) -> Result<(Value, Vec<analyzer_protocol::Event>), SupervisorError>
    where
        F: FnMut(&analyzer_protocol::Event),
    {
        self.request_job_with_progress(
            Method::Separate,
            job_id,
            params,
            "separation",
            SEPARATION_REQUEST_TIMEOUT,
            on_event,
        )
        .await
    }

    pub async fn analyze_rhythm(
        &self,
        job_id: JobId,
        params: Value,
    ) -> Result<(Value, Vec<analyzer_protocol::Event>), SupervisorError> {
        self.request_job(Method::AnalyzeRhythm, job_id, params, "rhythm")
            .await
    }

    pub async fn analyze_harmony(
        &self,
        job_id: JobId,
        params: Value,
    ) -> Result<(Value, Vec<analyzer_protocol::Event>), SupervisorError> {
        self.request_job(Method::AnalyzeHarmony, job_id, params, "harmony")
            .await
    }

    pub async fn analyze_pitch(
        &self,
        job_id: JobId,
        params: Value,
    ) -> Result<(Value, Vec<analyzer_protocol::Event>), SupervisorError> {
        self.request_job(Method::AnalyzePitch, job_id, params, "pitch")
            .await
    }

    async fn request_job(
        &self,
        method: Method,
        job_id: JobId,
        params: Value,
        stage: &'static str,
    ) -> Result<(Value, Vec<analyzer_protocol::Event>), SupervisorError> {
        self.request_job_with_timeout(method, job_id, params, stage, self.launch.request_timeout)
            .await
    }

    async fn request_job_with_timeout(
        &self,
        method: Method,
        job_id: JobId,
        params: Value,
        stage: &'static str,
        request_timeout: Duration,
    ) -> Result<(Value, Vec<analyzer_protocol::Event>), SupervisorError> {
        let mut supervisor = self.supervisor.lock().await;
        if supervisor.is_none() && *self.restart_failures.lock().await >= MAX_AUTOMATIC_RESTARTS {
            return Err(SupervisorError::RestartLimit);
        }
        if let Err(error) = ensure_healthy(&mut supervisor, &self.launch).await {
            *supervisor = None;
            *self.restart_failures.lock().await += 1;
            return Err(error);
        }
        let result = match supervisor.as_mut() {
            Some(active) => {
                active
                    .request_with_job_timeout(method.clone(), job_id, params, request_timeout)
                    .await
            }
            None => Err(SupervisorError::UnexpectedMessage(
                "healthy worker was unavailable",
            )),
        };
        match result {
            Ok((response, events)) => match response.result {
                Some(value) => {
                    *self.restart_failures.lock().await = 0;
                    Ok((value, events))
                }
                None => Err(SupervisorError::UnexpectedMessage(match stage {
                    "separation" => "separation response had no result",
                    "rhythm" => "rhythm response had no result",
                    "harmony" => "harmony response had no result",
                    _ => "pitch response had no result",
                })),
            },
            Err(error) => {
                // A malformed, timed-out, or mismatched message leaves stdout framing unknown.
                // Dropping kills the child (`kill_on_drop`), so the next request starts cleanly.
                warn!(%error, ?method, "discarding desynchronized analysis worker");
                *supervisor = None;
                *self.restart_failures.lock().await += 1;
                Err(error)
            }
        }
    }

    async fn request_job_with_progress<F>(
        &self,
        method: Method,
        job_id: JobId,
        params: Value,
        stage: &'static str,
        request_timeout: Duration,
        on_event: F,
    ) -> Result<(Value, Vec<analyzer_protocol::Event>), SupervisorError>
    where
        F: FnMut(&analyzer_protocol::Event),
    {
        let mut supervisor = self.supervisor.lock().await;
        if supervisor.is_none() && *self.restart_failures.lock().await >= MAX_AUTOMATIC_RESTARTS {
            return Err(SupervisorError::RestartLimit);
        }
        if let Err(error) = ensure_healthy(&mut supervisor, &self.launch).await {
            *supervisor = None;
            *self.restart_failures.lock().await += 1;
            return Err(error);
        }
        let result = match supervisor.as_mut() {
            Some(active) => {
                active
                    .request_with_job_progress(
                        method.clone(),
                        job_id,
                        params,
                        request_timeout,
                        on_event,
                    )
                    .await
            }
            None => Err(SupervisorError::UnexpectedMessage(
                "healthy worker was unavailable",
            )),
        };
        let worker_desynchronized = result.is_err();
        let outcome = finish_job_request(result, stage, method, &self.restart_failures).await;
        if worker_desynchronized {
            // A malformed, timed-out, or mismatched message leaves stdout framing unknown.
            // Dropping kills the child (`kill_on_drop`), so the next request starts cleanly.
            *supervisor = None;
        }
        outcome
    }
}

async fn finish_job_request(
    result: Result<(analyzer_protocol::Response, Vec<analyzer_protocol::Event>), SupervisorError>,
    stage: &'static str,
    method: Method,
    restart_failures: &Mutex<u8>,
) -> Result<(Value, Vec<analyzer_protocol::Event>), SupervisorError> {
    match result {
        Ok((response, events)) => match response.result {
            Some(value) => {
                *restart_failures.lock().await = 0;
                Ok((value, events))
            }
            None => Err(SupervisorError::UnexpectedMessage(match stage {
                "separation" => "separation response had no result",
                "rhythm" => "rhythm response had no result",
                "harmony" => "harmony response had no result",
                _ => "pitch response had no result",
            })),
        },
        Err(error) => {
            // A malformed, timed-out, or mismatched message leaves stdout framing unknown.
            // Dropping kills the child (`kill_on_drop`), so the next request starts cleanly.
            warn!(%error, ?method, "discarding desynchronized analysis worker");
            *restart_failures.lock().await += 1;
            Err(error)
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
    if let Some(program) = packaged_sidecar("analysis-worker") {
        return WorkerLaunch::new(program, Vec::new(), Duration::from_secs(45));
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

pub fn packaged_sidecar(name: &str) -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let candidate = executable.parent()?.join(format!("{name}.exe"));
    candidate.is_file().then_some(candidate)
}
