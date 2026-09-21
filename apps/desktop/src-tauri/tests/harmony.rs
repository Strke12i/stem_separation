use analyzer_domain::{
    Artifact, ArtifactKind, CreatedBy, SourceMetadata, StemKind, TrackId, TrackManifest,
};
use local_music_analyzer_desktop::doctor::WorkerManager;
use local_music_analyzer_desktop::harmony::HarmonyService;
use local_music_analyzer_desktop::ingest::{IngestService, MediaTools};
use local_music_analyzer_desktop::supervisor::WorkerLaunch;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

fn python() -> OsString {
    std::env::var_os("PYTHON").unwrap_or_else(|| OsString::from("python"))
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_worker.py")
}

fn artifact(id: &str, kind: ArtifactKind, path: &str, stem: Option<StemKind>) -> Artifact {
    Artifact {
        artifact_id: id.to_owned(),
        kind,
        relative_path: path.to_owned(),
        sha256: format!("{id}-hash"),
        stem,
        created_by: CreatedBy {
            stage: "test".to_owned(),
            engine: "test".to_owned(),
            engine_version: "1".to_owned(),
            model: None,
        },
    }
}

/// A track with a normalized source and a separated `other` stem.
fn prepare_workspace(temp: &TempDir) -> (Arc<IngestService>, String) {
    let root = temp.path().join("workspace");
    let track_id = TrackId::new();
    let workspace = root.join(track_id.as_str());
    fs::create_dir_all(workspace.join("normalized")).unwrap();
    fs::create_dir_all(workspace.join("stems/demucs-4")).unwrap();
    fs::write(
        workspace.join("normalized/source.wav"),
        b"RIFF____WAVEfixture",
    )
    .unwrap();
    fs::write(
        workspace.join("stems/demucs-4/other.wav"),
        b"RIFF____WAVEfixture",
    )
    .unwrap();
    let manifest = TrackManifest {
        schema_version: 1,
        track_id: track_id.clone(),
        source: SourceMetadata {
            sha256: "source-hash".to_owned(),
            original_name: "source.wav".to_owned(),
            duration_seconds: 4.0,
            sample_rate: 44_100,
            channels: 2,
        },
        analysis: Default::default(),
        artifacts: vec![
            artifact(
                "normalized",
                ArtifactKind::NormalizedSource,
                "normalized/source.wav",
                None,
            ),
            artifact(
                "other",
                ArtifactKind::Stem,
                "stems/demucs-4/other.wav",
                Some(StemKind::Other),
            ),
        ],
        stages: Vec::new(),
        jobs: Vec::new(),
    };
    fs::write(
        workspace.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    (
        Arc::new(IngestService::new(root, MediaTools::development())),
        track_id.to_string(),
    )
}

fn service(ingest: &Arc<IngestService>) -> HarmonyService {
    let mut launch = WorkerLaunch::new(
        python(),
        vec![fixture().into_os_string(), "harmony".into()],
        Duration::from_secs(5),
    );
    launch.request_timeout = Duration::from_secs(5);
    HarmonyService::new(Arc::clone(ingest), Arc::new(WorkerManager::new(launch)))
}

fn manifest_of(ingest: &IngestService, track_id: &str) -> TrackManifest {
    let workspace = ingest.workspace_for(track_id).unwrap();
    serde_json::from_slice(&fs::read(workspace.join("manifest.json")).unwrap()).unwrap()
}

#[tokio::test]
async fn a_stem_is_analyzed_and_cached_separately_from_the_mix() {
    let temporary = TempDir::new().unwrap();
    let (ingest, track_id) = prepare_workspace(&temporary);

    let stem = service(&ingest)
        .analyze(track_id.clone(), Some("other".to_owned()))
        .await
        .unwrap();
    assert_eq!(stem.stem.as_deref(), Some("other"));
    assert!(!stem.cache_hit);

    // A fresh service (fresh fake worker) for the mix: a second request to the
    // same fake would exceed its single job.
    let mix = service(&ingest)
        .analyze(track_id.clone(), None)
        .await
        .unwrap();
    assert_eq!(mix.stem, None);

    let manifest = manifest_of(&ingest, &track_id);
    assert!(manifest.analysis["harmony"].is_object());
    assert!(manifest.analysis["stem_harmony"]["other"].is_object());
    let stages: Vec<_> = manifest
        .artifacts
        .iter()
        .filter(|a| a.kind == ArtifactKind::Analysis)
        .map(|a| a.created_by.stage.as_str())
        .collect();
    assert!(
        stages.contains(&"harmony") && stages.contains(&"harmony:other"),
        "{stages:?}"
    );

    let svc = service(&ingest);
    assert!(
        svc.cached(&track_id, Some("other"))
            .unwrap()
            .unwrap()
            .cache_hit
    );
    assert!(svc.cached(&track_id, None).unwrap().unwrap().cache_hit);
    // Never separated, so nothing to find rather than an error.
    assert!(svc.cached(&track_id, Some("piano")).unwrap().is_none());
}

#[tokio::test]
async fn stem_names_that_are_not_stems_never_reach_a_path() {
    let temporary = TempDir::new().unwrap();
    let (ingest, track_id) = prepare_workspace(&temporary);
    let svc = service(&ingest);

    assert!(svc.cached(&track_id, Some("../other")).is_err());
    assert!(
        svc.analyze(track_id, Some("../other".to_owned()))
            .await
            .is_err()
    );
}
