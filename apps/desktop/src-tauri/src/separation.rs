//! Rust-owned separation orchestration, cache validation, and artifact promotion.

use crate::doctor::WorkerManager;
use crate::ingest::IngestService;
use crate::scheduler::ResourceScheduler;
use analyzer_domain::{
    Artifact, ArtifactKind, CreatedBy, JobId, JobStatus, StemKind, TrackManifest,
};
use analyzer_protocol::Event;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::Mutex;

const MODEL_FOUR: &str = "demucs-4";
const MODEL_SIX: &str = "demucs-6-experimental";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub experimental: bool,
    pub stems: Vec<String>,
    pub installed: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparationReport {
    pub job_id: String,
    pub track_id: String,
    pub model_id: String,
    pub cache_hit: bool,
    pub stems: Vec<String>,
    pub progress_events: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparationStatus {
    pub job_id: String,
    pub model_id: String,
    pub stage: String,
    pub progress: f32,
    pub elapsed_seconds: u64,
    pub cancel_requested: bool,
}

#[derive(Clone, Debug)]
pub struct StemFile {
    pub stem: String,
    pub path: PathBuf,
}

pub struct SeparationService {
    ingest: Arc<IngestService>,
    workers: Arc<WorkerManager>,
    scheduler: Arc<ResourceScheduler>,
    execution: Mutex<()>,
    active: Mutex<Option<ActiveJob>>,
}

#[derive(Clone, Debug)]
struct ActiveJob {
    track_id: String,
    job_id: JobId,
    model_id: String,
    stage: String,
    progress: f32,
    started_at: Instant,
    cancelled: bool,
}

#[derive(Debug, Error)]
pub enum SeparationError {
    #[error("The requested model is not in the local registry.")]
    UnknownModel,
    #[error("Model '{0}' is not installed. Install its local bundle before separating audio.")]
    ModelNotInstalled(String),
    #[error("Could not prepare local separation storage: {0}")]
    Storage(#[from] std::io::Error),
    #[error("The track workspace manifest is invalid: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("Could not obtain a timestamp: {0}")]
    Clock(#[from] time::error::Format),
    #[error("The analysis worker failed: {0}")]
    Worker(String),
    #[error("The analysis worker returned an invalid stem manifest.")]
    InvalidWorkerResult,
    #[error("A generated stem failed local validation: {0}")]
    InvalidStem(String),
    #[error("Separation was cancelled before artifacts were promoted.")]
    Cancelled,
}

impl SeparationService {
    #[must_use]
    pub fn new(
        ingest: Arc<IngestService>,
        workers: Arc<WorkerManager>,
        scheduler: Arc<ResourceScheduler>,
    ) -> Self {
        Self {
            ingest,
            workers,
            scheduler,
            execution: Mutex::new(()),
            active: Mutex::new(None),
        }
    }

    pub fn models(&self) -> Vec<ModelInfo> {
        descriptors()
            .iter()
            .map(|descriptor| {
                let installed = model_is_installed(&self.ingest.models_root(), descriptor);
                ModelInfo {
                    id: descriptor.id.to_owned(),
                    label: descriptor.label.to_owned(),
                    experimental: descriptor.experimental,
                    stems: descriptor
                        .stems
                        .iter()
                        .map(|stem| stem.name().to_owned())
                        .collect(),
                    installed,
                    detail: if installed {
                        "Installed locally; no network is used during separation.".to_owned()
                    } else {
                        format!("Local bundle required: {}", descriptor.filename)
                    },
                }
            })
            .collect()
    }

    pub async fn separate(
        &self,
        track_id: String,
        model_id: String,
    ) -> Result<SeparationReport, SeparationError> {
        let _guard = self.execution.lock().await;
        let descriptor = descriptor(&model_id).ok_or(SeparationError::UnknownModel)?;
        let workspace = self
            .ingest
            .workspace_for(&track_id)
            .map_err(|error| SeparationError::Worker(error.to_string()))?;
        let model_dir = workspace
            .parent()
            .unwrap_or(&workspace)
            .join("models")
            .join(descriptor.id);
        if !model_is_installed(model_dir.parent().unwrap_or(&model_dir), descriptor) {
            return Err(SeparationError::ModelNotInstalled(descriptor.id.to_owned()));
        }
        let manifest = read_manifest(&workspace)?;
        let cache_key = cache_key(&manifest.source.sha256, descriptor);
        let final_dir = workspace.join("stems").join(descriptor.id).join(&cache_key);
        if cached_stems_are_valid(&final_dir, descriptor)? {
            return Ok(SeparationReport {
                job_id: "cache-hit".to_owned(),
                track_id,
                model_id,
                cache_hit: true,
                stems: descriptor
                    .stems
                    .iter()
                    .map(|stem| stem.name().to_owned())
                    .collect(),
                progress_events: 0,
            });
        }

        let job_id = JobId::new();
        {
            let mut active = self.active.lock().await;
            *active = Some(ActiveJob {
                track_id: track_id.clone(),
                job_id: job_id.clone(),
                model_id: model_id.clone(),
                stage: "Preparing local workspace".to_owned(),
                progress: 0.05,
                started_at: Instant::now(),
                cancelled: false,
            });
        }
        let temp_dir = workspace.join("tmp").join(job_id.as_str());
        fs::create_dir_all(
            temp_dir
                .parent()
                .ok_or_else(|| SeparationError::InvalidStem("temporary root".to_owned()))?,
        )?;
        let normalized = self
            .ingest
            .normalized_source_for(&track_id)
            .map_err(|error| SeparationError::Worker(error.to_string()))?;
        let params = json!({
            "workspace_path": workspace,
            "input_path": normalized,
            "output_dir": temp_dir,
            "model_id": descriptor.id,
            "model_dir": model_dir,
        });
        let _gpu_permit = self
            .scheduler
            .acquire_gpu()
            .await
            .map_err(|error| SeparationError::Worker(error.to_string()))?;
        self.update_active(&job_id, "Loading Demucs model", 0.12)
            .await;
        let active = &self.active;
        let progress_job_id = job_id.clone();
        let worker_result = self
            .workers
            .separate_with_progress(job_id.clone(), params, move |event| {
                let Some((stage, progress)) = separation_progress(event) else {
                    return;
                };
                // Worker events arrive while this task owns the worker lock. Do not await here:
                // status polling may briefly hold the mutex, in which case the next event wins.
                if let Ok(mut active) = active.try_lock() {
                    if let Some(job) = active.as_mut().filter(|job| job.job_id == progress_job_id) {
                        job.stage = stage;
                        job.progress = progress;
                    }
                }
            })
            .await;
        if self.cancellation_requested(&job_id).await {
            self.clear_active(&job_id).await;
            cleanup_temporary_job(&temp_dir);
            return Err(SeparationError::Cancelled);
        }
        let (result, events) = match worker_result {
            Ok(result) => result,
            Err(error) => {
                self.clear_active(&job_id).await;
                return Err(SeparationError::Worker(error.to_string()));
            }
        };
        self.update_active(&job_id, "Validating generated stems", 0.92)
            .await;
        if let Err(error) = validate_worker_result(&result, &temp_dir, descriptor) {
            self.clear_active(&job_id).await;
            return Err(error);
        }
        self.update_active(&job_id, "Publishing stems to workspace", 0.97)
            .await;
        if let Err(error) = promote(&temp_dir, &final_dir, &cache_key, descriptor) {
            self.clear_active(&job_id).await;
            return Err(error);
        }
        if let Err(error) = persist_artifacts(&workspace, manifest, descriptor, &cache_key) {
            self.clear_active(&job_id).await;
            return Err(error);
        }
        self.clear_active(&job_id).await;

        Ok(SeparationReport {
            job_id: job_id.to_string(),
            track_id,
            model_id,
            cache_hit: false,
            stems: descriptor
                .stems
                .iter()
                .map(|stem| stem.name().to_owned())
                .collect(),
            progress_events: events.len(),
        })
    }

    pub fn stem_files(
        &self,
        track_id: &str,
        model_id: &str,
    ) -> Result<Vec<StemFile>, SeparationError> {
        let descriptor = descriptor(model_id).ok_or(SeparationError::UnknownModel)?;
        let workspace = self
            .ingest
            .workspace_for(track_id)
            .map_err(|error| SeparationError::Worker(error.to_string()))?;
        let manifest = read_manifest(&workspace)?;
        let mut files = Vec::with_capacity(descriptor.stems.len());
        for expected in descriptor.stems {
            let artifact = manifest
                .artifacts
                .iter()
                .find(|artifact| {
                    artifact.kind == ArtifactKind::Stem
                        && artifact.stem.as_ref() == Some(expected)
                        && artifact.created_by.model.as_deref() == Some(descriptor.id)
                })
                .ok_or_else(|| SeparationError::InvalidStem(expected.name().to_owned()))?;
            let relative = Path::new(&artifact.relative_path);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(SeparationError::InvalidStem(expected.name().to_owned()));
            }
            let path = workspace.join(relative).canonicalize()?;
            if !path.starts_with(&workspace) || !valid_wav(&path)? {
                return Err(SeparationError::InvalidStem(expected.name().to_owned()));
            }
            files.push(StemFile {
                stem: expected.name().to_owned(),
                path,
            });
        }
        Ok(files)
    }

    pub async fn cancel_for_track(&self, track_id: &str) -> bool {
        let mut active = self.active.lock().await;
        if let Some(job) = active.as_mut().filter(|job| job.track_id == track_id) {
            job.cancelled = true;
            return true;
        }
        false
    }

    pub async fn status_for_track(&self, track_id: &str) -> Option<SeparationStatus> {
        self.active.lock().await.as_ref().and_then(|job| {
            (job.track_id == track_id).then(|| SeparationStatus {
                job_id: job.job_id.to_string(),
                model_id: job.model_id.clone(),
                stage: job.stage.clone(),
                progress: job.progress,
                elapsed_seconds: job.started_at.elapsed().as_secs(),
                cancel_requested: job.cancelled,
            })
        })
    }

    async fn update_active(&self, job_id: &JobId, stage: &str, progress: f32) {
        let mut active = self.active.lock().await;
        if let Some(job) = active.as_mut().filter(|job| job.job_id == *job_id) {
            job.stage = stage.to_owned();
            job.progress = progress;
        }
    }

    async fn cancellation_requested(&self, job_id: &JobId) -> bool {
        self.active
            .lock()
            .await
            .as_ref()
            .is_some_and(|job| job.job_id == *job_id && job.cancelled)
    }

    async fn clear_active(&self, job_id: &JobId) {
        let mut active = self.active.lock().await;
        if active.as_ref().is_some_and(|job| job.job_id == *job_id) {
            *active = None;
        }
    }
}

#[derive(Clone, Copy)]
struct ModelDescriptor {
    id: &'static str,
    label: &'static str,
    filename: &'static str,
    experimental: bool,
    stems: &'static [StemKind],
}

fn descriptors() -> &'static [ModelDescriptor] {
    const FOUR: &[StemKind] = &[
        StemKind::Vocals,
        StemKind::Drums,
        StemKind::Bass,
        StemKind::Other,
    ];
    const SIX: &[StemKind] = &[
        StemKind::Vocals,
        StemKind::Drums,
        StemKind::Bass,
        StemKind::Other,
        StemKind::Guitar,
        StemKind::Piano,
    ];
    &[
        ModelDescriptor {
            id: MODEL_FOUR,
            label: "Demucs 4 stems",
            filename: "htdemucs.yaml",
            experimental: false,
            stems: FOUR,
        },
        ModelDescriptor {
            id: MODEL_SIX,
            label: "Demucs 6 stems",
            filename: "htdemucs_6s.yaml",
            experimental: true,
            stems: SIX,
        },
    ]
}

fn descriptor(id: &str) -> Option<&'static ModelDescriptor> {
    descriptors().iter().find(|descriptor| descriptor.id == id)
}

