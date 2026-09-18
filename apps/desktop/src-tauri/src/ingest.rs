//! Rust-owned local audio ingestion and deterministic workspace creation.

use analyzer_domain::{
    Artifact, ArtifactKind, CreatedBy, JobStatus, MANIFEST_SCHEMA_VERSION, SourceMetadata,
    StageRecord, TrackId, TrackManifest,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::{debug, info, warn};

const NORMALIZED_RELATIVE_PATH: &str = "normalized/source.wav";
const SOURCE_DIRECTORY: &str = "source";
const MANIFEST_NAME: &str = "manifest.json";

#[derive(Clone, Debug)]
pub struct MediaTools {
    ffprobe: OsString,
    ffmpeg: OsString,
}

impl MediaTools {
    #[must_use]
    pub fn development() -> Self {
        Self {
            ffprobe: development_media_tool("LOCAL_MUSIC_ANALYZER_FFPROBE", "ffprobe"),
            ffmpeg: development_media_tool("LOCAL_MUSIC_ANALYZER_FFMPEG", "ffmpeg"),
        }
    }

    #[must_use]
    pub fn new(ffprobe: impl Into<OsString>, ffmpeg: impl Into<OsString>) -> Self {
        Self {
            ffprobe: ffprobe.into(),
            ffmpeg: ffmpeg.into(),
        }
    }
}

fn development_media_tool(environment_variable: &str, executable: &str) -> OsString {
    if let Some(configured) = std::env::var_os(environment_variable) {
        return configured;
    }
    #[cfg(windows)]
    if let Some(discovered) = winget_ffmpeg(executable) {
        return discovered.into_os_string();
    }
    OsString::from(executable)
}

#[cfg(windows)]
fn winget_ffmpeg(executable: &str) -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    let packages_root = PathBuf::from(local_app_data)
        .join("Microsoft")
        .join("WinGet")
        .join("Packages");
    winget_ffmpeg_in(&packages_root, executable)
}

