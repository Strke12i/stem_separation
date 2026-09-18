//! Rust-owned validation and persistence for monophonic pYIN note analysis.

use crate::doctor::WorkerManager;
use crate::ingest::IngestService;
use analyzer_domain::{
    Artifact, ArtifactKind, CreatedBy, JobId, JobStatus, StageRecord, TrackManifest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::Mutex;

const ENGINE: &str = "pyin";
const ENGINE_VERSION: &str = "0.11";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PitchNote {
    pub start: f64,
    pub end: f64,
    pub midi: i16,
    pub note: String,
    pub confidence: f64,
    pub stem: String,
    pub engine: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PitchReport {
    pub stem: String,
    pub engine: String,
    pub notes: Vec<PitchNote>,
    #[serde(default)]
    pub cache_hit: bool,
}

pub struct PitchService {
    ingest: Arc<IngestService>,
    workers: Arc<WorkerManager>,
    execution: Mutex<()>,
}

#[derive(Debug, Error)]
pub enum PitchError {
    #[error("The requested imported track is unavailable.")]
    UnknownTrack,
    #[error("Pitch analysis currently supports only the bass and vocals stems.")]
    UnsupportedStem,
    #[error("The requested {0} stem is unavailable. Generate local stems before analyzing pitch.")]
    MissingStem(String),
    #[error("Could not communicate with the analysis worker: {0}")]
    Worker(String),
    #[error("The pitch result returned by the analysis worker is invalid.")]
    InvalidResult,
    #[error("Could not read or write pitch artifacts: {0}")]
    Storage(#[from] std::io::Error),
    #[error("The track manifest is invalid: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("Could not obtain a timestamp: {0}")]
    Clock(#[from] time::error::Format),
}

impl PitchService {
    #[must_use]
    pub fn new(ingest: Arc<IngestService>, workers: Arc<WorkerManager>) -> Self {
        Self {
            ingest,
            workers,
            execution: Mutex::new(()),
        }
    }

    pub async fn analyze(&self, track_id: String, stem: String) -> Result<PitchReport, PitchError> {
        let _guard = self.execution.lock().await;
        validate_stem(&stem)?;
        let workspace = self
            .ingest
            .workspace_for(&track_id)
            .map_err(|_| PitchError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let (input, stem_hash) = stem_path(&workspace, &manifest, &stem)?;
        let signature = cache_key(&manifest.source.sha256, &stem, &stem_hash);
        let path = workspace
            .join("analysis/pitch")
            .join(&stem)
            .join(&signature)
            .join("notes.json");
        if path.is_file() {
            let mut cached: PitchReport = serde_json::from_slice(&fs::read(path)?)?;
            validate(&cached, manifest.source.duration_seconds, &stem)?;
            cached.cache_hit = true;
            return Ok(cached);
        }

        let (value, _) = self
            .workers
            .analyze_pitch(
                JobId::new(),
                json!({"workspace_path": workspace, "input_path": input, "stem": stem}),
            )
            .await
            .map_err(|error| PitchError::Worker(error.to_string()))?;
        let mut report: PitchReport =
            serde_json::from_value(value).map_err(|_| PitchError::InvalidResult)?;
        report.cache_hit = false;
        validate(&report, manifest.source.duration_seconds, &stem)?;
        write_json(&path, &report)?;
        persist(
            &self.ingest,
            &track_id,
            &workspace,
            &stem,
            &signature,
            &report,
        )
        .await?;
        Ok(report)
    }

    pub fn cached(&self, track_id: &str, stem: &str) -> Result<Option<PitchReport>, PitchError> {
        validate_stem(stem)?;
        let workspace = self
            .ingest
            .workspace_for(track_id)
            .map_err(|_| PitchError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let (_, stem_hash) = stem_path(&workspace, &manifest, stem)?;
        let path = workspace
            .join("analysis/pitch")
            .join(stem)
            .join(cache_key(&manifest.source.sha256, stem, &stem_hash))
            .join("notes.json");
        if !path.is_file() {
            return Ok(None);
        }
        let mut report: PitchReport = serde_json::from_slice(&fs::read(path)?)?;
        validate(&report, manifest.source.duration_seconds, stem)?;
        report.cache_hit = true;
        Ok(Some(report))
    }
}

fn validate_stem(stem: &str) -> Result<(), PitchError> {
    if matches!(stem, "bass" | "vocals") {
        Ok(())
    } else {
        Err(PitchError::UnsupportedStem)
    }
}

fn stem_path(
    workspace: &Path,
    manifest: &TrackManifest,
    stem: &str,
) -> Result<(PathBuf, String), PitchError> {
    let artifact = manifest
        .artifacts
        .iter()
        .rev()
        .find(|artifact| {
            artifact.kind == ArtifactKind::Stem
                && artifact
                    .stem
                    .as_ref()
                    .is_some_and(|kind| kind.name() == stem)
        })
        .ok_or_else(|| PitchError::MissingStem(stem.to_owned()))?;
    let relative = Path::new(&artifact.relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PitchError::MissingStem(stem.to_owned()));
    }
    let input = workspace.join(relative).canonicalize()?;
    if !input.starts_with(workspace) || !is_wav(&input)? {
        return Err(PitchError::MissingStem(stem.to_owned()));
    }
    Ok((input, artifact.sha256.clone()))
}

fn cache_key(source_hash: &str, stem: &str, stem_hash: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!(
            "{source_hash}|pitch|{stem}|{stem_hash}|{ENGINE}|{ENGINE_VERSION}"
        ))
    )
    .chars()
    .take(24)
    .collect()
}

fn validate(report: &PitchReport, duration: f64, stem: &str) -> Result<(), PitchError> {
    if report.stem != stem || report.engine != ENGINE || !duration.is_finite() || duration < 0.0 {
        return Err(PitchError::InvalidResult);
    }
    if report.notes.iter().any(|note| {
        !note.start.is_finite()
            || !note.end.is_finite()
            || note.start < 0.0
            || note.end <= note.start
            || note.end > duration + 0.2
            || !(0..=127).contains(&note.midi)
            || note.note.is_empty()
            || !note.confidence.is_finite()
            || !(0.0..=1.0).contains(&note.confidence)
            || note.stem != stem
            || note.engine != ENGINE
    }) || report
        .notes
        .windows(2)
        .any(|pair| pair[1].start < pair[0].end)
    {
        return Err(PitchError::InvalidResult);
    }
    Ok(())
}

fn read_manifest(workspace: &Path) -> Result<TrackManifest, PitchError> {
    Ok(serde_json::from_slice(&fs::read(
        workspace.join("manifest.json"),
    )?)?)
}

async fn persist(
    ingest: &IngestService,
    track_id: &str,
    workspace: &Path,
    stem: &str,
    signature: &str,
    report: &PitchReport,
) -> Result<(), PitchError> {
    // Re-read fresh under a per-track lock rather than reusing the manifest
    // captured at job start: another analysis service may have finished and
    // persisted its own artifacts while this job's worker was running, and
    // writing back a stale in-memory copy would silently erase that work.
    let lock = ingest.track_lock(track_id);
    let _guard = lock.lock().await;
    let mut manifest = read_manifest(workspace)?;
    let relative = format!("analysis/pitch/{stem}/{signature}/notes.json");
    let pitch = manifest
        .analysis
        .entry("pitch".to_owned())
        .or_insert_with(|| json!({}));
    let entries = pitch.as_object_mut().ok_or(PitchError::InvalidResult)?;
    entries.insert(stem.to_owned(), serde_json::to_value(report)?);

    let stage = format!("pitch:{stem}");
    manifest.artifacts.retain(|artifact| {
        !(artifact.kind == ArtifactKind::Analysis && artifact.created_by.stage == stage)
    });
    manifest.artifacts.push(Artifact {
        artifact_id: format!("artifact-pitch-{stem}"),
        kind: ArtifactKind::Analysis,
        relative_path: relative.clone(),
        sha256: format!("{:x}", Sha256::digest(fs::read(workspace.join(&relative))?)),
        stem: None,
        created_by: CreatedBy {
            stage: stage.clone(),
            engine: ENGINE.to_owned(),
            engine_version: ENGINE_VERSION.to_owned(),
            model: None,
        },
    });
    let now = OffsetDateTime::now_utc().format(&Rfc3339)?;
    manifest.stages.retain(|record| record.name != stage);
    manifest.stages.push(StageRecord {
        name: stage,
        status: JobStatus::Completed,
        cache_hit: false,
        started_at: now.clone(),
        finished_at: now,
        warnings: Vec::new(),
    });
    write_json(&workspace.join("manifest.json"), &manifest)
}

fn is_wav(path: &Path) -> Result<bool, PitchError> {
    let mut header = [0_u8; 12];
    let mut file = fs::File::open(path)?;
    if file.read_exact(&mut header).is_err() {
        return Ok(false);
    }
    Ok(&header[..4] == b"RIFF" && &header[8..12] == b"WAVE")
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), PitchError> {
    let parent = path.parent().ok_or(PitchError::InvalidResult)?;
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
