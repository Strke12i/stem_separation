//! Rust-owned persistence and validation for sidecar rhythm analysis.

use crate::doctor::WorkerManager;
use crate::ingest::IngestService;
use analyzer_domain::{
    Artifact, ArtifactKind, CreatedBy, JobId, JobStatus, StageRecord, TrackManifest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::Mutex;

const ENGINE: &str = "librosa.beat";
const ENGINE_VERSION: &str = "0.11";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RhythmReport {
    pub bpm: f64,
    #[serde(
        rename(serialize = "beatTimes", deserialize = "beat_times"),
        alias = "beatTimes"
    )]
    pub beat_times: Vec<f64>,
    pub algorithm: String,
    #[serde(
        rename(serialize = "stabilityScore", deserialize = "stability_score"),
        alias = "stabilityScore"
    )]
    pub stability_score: f64,
    #[serde(default, rename = "cacheHit")]
    pub cache_hit: bool,
}

pub struct RhythmService {
    ingest: Arc<IngestService>,
    workers: Arc<WorkerManager>,
    execution: Mutex<()>,
}

#[derive(Debug, Error)]
pub enum RhythmError {
    #[error("The requested imported track is unavailable.")]
    UnknownTrack,
    #[error("Could not communicate with the analysis worker: {0}")]
    Worker(String),
    #[error("The rhythm result returned by the analysis worker is invalid.")]
    InvalidResult,
    #[error("Could not read or write rhythm artifacts: {0}")]
    Storage(#[from] std::io::Error),
    #[error("The track manifest is invalid: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("Could not obtain a timestamp: {0}")]
    Clock(#[from] time::error::Format),
}

impl RhythmService {
    #[must_use]
    pub fn new(ingest: Arc<IngestService>, workers: Arc<WorkerManager>) -> Self {
        Self {
            ingest,
            workers,
            execution: Mutex::new(()),
        }
    }

    pub async fn analyze(&self, track_id: String) -> Result<RhythmReport, RhythmError> {
        let _guard = self.execution.lock().await;
        let workspace = self
            .ingest
            .workspace_for(&track_id)
            .map_err(|_| RhythmError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let signature = cache_key(&manifest.source.sha256);
        let artifact_path = workspace
            .join("analysis")
            .join("rhythm")
            .join(&signature)
            .join("rhythm.json");
        if artifact_path.is_file() {
            let mut cached: RhythmReport = serde_json::from_slice(&fs::read(&artifact_path)?)?;
            validate(&cached, manifest.source.duration_seconds)?;
            cached.cache_hit = true;
            return Ok(cached);
        }

        let normalized = self
            .ingest
            .normalized_source_for(&track_id)
            .map_err(|_| RhythmError::UnknownTrack)?;
        let job_id = JobId::new();
        let params = json!({"workspace_path": workspace, "input_path": normalized});
        let (result, _) = self
            .workers
            .analyze_rhythm(job_id, params)
            .await
            .map_err(|error| RhythmError::Worker(error.to_string()))?;
        let mut report: RhythmReport =
            serde_json::from_value(result).map_err(|_| RhythmError::InvalidResult)?;
        report.cache_hit = false;
        validate(&report, manifest.source.duration_seconds)?;
        write_json(&artifact_path, &report)?;
        persist(&workspace, manifest, &signature, &report)?;
        Ok(report)
    }

    pub fn cached(&self, track_id: &str) -> Result<Option<RhythmReport>, RhythmError> {
        let workspace = self
            .ingest
            .workspace_for(track_id)
            .map_err(|_| RhythmError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let path = workspace
            .join("analysis")
            .join("rhythm")
            .join(cache_key(&manifest.source.sha256))
            .join("rhythm.json");
        if !path.is_file() {
            return Ok(None);
        }
        let mut report: RhythmReport = serde_json::from_slice(&fs::read(path)?)?;
        validate(&report, manifest.source.duration_seconds)?;
        report.cache_hit = true;
        Ok(Some(report))
    }
}

fn cache_key(source_hash: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{source_hash}|rhythm|{ENGINE}|{ENGINE_VERSION}"))
    )
    .chars()
    .take(24)
    .collect()
}

fn validate(report: &RhythmReport, duration: f64) -> Result<(), RhythmError> {
    if !report.bpm.is_finite()
        || !(20.0..=400.0).contains(&report.bpm)
        || !report.stability_score.is_finite()
        || !(0.0..=1.0).contains(&report.stability_score)
        || report.algorithm != ENGINE
        || !duration.is_finite()
        || duration < 0.0
    {
        return Err(RhythmError::InvalidResult);
    }
    if report
        .beat_times
        .iter()
        .any(|beat| !beat.is_finite() || *beat < 0.0 || *beat > duration)
        || report.beat_times.windows(2).any(|pair| pair[1] <= pair[0])
    {
        return Err(RhythmError::InvalidResult);
    }
    Ok(())
}

fn read_manifest(workspace: &Path) -> Result<TrackManifest, RhythmError> {
    Ok(serde_json::from_slice(&fs::read(
        workspace.join("manifest.json"),
    )?)?)
}

fn persist(
    workspace: &Path,
    mut manifest: TrackManifest,
    signature: &str,
    report: &RhythmReport,
) -> Result<(), RhythmError> {
    let relative_path = format!("analysis/rhythm/{signature}/rhythm.json");
    manifest
        .analysis
        .insert("rhythm".to_owned(), serde_json::to_value(report)?);
    manifest.artifacts.retain(|artifact| {
        !(artifact.kind == ArtifactKind::Analysis && artifact.created_by.stage == "rhythm")
    });
    manifest.artifacts.push(Artifact {
        artifact_id: "artifact-rhythm".to_owned(),
        kind: ArtifactKind::Analysis,
        relative_path: relative_path.clone(),
        sha256: file_hash(&workspace.join(&relative_path))?,
        stem: None,
        created_by: CreatedBy {
            stage: "rhythm".to_owned(),
            engine: ENGINE.to_owned(),
            engine_version: ENGINE_VERSION.to_owned(),
            model: None,
        },
    });
    let now = OffsetDateTime::now_utc().format(&Rfc3339)?;
    manifest.stages.retain(|stage| stage.name != "rhythm");
    manifest.stages.push(StageRecord {
        name: "rhythm".to_owned(),
        status: JobStatus::Completed,
        cache_hit: false,
        started_at: now.clone(),
        finished_at: now,
        warnings: Vec::new(),
    });
    write_json(&workspace.join("manifest.json"), &manifest)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), RhythmError> {
    let parent = path.parent().ok_or(RhythmError::InvalidResult)?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    // A prior crash between create and rename can leave this file behind;
    // remove it so this write is not permanently blocked by AlreadyExists.
    let _ = fs::remove_file(&temporary);
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    let mut writer = BufWriter::new(file);
    writer.write_all(&serde_json::to_vec_pretty(value)?)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer
        .into_inner()
        .map_err(|error| error.into_error())?
        .sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn file_hash(path: &Path) -> Result<String, RhythmError> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}
