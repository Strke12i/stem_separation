use analyzer_domain::{Artifact, ArtifactKind, CreatedBy, SourceMetadata, TrackId, TrackManifest};
use local_music_analyzer_desktop::doctor::WorkerManager;
use local_music_analyzer_desktop::ingest::{IngestService, MediaTools};
use local_music_analyzer_desktop::rhythm::RhythmService;
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

fn prepare_workspace(temp: &TempDir) -> (Arc<IngestService>, String) {
    let root = temp.path().join("workspace");
    let track_id = TrackId::new();
    let workspace = root.join(track_id.as_str());
    fs::create_dir_all(workspace.join("normalized")).unwrap();
    fs::write(workspace.join("normalized/source.wav"), b"fixture").unwrap();
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
        artifacts: vec![Artifact {
            artifact_id: "normalized".to_owned(),
            kind: ArtifactKind::NormalizedSource,
            relative_path: "normalized/source.wav".to_owned(),
            sha256: "source-hash".to_owned(),
            stem: None,
            created_by: CreatedBy {
                stage: "normalize".to_owned(),
                engine: "test".to_owned(),
                engine_version: "1".to_owned(),
                model: None,
            },
        }],
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

#[tokio::test]
async fn validates_persists_and_reuses_rhythm_from_the_sidecar() {
    let temporary = TempDir::new().unwrap();
    let (ingest, track_id) = prepare_workspace(&temporary);
    let mut launch = WorkerLaunch::new(
        python(),
        vec![fixture().into_os_string(), "rhythm".into()],
        Duration::from_secs(1),
    );
    launch.request_timeout = Duration::from_secs(1);
    let service = RhythmService::new(Arc::clone(&ingest), Arc::new(WorkerManager::new(launch)));
    let first = service.analyze(track_id.clone()).await.unwrap();
    assert_eq!(first.bpm, 120.0);
    assert!(!first.cache_hit);
    let cached = service.analyze(track_id.clone()).await.unwrap();
    assert!(cached.cache_hit);
    let workspace = ingest.workspace_for(&track_id).unwrap();
    let manifest: TrackManifest =
        serde_json::from_slice(&fs::read(workspace.join("manifest.json")).unwrap()).unwrap();
    assert!(manifest.analysis.contains_key("rhythm"));
    assert!(
        manifest
            .artifacts
            .iter()
            .any(|artifact| artifact.kind == ArtifactKind::Analysis
                && artifact.created_by.stage == "rhythm")
    );
}
