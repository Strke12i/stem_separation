use analyzer_domain::{
    Artifact, ArtifactKind, CreatedBy, SourceMetadata, StemKind, TrackId, TrackManifest,
};
use local_music_analyzer_desktop::doctor::WorkerManager;
use local_music_analyzer_desktop::ingest::{IngestService, MediaTools};
use local_music_analyzer_desktop::pitch::PitchService;
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
    fs::create_dir_all(workspace.join("stems/demucs-4")).unwrap();
    fs::write(
        workspace.join("stems/demucs-4/bass.wav"),
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
            Artifact {
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
            },
            Artifact {
                artifact_id: "bass".to_owned(),
                kind: ArtifactKind::Stem,
                relative_path: "stems/demucs-4/bass.wav".to_owned(),
                sha256: "bass-hash".to_owned(),
                stem: Some(StemKind::Bass),
                created_by: CreatedBy {
                    stage: "separation".to_owned(),
                    engine: "test".to_owned(),
                    engine_version: "1".to_owned(),
                    model: Some("demucs-4".to_owned()),
                },
            },
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

/// Rhythm and pitch each capture the manifest at job start and used to write
/// their whole in-memory copy back at job end. Running two analyses for the
/// same track concurrently, each backed by its own worker process so they
/// truly overlap, used to let whichever finished last silently erase the
/// other's already-persisted artifact - the exact class of bug described in
/// docs/ARCHITECTURE.md's job-lifecycle invariants.
#[tokio::test]
async fn concurrent_analyses_do_not_clobber_each_others_artifacts() {
    let temporary = TempDir::new().unwrap();
    let (ingest, track_id) = prepare_workspace(&temporary);

    let mut rhythm_launch = WorkerLaunch::new(
        python(),
        vec![fixture().into_os_string(), "rhythm".into()],
        Duration::from_secs(2),
    );
    rhythm_launch.request_timeout = Duration::from_secs(2);
    let rhythm = RhythmService::new(
        Arc::clone(&ingest),
        Arc::new(WorkerManager::new(rhythm_launch)),
    );

    let mut pitch_launch = WorkerLaunch::new(
        python(),
        vec![fixture().into_os_string(), "pitch".into()],
        Duration::from_secs(2),
    );
    pitch_launch.request_timeout = Duration::from_secs(2);
    let pitch = PitchService::new(
        Arc::clone(&ingest),
        Arc::new(WorkerManager::new(pitch_launch)),
    );

    let (rhythm_result, pitch_result) = tokio::join!(
        rhythm.analyze(track_id.clone()),
        pitch.analyze(track_id.clone(), "bass".to_owned())
    );
    rhythm_result.unwrap();
    pitch_result.unwrap();

    let workspace = ingest.workspace_for(&track_id).unwrap();
    let manifest: TrackManifest =
        serde_json::from_slice(&fs::read(workspace.join("manifest.json")).unwrap()).unwrap();

    assert!(manifest.analysis.contains_key("rhythm"));
    assert!(manifest.analysis["pitch"]["bass"].is_object());
    assert!(
        manifest
            .artifacts
            .iter()
            .any(|artifact| artifact.created_by.stage == "rhythm")
    );
    assert!(
        manifest
            .artifacts
            .iter()
            .any(|artifact| artifact.created_by.stage == "pitch:bass")
    );
    // The separation artifact seeded before either analysis ran must also
    // survive both writes.
    assert!(
        manifest
            .artifacts
            .iter()
            .any(|artifact| artifact.created_by.stage == "separation")
    );
}