#[cfg(windows)]
fn winget_ffmpeg_in(packages_root: &Path, executable: &str) -> Option<PathBuf> {
    let packages = fs::read_dir(packages_root).ok()?;
    for package in packages.flatten() {
        let name = package.file_name();
        if !name
            .to_string_lossy()
            .starts_with("Gyan.FFmpeg.Essentials_")
        {
            continue;
        }
        let versions = fs::read_dir(package.path()).ok()?;
        for version in versions.flatten() {
            let candidate = version.path().join("bin").join(format!("{executable}.exe"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[derive(Clone, Debug)]
pub struct IngestService {
    workspace_root: PathBuf,
    tools: MediaTools,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestedTrack {
    pub track_id: String,
    pub original_name: String,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub workspace_path: String,
}

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("The selected path is not a readable audio file.")]
    InvalidSource,
    #[error("The selected file type is not supported for import.")]
    UnsupportedExtension,
    #[error("The requested imported track is unavailable.")]
    UnknownTrack,
    #[error("Could not complete workspace operation '{operation}': {source}")]
    Workspace {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("Could not read the selected audio file: {0}")]
    ReadSource(#[source] std::io::Error),
    #[error("FFprobe is unavailable. Install FFmpeg or configure LOCAL_MUSIC_ANALYZER_FFPROBE.")]
    FfprobeUnavailable,
    #[error("FFmpeg is unavailable. Install FFmpeg or configure LOCAL_MUSIC_ANALYZER_FFMPEG.")]
    FfmpegUnavailable,
    #[error("FFprobe could not inspect the selected file: {detail}")]
    ProbeFailed { detail: String },
    #[error("The selected file does not contain a valid audio stream.")]
    InvalidAudio,
    #[error("FFmpeg could not normalize the selected file: {detail}")]
    NormalizeFailed { detail: String },
    #[error("Normalized audio output was not created.")]
    MissingNormalizedOutput,
    #[error("Could not serialize the track manifest: {0}")]
    SerializeManifest(#[source] serde_json::Error),
    #[error("Could not generate an ingest timestamp: {0}")]
    Clock(#[source] time::error::Format),
}

impl IngestService {
    #[must_use]
    pub fn development() -> Self {
        let workspace_root = std::env::var_os("LOCAL_MUSIC_ANALYZER_WORKSPACE")
            .map(PathBuf::from)
            .unwrap_or_else(default_workspace_root);
        Self::new(workspace_root, MediaTools::development())
    }

    #[must_use]
    pub fn new(workspace_root: PathBuf, tools: MediaTools) -> Self {
        Self {
            workspace_root,
            tools,
        }
    }

    pub fn import(&self, selected_path: &Path) -> Result<IngestedTrack, IngestError> {
        let source_path = validate_source(selected_path)?;
        let source_metadata = probe_audio(&self.tools, &source_path)?;
        let source_hash = sha256_file(&source_path)?;
        let track_id = TrackId::new();
        let original_name = source_path
            .file_name()
            .and_then(OsStr::to_str)
            .map(ToOwned::to_owned)
            .ok_or(IngestError::InvalidSource)?;

        fs::create_dir_all(&self.workspace_root).map_err(|source| IngestError::Workspace {
            operation: "create workspace root",
            source,
        })?;
        let staging_root = self.workspace_root.join(".tmp").join(track_id.as_str());
        let final_root = self.workspace_root.join(track_id.as_str());
        fs::create_dir_all(&staging_root).map_err(|source| IngestError::Workspace {
            operation: "create staging workspace",
            source,
        })?;

        let result = self.import_to_staging(
            &staging_root,
            &source_path,
            &source_hash,
            &original_name,
            &source_metadata,
            track_id.clone(),
        );
        if let Err(error) = result {
            cleanup_staging(&staging_root);
            return Err(error);
        }

        fs::rename(&staging_root, &final_root).map_err(|source| IngestError::Workspace {
            operation: "publish completed workspace",
            source,
        })?;
        info!(track_id = %track_id, "audio import completed");

        Ok(IngestedTrack {
            track_id: track_id.to_string(),
            original_name,
            duration_seconds: source_metadata.duration_seconds,
            sample_rate: source_metadata.sample_rate,
            channels: source_metadata.channels,
            workspace_path: final_root.to_string_lossy().into_owned(),
        })
    }

    /// Removes only abandoned, directory-shaped job staging areas older than one day.
    /// Completed workspaces and source files are never considered for cleanup.
    pub fn cleanup_stale_temporary(&self) -> Result<usize, IngestError> {
        cleanup_temporary_dirs(&self.workspace_root, Duration::from_secs(24 * 60 * 60)).map_err(
            |source| IngestError::Workspace {
                operation: "clean stale temporary workspaces",
                source,
            },
        )
    }

    /// Marks incomplete jobs as failed after an application restart without touching artifacts.
    pub fn repair_interrupted_workspaces(&self) -> Result<usize, IngestError> {
        let mut repaired = 0;
        let entries = match fs::read_dir(&self.workspace_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(source) => {
                return Err(IngestError::Workspace {
                    operation: "scan workspaces for repair",
                    source,
                });
            }
        };
        for entry in entries.flatten() {
            let manifest_path = entry.path().join(MANIFEST_NAME);
            if !manifest_path.is_file() {
                continue;
            }
            let mut manifest: TrackManifest = match serde_json::from_slice(
                &fs::read(&manifest_path).map_err(IngestError::ReadSource)?,
            ) {
                Ok(manifest) => manifest,
                Err(_) => continue,
            };
            let mut changed = false;
            for job in &mut manifest.jobs {
                if matches!(
                    job.status,
                    JobStatus::Created
                        | JobStatus::Queued
                        | JobStatus::Preparing
                        | JobStatus::Running
                        | JobStatus::Finalizing
                ) {
                    job.status = JobStatus::Failed;
                    changed = true;
                }
            }
            for stage in &mut manifest.stages {
                if matches!(
                    stage.status,
                    JobStatus::Created
                        | JobStatus::Queued
                        | JobStatus::Preparing
                        | JobStatus::Running
                        | JobStatus::Finalizing
                ) {
                    stage.status = JobStatus::Failed;
                    stage.finished_at = utc_now()?;
                    stage.warnings.push(
                        "Interrupted by a previous application shutdown; retry this stage."
                            .to_owned(),
                    );
                    changed = true;
                }
            }
            if changed {
                write_json_atomically(&manifest_path, &manifest)?;
                repaired += 1;
            }
        }
        Ok(repaired)
    }

    pub fn normalized_source_for(&self, track_id: &str) -> Result<PathBuf, IngestError> {
        let track_component = Path::new(track_id);
        if !track_id.starts_with("track-")
            || track_component.components().count() != 1
            || track_component
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(IngestError::UnknownTrack);
        }
        let root = self
            .workspace_root
            .canonicalize()
            .map_err(|_| IngestError::UnknownTrack)?;
        let normalized = root
            .join(track_component)
            .join(NORMALIZED_RELATIVE_PATH)
            .canonicalize()
            .map_err(|_| IngestError::UnknownTrack)?;
        if !normalized.starts_with(&root) || !normalized.is_file() {
            return Err(IngestError::UnknownTrack);
        }
        Ok(normalized)
    }

    pub fn workspace_for(&self, track_id: &str) -> Result<PathBuf, IngestError> {
        let track_component = validated_track_component(track_id)?;
        let root = self
            .workspace_root
            .canonicalize()
            .map_err(|_| IngestError::UnknownTrack)?;
        let workspace = root
            .join(track_component)
            .canonicalize()
            .map_err(|_| IngestError::UnknownTrack)?;
        if !workspace.starts_with(&root) || !workspace.is_dir() {
            return Err(IngestError::UnknownTrack);
        }
        Ok(workspace)
    }

    #[must_use]
    pub fn models_root(&self) -> PathBuf {
        self.workspace_root.join("models")
    }

    #[allow(clippy::too_many_arguments)]
    fn import_to_staging(
        &self,
        staging_root: &Path,
        source_path: &Path,
        source_hash: &str,
        original_name: &str,
        source_metadata: &ProbeMetadata,
        track_id: TrackId,
    ) -> Result<(), IngestError> {
        let copied_source_relative = source_relative_path(original_name)?;
        let copied_source_path = staging_root.join(&copied_source_relative);
        copy_file_atomically(source_path, &copied_source_path)?;

        let normalized_path = staging_root.join(NORMALIZED_RELATIVE_PATH);
        normalize_audio(&self.tools, &copied_source_path, &normalized_path)?;
        let normalized_hash = sha256_file(&normalized_path)?;
        let now = utc_now()?;

        let manifest = TrackManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            track_id,
            source: SourceMetadata {
                sha256: source_hash.to_owned(),
                original_name: original_name.to_owned(),
                duration_seconds: source_metadata.duration_seconds,
                sample_rate: source_metadata.sample_rate,
                channels: source_metadata.channels,
            },
            analysis: serde_json::Map::new(),
            artifacts: vec![
                Artifact {
                    artifact_id: "artifact-source".to_owned(),
                    kind: ArtifactKind::Source,
                    relative_path: path_to_manifest_string(&copied_source_relative)?,
                    sha256: source_hash.to_owned(),
                    stem: None,
                    created_by: CreatedBy {
                        stage: "ingest".to_owned(),
                        engine: "rust-host".to_owned(),
                        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
                        model: None,
                    },
                },
                Artifact {
                    artifact_id: "artifact-normalized-source".to_owned(),
                    kind: ArtifactKind::NormalizedSource,
                    relative_path: NORMALIZED_RELATIVE_PATH.to_owned(),
                    sha256: normalized_hash,
                    stem: None,
                    created_by: CreatedBy {
                        stage: "normalize".to_owned(),
                        engine: "ffmpeg".to_owned(),
                        engine_version: "external".to_owned(),
                        model: None,
                    },
                },
            ],
            stages: vec![
                StageRecord {
                    name: "ingest".to_owned(),
                    status: JobStatus::Completed,
                    cache_hit: false,
                    started_at: now.clone(),
                    finished_at: now.clone(),
                    warnings: Vec::new(),
                },
                StageRecord {
                    name: "normalize".to_owned(),
                    status: JobStatus::Completed,
                    cache_hit: false,
                    started_at: now.clone(),
                    finished_at: now,
                    warnings: Vec::new(),
                },
            ],
            jobs: Vec::new(),
        };
        write_json_atomically(&staging_root.join(MANIFEST_NAME), &manifest)
    }
}

fn default_workspace_root() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data)
                .join("LocalMusicAnalyzer")
                .join("workspace");
        }
    }
    PathBuf::from("workspace")
}

fn validated_track_component(track_id: &str) -> Result<&Path, IngestError> {
    let track_component = Path::new(track_id);
    if !track_id.starts_with("track-")
        || track_component.components().count() != 1
        || track_component
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(IngestError::UnknownTrack);
    }
    Ok(track_component)
}

