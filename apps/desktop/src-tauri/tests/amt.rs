use analyzer_domain::{Artifact, ArtifactKind, CreatedBy, SourceMetadata, TrackId, TrackManifest};
use local_music_analyzer_desktop::amt::AmtService;
use local_music_analyzer_desktop::ingest::{IngestService, MediaTools};
use local_music_analyzer_desktop::scheduler::ResourceScheduler;
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
async fn validates_persists_and_reuses_transcription_from_the_sidecar() {
    let temporary = TempDir::new().unwrap();
    let (ingest, track_id) = prepare_workspace(&temporary);
    let mut launch = WorkerLaunch::new(
        python(),
        vec![fixture().into_os_string(), "amt".into()],
        Duration::from_secs(1),
    );
    launch.request_timeout = Duration::from_secs(1);
    let service = AmtService::new(
        Arc::clone(&ingest),
        Arc::new(ResourceScheduler::new()),
        launch,
    );

    let first = service.transcribe(track_id.clone(), None).await.unwrap();
    assert_eq!(first.engine, "basic-pitch");
    assert_eq!(first.notes.len(), 1);
    assert!(!first.cache_hit);

    let cached = service.transcribe(track_id.clone(), None).await.unwrap();
    assert!(cached.cache_hit);

    let workspace = ingest.workspace_for(&track_id).unwrap();
    let manifest: TrackManifest =
        serde_json::from_slice(&fs::read(workspace.join("manifest.json")).unwrap()).unwrap();
    assert!(manifest.analysis.contains_key("amt"));
    assert!(
        manifest
            .artifacts
            .iter()
            .any(|artifact| artifact.kind == ArtifactKind::Analysis
                && artifact.created_by.stage == "amt")
    );
}

fn service_in_mode(ingest: &Arc<IngestService>, args: Vec<OsString>) -> AmtService {
    let mut launch = WorkerLaunch::new(python(), args, Duration::from_secs(5));
    launch.request_timeout = Duration::from_secs(5);
    AmtService::new(
        Arc::clone(ingest),
        Arc::new(ResourceScheduler::new()),
        launch,
    )
}

#[tokio::test]
async fn an_invalid_transcription_leaves_no_temporary_or_final_output() {
    let temporary = TempDir::new().unwrap();
    let (ingest, track_id) = prepare_workspace(&temporary);
    let service = service_in_mode(
        &ingest,
        vec![fixture().into_os_string(), "amt_bad_midi".into()],
    );

    let error = service
        .transcribe(track_id.clone(), None)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("invalid"), "{error}");
    let workspace = ingest.workspace_for(&track_id).unwrap();
    let leftovers = fs::read_dir(workspace.join("tmp"))
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(
        leftovers, 0,
        "the worker's temporary output must be removed"
    );
    assert!(!workspace.join("analysis/amt").exists());
    let manifest: TrackManifest =
        serde_json::from_slice(&fs::read(workspace.join("manifest.json")).unwrap()).unwrap();
    assert!(!manifest.analysis.contains_key("amt"));
}

#[tokio::test]
async fn a_crashed_worker_is_discarded_and_the_next_request_starts_a_fresh_one() {
    let temporary = TempDir::new().unwrap();
    let (ingest, track_id) = prepare_workspace(&temporary);
    let marker = temporary.path().join("crashed-once");
    let service = service_in_mode(
        &ingest,
        vec![
            fixture().into_os_string(),
            "amt_crash_once".into(),
            marker.clone().into_os_string(),
        ],
    );

    assert!(service.transcribe(track_id.clone(), None).await.is_err());
    assert!(marker.exists(), "the first worker process must have run");

    let report = service.transcribe(track_id.clone(), None).await.unwrap();
    assert_eq!(report.notes.len(), 1);
    assert!(!report.cache_hit);
}

#[tokio::test]
async fn a_stem_transcription_lives_beside_the_mix_transcription() {
    use analyzer_domain::StemKind;
    let temporary = TempDir::new().unwrap();
    let (ingest, track_id) = prepare_workspace(&temporary);
    let workspace = ingest.workspace_for(&track_id).unwrap();
    // Add a separated `other` stem to the prepared track.
    fs::create_dir_all(workspace.join("stems/demucs-4")).unwrap();
    fs::write(
        workspace.join("stems/demucs-4/other.wav"),
        b"RIFF____WAVEfixture",
    )
    .unwrap();
    let mut manifest: TrackManifest =
        serde_json::from_slice(&fs::read(workspace.join("manifest.json")).unwrap()).unwrap();
    let mut stem = manifest.artifacts[0].clone();
    stem.artifact_id = "other".to_owned();
    stem.kind = ArtifactKind::Stem;
    stem.relative_path = "stems/demucs-4/other.wav".to_owned();
    stem.sha256 = "other-hash".to_owned();
    stem.stem = Some(StemKind::Other);
    manifest.artifacts.push(stem);
    fs::write(
        workspace.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    let of_stem = service_in_mode(&ingest, vec![fixture().into_os_string(), "amt".into()])
        .transcribe(track_id.clone(), Some("other".to_owned()))
        .await
        .unwrap();
    assert_eq!(of_stem.stem.as_deref(), Some("other"));
    let of_mix = service_in_mode(&ingest, vec![fixture().into_os_string(), "amt".into()])
        .transcribe(track_id.clone(), None)
        .await
        .unwrap();
    assert_eq!(of_mix.stem, None);

    let manifest: TrackManifest =
        serde_json::from_slice(&fs::read(workspace.join("manifest.json")).unwrap()).unwrap();
    assert!(manifest.analysis["amt"].is_object());
    assert!(manifest.analysis["stem_amt"]["other"].is_object());
    let stages: Vec<_> = manifest
        .artifacts
        .iter()
        .filter(|a| a.kind == ArtifactKind::Analysis)
        .map(|a| a.created_by.stage.as_str())
        .collect();
    assert!(
        stages.contains(&"amt") && stages.contains(&"amt:other"),
        "{stages:?}"
    );

    let service = service_in_mode(&ingest, vec![fixture().into_os_string(), "amt".into()]);
    assert!(
        service
            .cached(&track_id, Some("other"))
            .unwrap()
            .unwrap()
            .cache_hit
    );
    assert!(
        service
            .midi_bytes(&track_id, Some("other"))
            .unwrap()
            .starts_with(b"MThd")
    );
    assert!(
        service
            .midi_bytes(&track_id, None)
            .unwrap()
            .starts_with(b"MThd")
    );
    assert!(service.cached(&track_id, Some("piano")).unwrap().is_none());
    assert!(service.cached(&track_id, Some("../x")).is_err());
}
