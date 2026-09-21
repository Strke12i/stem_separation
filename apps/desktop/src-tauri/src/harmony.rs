use crate::doctor::WorkerManager;
use crate::ingest::IngestService;
use crate::stems::{StemLookupError, is_known_stem, stem_audio};
use analyzer_domain::{
    Artifact, ArtifactKind, CreatedBy, JobId, JobStatus, StageRecord, TrackManifest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::Mutex;

const ENGINE: &str = "librosa.chroma_cqt+templates";
// Beat-aligned smoothing yields a few segments per second at most; this is
// generous for many hours of audio while keeping a runaway worker result from
// bloating the manifest and every IPC response that carries it.
const MAX_CHORD_SEGMENTS: usize = 50_000;

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
    /// The stem this was computed from; `None` for the whole mix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stem: Option<String>,
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
    #[error("Harmony analysis is not available for that stem name.")]
    UnsupportedStem,
    #[error("The requested {0} stem is unavailable. Generate local stems first.")]
    MissingStem(String),
    #[error("Could not read or write harmony artifacts: {0}")]
    Storage(#[from] std::io::Error),
    #[error("The track manifest is invalid: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("Could not obtain a timestamp: {0}")]
    Clock(#[from] time::error::Format),
}
impl From<StemLookupError> for HarmonyError {
    fn from(error: StemLookupError) -> Self {
        match error {
            StemLookupError::Missing(stem) => Self::MissingStem(stem),
            StemLookupError::Io(error) => Self::Storage(error),
        }
    }
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
    /// Key and chords for the whole mix (`stem: None`) or for one separated stem.
    pub async fn analyze(
        &self,
        track_id: String,
        stem: Option<String>,
    ) -> Result<HarmonyReport, HarmonyError> {
        let _guard = self.execution.lock().await;
        let stem = stem.as_deref();
        check_stem(stem)?;
        let workspace = self
            .ingest
            .workspace_for(&track_id)
            .map_err(|_| HarmonyError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let beats = beats(&manifest);
        let (input, stem_hash) = match stem {
            Some(name) => {
                let (path, hash) = stem_audio(&workspace, &manifest, name)?;
                (path, Some(hash))
            }
            None => (
                self.ingest
                    .normalized_source_for(&track_id)
                    .map_err(|_| HarmonyError::UnknownTrack)?,
                None,
            ),
        };
        let signature = key(
            &manifest.source.sha256,
            stem.zip(stem_hash.as_deref()),
            &beats,
        );
        let path = cache_dir(&workspace, stem)
            .join(&signature)
            .join("harmony.json");
        if path.is_file() {
            let mut cached: HarmonyReport = serde_json::from_slice(&fs::read(path)?)?;
            validate(&cached, manifest.source.duration_seconds, stem)?;
            cached.cache_hit = true;
            return Ok(cached);
        }
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
        report.stem = stem.map(str::to_owned);
        validate(&report, manifest.source.duration_seconds, stem)?;
        write_json(&path, &report)?;
        persist(
            &self.ingest,
            &track_id,
            &workspace,
            stem,
            &signature,
            &report,
        )
        .await?;
        Ok(report)
    }
    pub fn cached(
        &self,
        track_id: &str,
        stem: Option<&str>,
    ) -> Result<Option<HarmonyReport>, HarmonyError> {
        check_stem(stem)?;
        let workspace = self
            .ingest
            .workspace_for(track_id)
            .map_err(|_| HarmonyError::UnknownTrack)?;
        let manifest = read_manifest(&workspace)?;
        let stem_hash = match stem {
            Some(name) => match stem_audio(&workspace, &manifest, name) {
                Ok((_, hash)) => Some(hash),
                // No such stem yet, so there is nothing cached for it.
                Err(StemLookupError::Missing(_)) => return Ok(None),
                Err(error) => return Err(error.into()),
            },
            None => None,
        };
        let path = cache_dir(&workspace, stem)
            .join(key(
                &manifest.source.sha256,
                stem.zip(stem_hash.as_deref()),
                &beats(&manifest),
            ))
            .join("harmony.json");
        if !path.is_file() {
            return Ok(None);
        }
        let mut report: HarmonyReport = serde_json::from_slice(&fs::read(path)?)?;
        validate(&report, manifest.source.duration_seconds, stem)?;
        report.cache_hit = true;
        Ok(Some(report))
    }
}
fn check_stem(stem: Option<&str>) -> Result<(), HarmonyError> {
    match stem {
        Some(name) if !is_known_stem(name) => Err(HarmonyError::UnsupportedStem),
        _ => Ok(()),
    }
}
fn cache_dir(workspace: &Path, stem: Option<&str>) -> PathBuf {
    let base = workspace.join("analysis/harmony");
    stem.map_or(base.clone(), |stem| base.join(stem))
}
fn beats(manifest: &TrackManifest) -> Vec<f64> {
    manifest
        .analysis
        .get("rhythm")
        .and_then(|v| v.get("beatTimes").or_else(|| v.get("beat_times")))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}
/// Cache signature. The whole-mix form is unchanged from before stems were
/// supported so existing caches stay valid; a stem adds its name and checksum.
fn key(source: &str, stem: Option<(&str, &str)>, beats: &[f64]) -> String {
    let stem = stem.map_or_else(String::new, |(name, hash)| format!("|{name}|{hash}"));
    format!(
        "{:x}",
        Sha256::digest(format!("{source}|harmony|{ENGINE}{stem}|{beats:?}"))
    )
    .chars()
    .take(24)
    .collect()
}
fn validate(report: &HarmonyReport, duration: f64, stem: Option<&str>) -> Result<(), HarmonyError> {
    if report.algorithm != ENGINE
        || report.stem.as_deref() != stem
        || report.chords.len() > MAX_CHORD_SEGMENTS
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
async fn persist(
    ingest: &IngestService,
    track_id: &str,
    workspace: &Path,
    stem: Option<&str>,
    signature: &str,
    report: &HarmonyReport,
) -> Result<(), HarmonyError> {
    // Re-read fresh under a per-track lock rather than reusing the manifest
    // captured at job start: another analysis service may have finished and
    // persisted its own artifacts while this job's worker was running, and
    // writing back a stale in-memory copy would silently erase that work.
    let lock = ingest.track_lock(track_id);
    let _guard = lock.lock().await;
    let mut manifest = read_manifest(workspace)?;
    let (relative, stage, artifact_id) = match stem {
        None => (
            format!("analysis/harmony/{signature}/harmony.json"),
            "harmony".to_owned(),
            "artifact-harmony".to_owned(),
        ),
        Some(stem) => (
            format!("analysis/harmony/{stem}/{signature}/harmony.json"),
            format!("harmony:{stem}"),
            format!("artifact-harmony-{stem}"),
        ),
    };
    match stem {
        // The whole-mix result keeps its own key: the library index reads it.
        None => {
            manifest
                .analysis
                .insert("harmony".to_owned(), serde_json::to_value(report)?);
        }
        Some(stem) => {
            let entries = manifest
                .analysis
                .entry("stem_harmony".to_owned())
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or(HarmonyError::InvalidResult)?;
            entries.insert(stem.to_owned(), serde_json::to_value(report)?);
        }
    }
    manifest.artifacts.retain(|artifact| {
        !(artifact.kind == ArtifactKind::Analysis && artifact.created_by.stage == stage)
    });
    manifest.artifacts.push(Artifact {
        artifact_id,
        kind: ArtifactKind::Analysis,
        relative_path: relative.clone(),
        sha256: format!("{:x}", Sha256::digest(fs::read(workspace.join(&relative))?)),
        stem: None,
        created_by: CreatedBy {
            stage: stage.clone(),
            engine: ENGINE.to_owned(),
            engine_version: "0.1".to_owned(),
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
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), HarmonyError> {
    let parent = path.parent().ok_or(HarmonyError::InvalidResult)?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    // A prior crash between create and rename can leave this file behind;
    // remove it so this write is not permanently blocked by AlreadyExists.
    let _ = fs::remove_file(&temporary);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn report(chords: usize) -> HarmonyReport {
        HarmonyReport {
            key: KeyReport {
                tonic: "C".to_owned(),
                mode: "major".to_owned(),
                label: "C major".to_owned(),
                score: 0.9,
                second_best: "A minor".to_owned(),
                margin: 0.1,
            },
            chords: (0..chords)
                .map(|index| ChordSegment {
                    start: index as f64 * 0.001,
                    end: index as f64 * 0.001 + 0.0005,
                    label: "C".to_owned(),
                    root: Some("C".to_owned()),
                    quality: Some("major".to_owned()),
                    score: 0.9,
                    beat_aligned: false,
                })
                .collect(),
            algorithm: ENGINE.to_owned(),
            stem: None,
            cache_hit: false,
        }
    }

    fn legacy_hash(input: &str) -> String {
        format!("{:x}", Sha256::digest(input))
            .chars()
            .take(24)
            .collect()
    }

    #[test]
    fn the_whole_mix_cache_key_is_unchanged_and_stems_get_their_own() {
        // Stem support must not invalidate caches already on disk.
        assert_eq!(
            key("src", None, &[0.5, 1.0]),
            legacy_hash("src|harmony|librosa.chroma_cqt+templates|[0.5, 1.0]")
        );
        let stem = key("src", Some(("other", "h1")), &[0.5, 1.0]);
        assert_ne!(stem, key("src", None, &[0.5, 1.0]));
        assert_ne!(stem, key("src", Some(("other", "h2")), &[0.5, 1.0]));
        assert_ne!(stem, key("src", Some(("piano", "h1")), &[0.5, 1.0]));
    }

    #[test]
    fn accepts_a_result_at_the_segment_cap_and_rejects_one_above_it() {
        assert!(validate(&report(MAX_CHORD_SEGMENTS), 1_000.0, None).is_ok());
        assert!(matches!(
            validate(&report(MAX_CHORD_SEGMENTS + 1), 1_000.0, None),
            Err(HarmonyError::InvalidResult)
        ));
    }
}