#[derive(Debug)]
struct ProbeMetadata {
    duration_seconds: f64,
    sample_rate: u32,
    channels: u16,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u16>,
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

fn validate_source(path: &Path) -> Result<PathBuf, IngestError> {
    let canonical = path
        .canonicalize()
        .map_err(|_| IngestError::InvalidSource)?;
    let metadata = canonical
        .metadata()
        .map_err(|_| IngestError::InvalidSource)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(IngestError::InvalidSource);
    }

    let extension = canonical
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .ok_or(IngestError::UnsupportedExtension)?;
    if matches!(
        extension.as_str(),
        "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" | "opus"
    ) {
        Ok(canonical)
    } else {
        Err(IngestError::UnsupportedExtension)
    }
}

fn probe_audio(tools: &MediaTools, source_path: &Path) -> Result<ProbeMetadata, IngestError> {
    let output = Command::new(&tools.ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            "--",
        ])
        .arg(source_path)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                IngestError::FfprobeUnavailable
            } else {
                IngestError::ReadSource(error)
            }
        })?;
    if !output.status.success() {
        return Err(IngestError::ProbeFailed {
            detail: stderr_detail(&output.stderr),
        });
    }
    let probe: ProbeOutput =
        serde_json::from_slice(&output.stdout).map_err(|_| IngestError::InvalidAudio)?;
    parse_probe(probe)
}

