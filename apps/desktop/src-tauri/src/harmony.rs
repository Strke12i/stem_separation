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

const ENGINE: &str = "librosa.chroma_cqt+templates";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyReport {
    pub tonic: String,
    pub mode: String,
    pub label: String,
    pub score: f64,
    #[serde(
        rename(serialize = "secondBest", deserialize = "second_best"),
        alias = "secondBest"
    )]
    pub second_best: String,
    pub margin: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordSegment {
    pub start: f64,
    pub end: f64,
    pub label: String,
    pub root: Option<String>,
    pub quality: Option<String>,
    pub score: f64,
    #[serde(
        rename(serialize = "beatAligned", deserialize = "beat_aligned"),
        alias = "beatAligned"
    )]
    pub beat_aligned: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyReport {
    pub key: KeyReport,
    pub chords: Vec<ChordSegment>,
    pub algorithm: String,
    #[serde(default)]
    pub cache_hit: bool,
}
pub struct HarmonyService {
    ingest: Arc<IngestService>,
    workers: Arc<WorkerManager>,
    execution: Mutex<()>,
}
#[derive(Debug, Error)]
pub enum HarmonyError {
    #[error("The requested imported track is unavailable.")]
    UnknownTrack,
    #[error("Could not communicate with the analysis worker: {0}")]
    Worker(String),
    #[error("The harmony result returned by the analysis worker is invalid.")]
    InvalidResult,
    #[error("Could not read or write harmony artifacts: {0}")]
    Storage(#[from] std::io::Error),
    #[error("The track manifest is invalid: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("Could not obtain a timestamp: {0}")]
    Clock(#[from] time::error::Format),
}
impl HarmonyService {
    #[must_use]
    pub fn new(ingest: Arc<IngestService>, workers: Arc<WorkerManager>) -> Self {
        Self {
            ingest,
            workers,
            execution: Mutex::new(()),
        }
    }
    pub async fn analyze(&self, track_id: String) -> Result<HarmonyReport, HarmonyError> {
        let _guard = self.execution.lock().await;
        let workspace = self
            .ingest
            .workspace_for(&track_id)
            .map_err(|_| HarmonyError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let beats = beats(&manifest);
        let signature = key(&manifest.source.sha256, &beats);
        let path = workspace
            .join("analysis/harmony")
            .join(&signature)
            .join("harmony.json");
        if path.is_file() {
            let mut cached: HarmonyReport = serde_json::from_slice(&fs::read(path)?)?;
            validate(&cached, manifest.source.duration_seconds)?;
            cached.cache_hit = true;
            return Ok(cached);
        }
        let input = self
            .ingest
            .normalized_source_for(&track_id)
            .map_err(|_| HarmonyError::UnknownTrack)?;
        let (value, _) = self
            .workers
            .analyze_harmony(
                JobId::new(),
                json!({"workspace_path":workspace,"input_path":input,"beat_times":beats}),
            )
            .await
            .map_err(|error| HarmonyError::Worker(error.to_string()))?;
        let mut report: HarmonyReport =
            serde_json::from_value(value).map_err(|_| HarmonyError::InvalidResult)?;
        report.cache_hit = false;
        validate(&report, manifest.source.duration_seconds)?;
        write_json(&path, &report)?;
        persist(&workspace, manifest, &signature, &report)?;
        Ok(report)
    }
    pub fn cached(&self, track_id: &str) -> Result<Option<HarmonyReport>, HarmonyError> {
        let workspace = self
            .ingest
            .workspace_for(track_id)
            .map_err(|_| HarmonyError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let path = workspace
            .join("analysis/harmony")
            .join(key(&manifest.source.sha256, &beats(&manifest)))
            .join("harmony.json");
        if !path.is_file() {
            return Ok(None);
        }
        let mut report: HarmonyReport = serde_json::from_slice(&fs::read(path)?)?;
        validate(&report, manifest.source.duration_seconds)?;
        report.cache_hit = true;
        Ok(Some(report))
    }
}
fn beats(manifest: &TrackManifest) -> Vec<f64> {
    manifest
        .analysis
        .get("rhythm")
        .and_then(|v| v.get("beatTimes").or_else(|| v.get("beat_times")))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}
fn key(source: &str, beats: &[f64]) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{source}|harmony|{ENGINE}|{beats:?}"))
    )
    .chars()
    .take(24)
    .collect()
}
fn validate(report: &HarmonyReport, duration: f64) -> Result<(), HarmonyError> {
    if report.algorithm != ENGINE
        || !report.key.score.is_finite()
        || !report.key.margin.is_finite()
        || report.key.tonic.is_empty()
        || !matches!(report.key.mode.as_str(), "major" | "minor")
        || report.chords.iter().any(|c| {
            !c.start.is_finite()
                || !c.end.is_finite()
                || c.start < 0.0
                || c.end <= c.start
                || c.end > duration + 0.2
                || !c.score.is_finite()
                || c.label.is_empty()
        })
    {
        return Err(HarmonyError::InvalidResult);
    }
    Ok(())
}
fn read_manifest(workspace: &Path) -> Result<TrackManifest, HarmonyError> {
    Ok(serde_json::from_slice(&fs::read(
        workspace.join("manifest.json"),
    )?)?)
}
fn persist(
    workspace: &Path,
    mut manifest: TrackManifest,
    signature: &str,
    report: &HarmonyReport,
) -> Result<(), HarmonyError> {
    let relative = format!("analysis/harmony/{signature}/harmony.json");
    manifest
        .analysis
        .insert("harmony".to_owned(), serde_json::to_value(report)?);
    manifest
        .artifacts
        .retain(|a| !(a.kind == ArtifactKind::Analysis && a.created_by.stage == "harmony"));
    manifest.artifacts.push(Artifact {
        artifact_id: "artifact-harmony".to_owned(),
        kind: ArtifactKind::Analysis,
        relative_path: relative.clone(),
        sha256: format!("{:x}", Sha256::digest(fs::read(workspace.join(&relative))?)),
        stem: None,
        created_by: CreatedBy {
            stage: "harmony".to_owned(),
            engine: ENGINE.to_owned(),
            engine_version: "0.1".to_owned(),
            model: None,
        },
    });
    let now = OffsetDateTime::now_utc().format(&Rfc3339)?;
    manifest.stages.retain(|s| s.name != "harmony");
    manifest.stages.push(StageRecord {
        name: "harmony".to_owned(),
        status: JobStatus::Completed,
        cache_hit: false,
        started_at: now.clone(),
        finished_at: now,
        warnings: Vec::new(),
    });
    write_json(&workspace.join("manifest.json"), &manifest)
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), HarmonyError> {
    let parent = path.parent().ok_or(HarmonyError::InvalidResult)?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    let mut out = BufWriter::new(file);
    out.write_all(&serde_json::to_vec_pretty(value)?)?;
    out.write_all(b"\n")?;
    out.flush()?;
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}