fn separation_progress(event: &Event) -> Option<(String, f32)> {
    if event.event != "progress" {
        return None;
    }
    let percent = event.data.get("percent")?.as_f64()?;
    if !percent.is_finite() || !(0.0..=1.0).contains(&percent) {
        return None;
    }
    let stage = match event.data.get("stage")?.as_str()? {
        "loading_model" => "Loading Demucs model",
        "separating" => "Separating stems locally",
        "validating" => "Validating generated stems",
        other => other,
    };
    Some((stage.to_owned(), percent as f32))
}

fn model_is_installed(models_root: &Path, descriptor: &ModelDescriptor) -> bool {
    let directory = models_root.join(descriptor.id);
    directory.join(descriptor.filename).is_file() && directory.join(".lma-model.json").is_file()
}

fn cache_key(source_hash: &str, descriptor: &ModelDescriptor) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!(
            "{source_hash}|audio-separator|{}|{}",
            descriptor.id, descriptor.filename
        ))
    )
    .chars()
    .take(24)
    .collect()
}

fn read_manifest(workspace: &Path) -> Result<TrackManifest, SeparationError> {
    Ok(serde_json::from_slice(&fs::read(
        workspace.join("manifest.json"),
    )?)?)
}

fn cached_stems_are_valid(
    directory: &Path,
    descriptor: &ModelDescriptor,
) -> Result<bool, SeparationError> {
    if !directory.join("separation.json").is_file() {
        return Ok(false);
    }
    for stem in descriptor.stems {
        if !valid_wav(&directory.join(format!("{}.wav", stem.name())))? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_worker_result(
    result: &Value,
    temporary: &Path,
    descriptor: &ModelDescriptor,
) -> Result<(), SeparationError> {
    let stems = result
        .get("stems")
        .and_then(Value::as_array)
        .ok_or(SeparationError::InvalidWorkerResult)?;
    if stems.len() != descriptor.stems.len() {
        return Err(SeparationError::InvalidWorkerResult);
    }
    for expected in descriptor.stems {
        let found = stems
            .iter()
            .find(|item| item.get("stem").and_then(Value::as_str) == Some(expected.name()))
            .ok_or(SeparationError::InvalidWorkerResult)?;
        let relative = found
            .get("relative_path")
            .and_then(Value::as_str)
            .ok_or(SeparationError::InvalidWorkerResult)?;
        let relative_path = Path::new(relative);
        if relative_path.components().count() != 1
            || relative_path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(SeparationError::InvalidWorkerResult);
        }
        let output = temporary.join(relative_path);
        if !output.starts_with(temporary) || !valid_wav(&output)? {
            return Err(SeparationError::InvalidStem(expected.name().to_owned()));
        }
    }
    Ok(())
}

fn valid_wav(path: &Path) -> Result<bool, SeparationError> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.len() <= 44 {
        return Ok(false);
    }
    let mut header = [0_u8; 12];
    BufReader::new(File::open(path)?).read_exact(&mut header)?;
    Ok(&header[..4] == b"RIFF" && &header[8..] == b"WAVE")
}