fn parse_probe(probe: ProbeOutput) -> Result<ProbeMetadata, IngestError> {
    let audio = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"))
        .ok_or(IngestError::InvalidAudio)?;
    let sample_rate = audio
        .sample_rate
        .as_deref()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .ok_or(IngestError::InvalidAudio)?;
    let channels = audio
        .channels
        .filter(|value| *value > 0)
        .ok_or(IngestError::InvalidAudio)?;
    let duration = audio
        .duration
        .as_deref()
        .or_else(|| {
            probe
                .format
                .as_ref()
                .and_then(|format| format.duration.as_deref())
        })
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or(IngestError::InvalidAudio)?;

    Ok(ProbeMetadata {
        duration_seconds: duration,
        sample_rate,
        channels,
    })
}

fn normalize_audio(tools: &MediaTools, input: &Path, output: &Path) -> Result<(), IngestError> {
    let parent = output
        .parent()
        .ok_or(IngestError::MissingNormalizedOutput)?;
    fs::create_dir_all(parent).map_err(workspace_error("create normalized-audio directory"))?;
    let temporary_output = parent.join("source.tmp.wav");
    let command_output = Command::new(&tools.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-n", "-i"])
        .arg(input)
        .args(["-vn", "-acodec", "pcm_s16le", "-ar", "44100", "-ac", "2"])
        .arg(&temporary_output)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                IngestError::FfmpegUnavailable
            } else {
                IngestError::ReadSource(error)
            }
        })?;
    if !command_output.status.success() {
        return Err(IngestError::NormalizeFailed {
            detail: stderr_detail(&command_output.stderr),
        });
    }
    let metadata = temporary_output
        .metadata()
        .map_err(|_| IngestError::MissingNormalizedOutput)?;
    if metadata.len() == 0 {
        return Err(IngestError::MissingNormalizedOutput);
    }
    OpenOptions::new()
        .write(true)
        .open(&temporary_output)
        .and_then(|file| file.sync_all())
        .map_err(workspace_error("sync normalized-audio output"))?;
    fs::rename(temporary_output, output).map_err(workspace_error("publish normalized-audio output"))
}

fn sha256_file(path: &Path) -> Result<String, IngestError> {
    let file = File::open(path).map_err(IngestError::ReadSource)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes_read = reader.read(&mut buffer).map_err(IngestError::ReadSource)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn copy_file_atomically(source: &Path, destination: &Path) -> Result<(), IngestError> {
    let parent = destination.parent().ok_or(IngestError::InvalidSource)?;
    fs::create_dir_all(parent).map_err(workspace_error("create source directory"))?;
    let temporary = destination.with_extension("copying");
    let source_file = File::open(source).map_err(IngestError::ReadSource)?;
    let destination_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(workspace_error("create source copy"))?;
    let mut reader = BufReader::new(source_file);
    let mut writer = BufWriter::new(destination_file);
    std::io::copy(&mut reader, &mut writer).map_err(IngestError::ReadSource)?;
    writer
        .flush()
        .map_err(workspace_error("flush source copy"))?;
    writer
        .into_inner()
        .map_err(|error| IngestError::Workspace {
            operation: "finish source copy",
            source: error.into_error(),
        })?
        .sync_all()
        .map_err(workspace_error("sync source copy"))?;
    fs::rename(temporary, destination).map_err(workspace_error("publish source copy"))
}

fn write_json_atomically(path: &Path, manifest: &TrackManifest) -> Result<(), IngestError> {
    let serialized = serde_json::to_vec_pretty(manifest).map_err(IngestError::SerializeManifest)?;
    let temporary = path.with_extension("json.tmp");
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(workspace_error("create temporary manifest"))?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(&serialized)
        .map_err(workspace_error("write temporary manifest"))?;
    writer
        .write_all(b"\n")
        .map_err(workspace_error("finish temporary manifest"))?;
    writer
        .flush()
        .map_err(workspace_error("flush temporary manifest"))?;
    writer
        .into_inner()
        .map_err(|error| IngestError::Workspace {
            operation: "finish temporary manifest",
            source: error.into_error(),
        })?
        .sync_all()
        .map_err(workspace_error("sync temporary manifest"))?;
    fs::rename(temporary, path).map_err(workspace_error("publish manifest"))
}

fn source_relative_path(original_name: &str) -> Result<PathBuf, IngestError> {
    let file_name = Path::new(original_name)
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(IngestError::InvalidSource)?;
    Ok(Path::new(SOURCE_DIRECTORY).join(file_name))
}

fn path_to_manifest_string(path: &Path) -> Result<String, IngestError> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(IngestError::InvalidSource);
    }
    Ok(path.to_string_lossy().replace('\\', "/"))
}

