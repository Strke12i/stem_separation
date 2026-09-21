//! Optional Basic Pitch worker orchestration and MIDI artifact promotion.

use crate::doctor::packaged_sidecar;
use crate::ingest::IngestService;
use crate::scheduler::ResourceScheduler;
use crate::stems::{StemLookupError, is_known_stem, stem_audio};
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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::Mutex;

const ENGINE: &str = "basic-pitch";
const VERSION: &str = "0.4.0";
const MAX_AUTOMATIC_RESTARTS: u8 = 3;

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
    /// The stem this was transcribed from; `None` for the whole mix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stem: Option<String>,
    #[serde(default)]
    pub cache_hit: bool,
}
pub struct AmtService {
    ingest: Arc<IngestService>,
    worker: Mutex<Option<WorkerSupervisor>>,
    execution: Mutex<()>,
    scheduler: Arc<ResourceScheduler>,
    restart_failures: Mutex<u8>,
    launch: WorkerLaunch,
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
    #[error("Transcription is not available for that stem name.")]
    UnsupportedStem,
    #[error("The requested {0} stem is unavailable. Generate local stems first.")]
    MissingStem(String),
    #[error("Could not read or write AMT artifacts: {0}")]
    Storage(#[from] std::io::Error),
    #[error("The track manifest is invalid: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("Could not obtain a timestamp: {0}")]
    Clock(#[from] time::error::Format),
}
impl From<StemLookupError> for AmtError {
    fn from(error: StemLookupError) -> Self {
        match error {
            StemLookupError::Missing(stem) => Self::MissingStem(stem),
            StemLookupError::Io(error) => Self::Storage(error),
        }
    }
}
impl AmtService {
    #[must_use]
    pub fn development(ingest: Arc<IngestService>, scheduler: Arc<ResourceScheduler>) -> Self {
        Self::new(ingest, scheduler, amt_launch())
    }

    #[must_use]
    pub fn new(
        ingest: Arc<IngestService>,
        scheduler: Arc<ResourceScheduler>,
        launch: WorkerLaunch,
    ) -> Self {
        Self {
            ingest,
            worker: Mutex::new(None),
            execution: Mutex::new(()),
            scheduler,
            restart_failures: Mutex::new(0),
            launch,
        }
    }
    /// Polyphonic notes and a MIDI file for the whole mix (`stem: None`) or for
    /// one separated stem.
    pub async fn transcribe(
        &self,
        track_id: String,
        stem: Option<String>,
    ) -> Result<AmtReport, AmtError> {
        let _guard = self.execution.lock().await;
        let stem = stem.as_deref();
        check_stem(stem)?;
        let workspace = self
            .ingest
            .workspace_for(&track_id)
            .map_err(|_| AmtError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let (input, stem_hash) = match stem {
            Some(name) => {
                let (path, hash) = stem_audio(&workspace, &manifest, name)?;
                (path, Some(hash))
            }
            None => (
                self.ingest
                    .normalized_source_for(&track_id)
                    .map_err(|_| AmtError::UnknownTrack)?,
                None,
            ),
        };
        let key = cache_key(&manifest.source.sha256, stem.zip(stem_hash.as_deref()));
        let final_dir = amt_dir(&workspace, stem).join(&key);
        let json_path = final_dir.join("transcription.json");
        if json_path.is_file() && final_dir.join("transcription.mid").is_file() {
            let mut report: AmtReport = serde_json::from_slice(&fs::read(json_path)?)?;
            validate(&report, manifest.source.duration_seconds, stem)?;
            report.cache_hit = true;
            return Ok(report);
        }
        let job = JobId::new();
        let temp = workspace.join("tmp").join(job.as_str());
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
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                cleanup_temporary_job(&temp);
                return Err(error);
            }
        };
        // Every failure below happens after the worker already wrote into
        // `temp`; clean it up on every path, not only the happy one, so a
        // bad transcription never leaks a full-size MIDI/notes temp dir.
        let outcome = (|| -> Result<AmtReport, AmtError> {
            let mut report: AmtReport =
                serde_json::from_value(value).map_err(|_| AmtError::InvalidResult)?;
            report.midi_artifact = "transcription.mid".to_owned();
            report.cache_hit = false;
            report.stem = stem.map(str::to_owned);
            validate(&report, manifest.source.duration_seconds, stem)?;
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
            Ok(report)
        })();
        let report = match outcome {
            Ok(report) => report,
            Err(error) => {
                cleanup_temporary_job(&temp);
                return Err(error);
            }
        };
        persist(&self.ingest, &track_id, &workspace, stem, &key, &report).await?;
        Ok(report)
    }

    pub fn cached(
        &self,
        track_id: &str,
        stem: Option<&str>,
    ) -> Result<Option<AmtReport>, AmtError> {
        check_stem(stem)?;
        let workspace = self
            .ingest
            .workspace_for(track_id)
            .map_err(|_| AmtError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let Some(final_dir) = cached_dir(&workspace, &manifest, stem)? else {
            return Ok(None);
        };
        let json_path = final_dir.join("transcription.json");
        let midi_path = final_dir.join("transcription.mid");
        if !json_path.is_file() || !valid_midi(&midi_path)? {
            return Ok(None);
        }
        let mut report: AmtReport = serde_json::from_slice(&fs::read(json_path)?)?;
        validate(&report, manifest.source.duration_seconds, stem)?;
        report.cache_hit = true;
        Ok(Some(report))
    }

    pub async fn shutdown(&self) {
        let mut worker = self.worker.lock().await;
        if let Some(mut active) = worker.take() {
            if let Err(error) = active.shutdown().await {
                tracing::warn!(error = %error, "AMT worker shutdown during app exit failed");
            }
        }
    }

    pub fn midi_bytes(&self, track_id: &str, stem: Option<&str>) -> Result<Vec<u8>, AmtError> {
        check_stem(stem)?;
        let workspace = self
            .ingest
            .workspace_for(track_id)
            .map_err(|_| AmtError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let path = cached_dir(&workspace, &manifest, stem)?
            .ok_or_else(|| {
                stem.map_or(AmtError::InvalidResult, |s| {
                    AmtError::MissingStem(s.to_owned())
                })
            })?
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
        if worker.is_none() && *self.restart_failures.lock().await >= MAX_AUTOMATIC_RESTARTS {
            return Err(AmtError::WorkerUnavailable);
        }
        if worker.is_none() {
            match WorkerSupervisor::start(self.launch.clone()).await {
                Ok(started) => *worker = Some(started),
                Err(error) => {
                    *self.restart_failures.lock().await += 1;
                    return Err(map_start(error));
                }
            }
        }
        if let Err(error) = worker
            .as_mut()
            .ok_or(AmtError::WorkerUnavailable)?
            .ping()
            .await
        {
            // A dead or desynchronized worker must not be reused: stdout framing
            // is unknown. Dropping kills the child (kill_on_drop) so the next
            // request starts a clean process, mirroring WorkerManager's handling.
            *worker = None;
            *self.restart_failures.lock().await += 1;
            return Err(AmtError::Worker(error.to_string()));
        }
        let result = worker
            .as_mut()
            .ok_or(AmtError::WorkerUnavailable)?
            .request_with_job(Method::Transcribe, job, params)
            .await;
        match result {
            Ok((response, _events)) => {
                *self.restart_failures.lock().await = 0;
                response.result.ok_or(AmtError::InvalidResult)
            }
            Err(error) => {
                *worker = None;
                *self.restart_failures.lock().await += 1;
                Err(AmtError::Worker(error.to_string()))
            }
        }
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
fn check_stem(stem: Option<&str>) -> Result<(), AmtError> {
    match stem {
        Some(name) if !is_known_stem(name) => Err(AmtError::UnsupportedStem),
        _ => Ok(()),
    }
}
fn amt_dir(workspace: &Path, stem: Option<&str>) -> PathBuf {
    let base = workspace.join("analysis/amt");
    stem.map_or(base.clone(), |stem| base.join(stem))
}
/// The cache directory for this transcription, or `None` when the requested
/// stem has not been separated yet (so nothing can be cached for it).
fn cached_dir(
    workspace: &Path,
    manifest: &TrackManifest,
    stem: Option<&str>,
) -> Result<Option<PathBuf>, AmtError> {
    let stem_hash = match stem {
        Some(name) => match stem_audio(workspace, manifest, name) {
            Ok((_, hash)) => Some(hash),
            Err(StemLookupError::Missing(_)) => return Ok(None),
            Err(error) => return Err(error.into()),
        },
        None => None,
    };
    Ok(Some(amt_dir(workspace, stem).join(cache_key(
        &manifest.source.sha256,
        stem.zip(stem_hash.as_deref()),
    ))))
}
/// Cache signature. The whole-mix form is unchanged from before stems were
/// supported so existing caches stay valid; a stem adds its name and checksum.
fn cache_key(source: &str, stem: Option<(&str, &str)>) -> String {
    let scope = stem.map_or_else(
        || "original".to_owned(),
        |(name, hash)| format!("{name}|{hash}"),
    );
    format!(
        "{:x}",
        Sha256::digest(format!("{source}|amt|{ENGINE}|{VERSION}|{scope}"))
    )
    .chars()
    .take(24)
    .collect()
}
fn validate(report: &AmtReport, duration: f64, stem: Option<&str>) -> Result<(), AmtError> {
    if report.engine != ENGINE
        || report.model != "icassp_2022"
        || report.midi_artifact != "transcription.mid"
        || report.stem.as_deref() != stem
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
async fn persist(
    ingest: &IngestService,
    track_id: &str,
    workspace: &Path,
    stem: Option<&str>,
    key: &str,
    report: &AmtReport,
) -> Result<(), AmtError> {
    // Re-read fresh under a per-track lock rather than reusing the manifest
    // captured at job start: another analysis service may have finished and
    // persisted its own artifacts while this job's worker was running, and
    // writing back a stale in-memory copy would silently erase that work.
    let lock = ingest.track_lock(track_id);
    let _guard = lock.lock().await;
    let mut manifest = read_manifest(workspace)?;
    let (relative, stage, artifact_id) = match stem {
        None => (
            format!("analysis/amt/{key}/transcription.mid"),
            "amt".to_owned(),
            "artifact-amt-midi".to_owned(),
        ),
        Some(stem) => (
            format!("analysis/amt/{stem}/{key}/transcription.mid"),
            format!("amt:{stem}"),
            format!("artifact-amt-midi-{stem}"),
        ),
    };
    match stem {
        None => {
            manifest
                .analysis
                .insert("amt".to_owned(), serde_json::to_value(report)?);
        }
        Some(stem) => {
            let entries = manifest
                .analysis
                .entry("stem_amt".to_owned())
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or(AmtError::InvalidResult)?;
            entries.insert(stem.to_owned(), serde_json::to_value(report)?);
        }
    }
    manifest
        .artifacts
        .retain(|a| !(a.kind == ArtifactKind::Analysis && a.created_by.stage == stage));
    manifest.artifacts.push(Artifact {
        artifact_id,
        kind: ArtifactKind::Analysis,
        relative_path: relative.clone(),
        sha256: format!("{:x}", Sha256::digest(fs::read(workspace.join(&relative))?)),
        stem: None,
        created_by: CreatedBy {
            stage: stage.clone(),
            engine: ENGINE.to_owned(),
            engine_version: VERSION.to_owned(),
            model: Some("icassp_2022".to_owned()),
        },
    });
    let now = OffsetDateTime::now_utc().format(&Rfc3339)?;
    manifest.stages.retain(|s| s.name != stage);
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
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), AmtError> {
    let parent = path.parent().ok_or(AmtError::InvalidResult)?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension("json.tmp");
    // A prior crash between create and rename can leave this file behind;
    // remove it so this write is not permanently blocked by AlreadyExists.
    let _ = fs::remove_file(&temp);
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
fn cleanup_temporary_job(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(error = %error, job_path = %path.display(), "could not remove leftover AMT temporary output");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whole_mix_cache_key_is_unchanged_and_stems_get_their_own() {
        // Stem support must not invalidate caches already on disk.
        let legacy: String = format!("{:x}", Sha256::digest("src|amt|basic-pitch|0.4.0|original"))
            .chars()
            .take(24)
            .collect();
        assert_eq!(cache_key("src", None), legacy);
        let stem = cache_key("src", Some(("other", "h1")));
        assert_ne!(stem, cache_key("src", None));
        assert_ne!(stem, cache_key("src", Some(("other", "h2"))));
        assert_ne!(stem, cache_key("src", Some(("piano", "h1"))));
    }
}