fn promote(
    temporary: &Path,
    final_dir: &Path,
    cache_key: &str,
    descriptor: &ModelDescriptor,
) -> Result<(), SeparationError> {
    let parent = final_dir
        .parent()
        .ok_or_else(|| SeparationError::InvalidStem("destination".to_owned()))?;
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".{}-staging", cache_key));
    if staging.exists() {
        return Err(SeparationError::InvalidStem(
            "stale staging directory requires manual inspection".to_owned(),
        ));
    }
    fs::rename(temporary, &staging)?;
    let marker = json!({"cache_key": cache_key, "engine": "audio-separator", "model_id": descriptor.id, "stems": descriptor.stems.iter().map(StemKind::name).collect::<Vec<_>>()});
    write_json(&staging.join("separation.json"), &marker)?;
    fs::rename(staging, final_dir)?;
    Ok(())
}

fn persist_artifacts(
    workspace: &Path,
    mut manifest: TrackManifest,
    descriptor: &ModelDescriptor,
    cache_key: &str,
) -> Result<(), SeparationError> {
    manifest.artifacts.retain(|artifact| {
        !(artifact.kind == ArtifactKind::Stem
            && artifact.created_by.model.as_deref() == Some(descriptor.id))
    });
    for stem in descriptor.stems {
        let relative_path = format!("stems/{}/{}/{}.wav", descriptor.id, cache_key, stem.name());
        manifest.artifacts.push(Artifact {
            artifact_id: format!("artifact-{}-{}", descriptor.id, stem.name()),
            kind: ArtifactKind::Stem,
            relative_path: relative_path.clone(),
            sha256: file_hash(&workspace.join(&relative_path))?,
            stem: Some(stem.clone()),
            created_by: CreatedBy {
                stage: "separation".to_owned(),
                engine: "audio-separator".to_owned(),
                engine_version: "python-sidecar".to_owned(),
                model: Some(descriptor.id.to_owned()),
            },
        });
    }
    let now = OffsetDateTime::now_utc().format(&Rfc3339)?;
    manifest
        .stages
        .retain(|stage| stage.name != format!("separation:{}", descriptor.id));
    manifest.stages.push(analyzer_domain::StageRecord {
        name: format!("separation:{}", descriptor.id),
        status: JobStatus::Completed,
        cache_hit: false,
        started_at: now.clone(),
        finished_at: now,
        warnings: Vec::new(),
    });
    write_json(&workspace.join("manifest.json"), &manifest)
}

fn file_hash(path: &Path) -> Result<String, SeparationError> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), SeparationError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let temporary = path.with_extension("json.tmp");
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    let mut writer = BufWriter::new(file);
    writer.write_all(&bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer
        .into_inner()
        .map_err(|error| error.into_error())?
        .sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn cleanup_temporary_job(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(error = %error, job_path = %path.display(), "could not remove cancelled separation temporary output");
        }
    }
}