fn utc_now() -> Result<String, IngestError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(IngestError::Clock)
}

fn stderr_detail(stderr: &[u8]) -> String {
    let detail = String::from_utf8_lossy(stderr);
    let trimmed = detail.trim();
    if trimmed.is_empty() {
        "unknown external-tool error".to_owned()
    } else {
        trimmed.chars().take(500).collect()
    }
}

fn workspace_error(operation: &'static str) -> impl FnOnce(std::io::Error) -> IngestError {
    move |source| IngestError::Workspace { operation, source }
}

fn cleanup_staging(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        debug!(error = %error, "could not remove failed ingest staging directory");
    }
    if let Some(parent) = path.parent() {
        if let Err(error) = fs::remove_dir(parent) {
            if error.kind() != std::io::ErrorKind::DirectoryNotEmpty {
                warn!(error = %error, "could not remove empty ingest staging parent");
            }
        }
    }
}

fn cleanup_temporary_dirs(root: &Path, minimum_age: Duration) -> std::io::Result<usize> {
    let now = SystemTime::now();
    let mut candidates = Vec::new();
    let ingest_staging = root.join(".tmp");
    if ingest_staging.is_dir() {
        candidates.extend(
            fs::read_dir(ingest_staging)?
                .flatten()
                .map(|entry| entry.path()),
        );
    }
    if root.is_dir() {
        for workspace in fs::read_dir(root)?.flatten().map(|entry| entry.path()) {
            let jobs = workspace.join("tmp");
            if jobs.is_dir() {
                candidates.extend(fs::read_dir(jobs)?.flatten().map(|entry| entry.path()));
            }
        }
    }
    let mut removed = 0;
    for candidate in candidates {
        let age = candidate
            .metadata()?
            .modified()
            .ok()
            .and_then(|time| now.duration_since(time).ok());
        if candidate.is_dir() && age.is_some_and(|value| value >= minimum_age) {
            fs::remove_dir_all(candidate)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(all(test, windows))]
mod development_media_tools_tests {
    use super::{default_workspace_root, winget_ffmpeg_in};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn finds_ffprobe_in_a_winget_essentials_installation() {
        let temporary = TempDir::new().expect("temporary directory must exist");
        let binary = temporary
            .path()
            .join("Gyan.FFmpeg.Essentials_Test")
            .join("ffmpeg-test")
            .join("bin");
        fs::create_dir_all(&binary).expect("fixture directory must be created");
        fs::write(binary.join("ffprobe.exe"), b"fixture").expect("fixture binary must be written");

        let found = winget_ffmpeg_in(temporary.path(), "ffprobe")
            .expect("Winget FFprobe must be discovered");

        assert_eq!(found, binary.join("ffprobe.exe"));
    }

    #[test]
    fn defaults_workspace_to_local_app_data_not_the_source_tree() {
        let local_app_data = std::env::var_os("LOCALAPPDATA")
            .expect("Windows must expose LOCALAPPDATA for development");
        assert_eq!(
            default_workspace_root(),
            std::path::PathBuf::from(local_app_data)
                .join("LocalMusicAnalyzer")
                .join("workspace")
        );
    }
}

#[cfg(test)]
mod resilience_tests {
    use super::cleanup_temporary_dirs;
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn removes_only_job_and_ingest_temporary_directories() {
        let root = TempDir::new().expect("temporary workspace root must exist");
        fs::create_dir_all(root.path().join(".tmp/track-staging"))
            .expect("ingest staging fixture must exist");
        fs::create_dir_all(root.path().join("track-live/tmp/job-abandoned"))
            .expect("job staging fixture must exist");
        fs::create_dir_all(root.path().join("track-live/source"))
            .expect("published source fixture must exist");

        assert_eq!(
            cleanup_temporary_dirs(root.path(), Duration::ZERO).unwrap(),
            2
        );
        assert!(root.path().join("track-live/source").is_dir());
    }
}
