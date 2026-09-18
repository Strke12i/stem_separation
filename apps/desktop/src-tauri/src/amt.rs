//! Optional Basic Pitch worker orchestration and MIDI artifact promotion.

use crate::doctor::packaged_sidecar;
use crate::ingest::IngestService;
use crate::scheduler::ResourceScheduler;
use crate::supervisor::{SupervisorError, WorkerLaunch, WorkerSupervisor};
use analyzer_domain::{
    Artifact, ArtifactKind, CreatedBy, JobId, JobStatus, StageRecord, TrackManifest,
};
use analyzer_protocol::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::Mutex;

const ENGINE: &str = "basic-pitch";
const VERSION: &str = "0.4.0";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmtNote {
    pub start: f64,
    pub end: f64,
    pub midi: i16,
    pub velocity: u8,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmtReport {
    pub engine: String,
    pub model: String,
    pub notes: Vec<AmtNote>,
    #[serde(
        rename(serialize = "midiArtifact", deserialize = "relative_midi_path"),
        alias = "midiArtifact"
    )]
    pub midi_artifact: String,
    #[serde(default)]
    pub cache_hit: bool,
}
pub struct AmtService {
    ingest: Arc<IngestService>,
    worker: Mutex<Option<WorkerSupervisor>>,
    execution: Mutex<()>,
    scheduler: Arc<ResourceScheduler>,
}
#[derive(Debug, Error)]
pub enum AmtError {
    #[error("The requested imported track is unavailable.")]
    UnknownTrack,
    #[error(
        "The optional Basic Pitch worker is unavailable. Install python/amt-worker locally, then restart the app."
    )]
    WorkerUnavailable,
    #[error("The AMT worker failed: {0}")]
    Worker(String),
    #[error("The AMT worker returned an invalid transcription.")]
    InvalidResult,
    #[error("Could not read or write AMT artifacts: {0}")]
    Storage(#[from] std::io::Error),
    #[error("The track manifest is invalid: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("Could not obtain a timestamp: {0}")]
    Clock(#[from] time::error::Format),
}
impl AmtService {
    #[must_use]
    pub fn development(ingest: Arc<IngestService>, scheduler: Arc<ResourceScheduler>) -> Self {
        Self {
            ingest,
            worker: Mutex::new(None),
            execution: Mutex::new(()),
            scheduler,
        }
    }
    pub async fn transcribe(&self, track_id: String) -> Result<AmtReport, AmtError> {
        let _guard = self.execution.lock().await;
        let workspace = self
            .ingest
            .workspace_for(&track_id)
            .map_err(|_| AmtError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let key = cache_key(&manifest.source.sha256);
        let final_dir = workspace.join("analysis/amt").join(&key);
        let json_path = final_dir.join("transcription.json");
        if json_path.is_file() && final_dir.join("transcription.mid").is_file() {
            let mut report: AmtReport = serde_json::from_slice(&fs::read(json_path)?)?;
            validate(&report, manifest.source.duration_seconds)?;
            report.cache_hit = true;
            return Ok(report);
        }
        let job = JobId::new();
        let temp = workspace.join("tmp").join(job.as_str());
        let input = self
            .ingest
            .normalized_source_for(&track_id)
            .map_err(|_| AmtError::UnknownTrack)?;
        let _gpu_permit = self
            .scheduler
            .acquire_gpu()
            .await
            .map_err(|error| AmtError::Worker(error.to_string()))?;
        let result = self
            .request(
                job.clone(),
                json!({"workspace_path":workspace,"input_path":input,"output_dir":temp}),
            )
            .await;
        let value = result?;
        let mut report: AmtReport =
            serde_json::from_value(value).map_err(|_| AmtError::InvalidResult)?;
        report.midi_artifact = "transcription.mid".to_owned();
        report.cache_hit = false;
        validate(&report, manifest.source.duration_seconds)?;
        let midi = temp.join("transcription.mid");
        if !valid_midi(&midi)? {
            return Err(AmtError::InvalidResult);
        }
        fs::create_dir_all(final_dir.parent().ok_or(AmtError::InvalidResult)?)?;
        if final_dir.exists() {
            return Err(AmtError::InvalidResult);
        }
        write_json(&temp.join("transcription.json"), &report)?;
        fs::rename(&temp, &final_dir)?;
        persist(&workspace, manifest, &key, &report)?;
        Ok(report)
    }

    pub fn cached(&self, track_id: &str) -> Result<Option<AmtReport>, AmtError> {
        let workspace = self
            .ingest
            .workspace_for(track_id)
            .map_err(|_| AmtError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let key = cache_key(&manifest.source.sha256);
        let final_dir = workspace.join("analysis/amt").join(key);
        let json_path = final_dir.join("transcription.json");
        let midi_path = final_dir.join("transcription.mid");
        if !json_path.is_file() || !valid_midi(&midi_path)? {
            return Ok(None);
        }
        let mut report: AmtReport = serde_json::from_slice(&fs::read(json_path)?)?;
        validate(&report, manifest.source.duration_seconds)?;
        report.cache_hit = true;
        Ok(Some(report))
    }

    pub fn midi_bytes(&self, track_id: &str) -> Result<Vec<u8>, AmtError> {
        let workspace = self
            .ingest
            .workspace_for(track_id)
            .map_err(|_| AmtError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let path = workspace
            .join("analysis/amt")
            .join(cache_key(&manifest.source.sha256))
            .join("transcription.mid");
        let bytes = fs::read(&path)?;
        if !bytes.starts_with(b"MThd") {
            return Err(AmtError::InvalidResult);
        }
        Ok(bytes)
    }
    async fn request(
        &self,
        job: JobId,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, AmtError> {
        let mut worker = self.worker.lock().await;
        if worker.is_none() {
            *worker = Some(
                WorkerSupervisor::start(amt_launch())
                    .await
                    .map_err(map_start)?,
            );
        }
        let active = worker.as_mut().ok_or(AmtError::WorkerUnavailable)?;
        active
            .ping()
            .await
            .map_err(|e| AmtError::Worker(e.to_string()))?;
        active
            .request_with_job(Method::Transcribe, job, params)
            .await
            .map_err(|e| AmtError::Worker(e.to_string()))?
            .0
            .result
            .ok_or(AmtError::InvalidResult)
    }
}
fn map_start(error: SupervisorError) -> AmtError {
    match error {
        SupervisorError::Spawn(_) | SupervisorError::MissingHandshake => {
            AmtError::WorkerUnavailable
        }
        other => AmtError::Worker(other.to_string()),
    }
}
fn amt_launch() -> WorkerLaunch {
    if let Some(program) = std::env::var_os("LOCAL_MUSIC_ANALYZER_AMT_WORKER") {
        return WorkerLaunch::new(program, Vec::new(), Duration::from_secs(15));
    }
    if let Some(program) = packaged_sidecar("amt-worker") {
        return WorkerLaunch::new(program, Vec::new(), Duration::from_secs(60));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../python/amt-worker");
    let mut launch = WorkerLaunch::new(
        "uv",
        vec![
            "run".into(),
            "--offline".into(),
            "--directory".into(),
            root.to_string_lossy().into_owned().into(),
            "python".into(),
            "-m".into(),
            "music_analyzer_amt".into(),
        ],
        Duration::from_secs(15),
    );
    launch.request_timeout = Duration::from_secs(600);
    launch
}
fn cache_key(source: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{source}|amt|{ENGINE}|{VERSION}|original"))
    )
    .chars()
    .take(24)
    .collect()
}
fn validate(report: &AmtReport, duration: f64) -> Result<(), AmtError> {
    if report.engine != ENGINE
        || report.model != "icassp_2022"
        || report.midi_artifact != "transcription.mid"
        || report.notes.iter().any(|n| {
            !n.start.is_finite()
                || !n.end.is_finite()
                || n.start < 0.0
                || n.end <= n.start
                || n.end > duration + 0.2
                || !(0..=127).contains(&n.midi)
                || n.velocity == 0
        })
    {
        Err(AmtError::InvalidResult)
    } else {
        Ok(())
    }
}
fn valid_midi(path: &Path) -> Result<bool, AmtError> {
    Ok(fs::read(path)?.starts_with(b"MThd"))
}
fn read_manifest(workspace: &Path) -> Result<TrackManifest, AmtError> {
    Ok(serde_json::from_slice(&fs::read(
        workspace.join("manifest.json"),
    )?)?)
}
fn persist(
    workspace: &Path,
    mut manifest: TrackManifest,
    key: &str,
    report: &AmtReport,
) -> Result<(), AmtError> {
    let relative = format!("analysis/amt/{key}/transcription.mid");
    manifest
        .analysis
        .insert("amt".to_owned(), serde_json::to_value(report)?);
    manifest
        .artifacts
        .retain(|a| !(a.kind == ArtifactKind::Analysis && a.created_by.stage == "amt"));
    manifest.artifacts.push(Artifact {
        artifact_id: "artifact-amt-midi".to_owned(),
        kind: ArtifactKind::Analysis,
        relative_path: relative.clone(),
        sha256: format!("{:x}", Sha256::digest(fs::read(workspace.join(&relative))?)),
        stem: None,
        created_by: CreatedBy {
            stage: "amt".to_owned(),
            engine: ENGINE.to_owned(),
            engine_version: VERSION.to_owned(),
            model: Some("icassp_2022".to_owned()),
        },
    });
    let now = OffsetDateTime::now_utc().format(&Rfc3339)?;
    manifest.stages.retain(|s| s.name != "amt");
    manifest.stages.push(StageRecord {
        name: "amt".to_owned(),
        status: JobStatus::Completed,
        cache_hit: false,
        started_at: now.clone(),
        finished_at: now,
        warnings: Vec::new(),
    });
    write_json(&workspace.join("manifest.json"), &manifest)
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), AmtError> {
    let parent = path.parent().ok_or(AmtError::InvalidResult)?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension("json.tmp");
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    let mut out = BufWriter::new(file);
    out.write_all(&serde_json::to_vec_pretty(value)?)?;
    out.write_all(b"\n")?;
    out.flush()?;
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    fs::rename(temp, path)?;
    Ok(())
}
