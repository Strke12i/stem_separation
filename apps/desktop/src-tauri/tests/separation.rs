use analyzer_domain::{Artifact, ArtifactKind, CreatedBy, SourceMetadata, TrackId, TrackManifest};
use local_music_analyzer_desktop::doctor::WorkerManager;
use local_music_analyzer_desktop::ingest::{IngestService, MediaTools};
use local_music_analyzer_desktop::scheduler::ResourceScheduler;
use local_music_analyzer_desktop::separation::SeparationService;
use local_music_analyzer_desktop::supervisor::WorkerLaunch;
use sha2::{Digest, Sha256};
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

fn wav() -> Vec<u8> {
    let mut bytes = vec![0_u8; 48];
    bytes[..4].copy_from_slice(b"RIFF");
    bytes[8..12].copy_from_slice(b"WAVE");
    bytes
}

fn prepare_workspace(temp: &TempDir) -> (Arc<IngestService>, String) {
    let root = temp.path().join("workspace");
    let track_id = TrackId::new();
    let workspace = root.join(track_id.as_str());
    fs::create_dir_all(workspace.join("normalized")).unwrap();
    fs::write(workspace.join("normalized/source.wav"), wav()).unwrap();
    let manifest = TrackManifest {
        schema_version: 1,
        track_id: track_id.clone(),
        source: SourceMetadata {
            sha256: "source-hash".to_owned(),
            original_name: "source.wav".to_owned(),
            duration_seconds: 1.0,
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
    let model = root.join("models/demucs-4");
    fs::create_dir_all(&model).unwrap();
    fs::write(model.join("htdemucs.yaml"), "fixture").unwrap();
    fs::write(
        model.join(".lma-model.json"),
        r#"{"model_id":"demucs-4","model_filename":"htdemucs.yaml"}"#,
    )
    .unwrap();
    let config_hash = format!("{:x}", Sha256::digest(b"fixture"));
    fs::write(
        model.join(".lma-bundle.json"),
        format!(
            r#"{{"schema_version":1,"model_id":"demucs-4","model_filename":"htdemucs.yaml","installed_at":"2024-01-01T00:00:00Z","files":[{{"relative_path":"htdemucs.yaml","size_bytes":7,"sha256":"{config_hash}"}}]}}"#
        ),
    )
    .unwrap();
    (
        Arc::new(IngestService::new(root, MediaTools::development())),
        track_id.to_string(),
    )
}

#[tokio::test]
async fn promotes_validated_stems_and_reuses_the_cache() {
    let temp = TempDir::new().unwrap();
    let (ingest, track_id) = prepare_workspace(&temp);
    let mut launch = WorkerLaunch::new(
        python(),
        vec![fixture().into_os_string(), "separate".into()],
        Duration::from_secs(1),
    );
    launch.request_timeout = Duration::from_secs(1);
    let workers = Arc::new(WorkerManager::new(launch));
    let service = SeparationService::new(
        Arc::clone(&ingest),
        workers,
        Arc::new(ResourceScheduler::new()),
    );

    let first = service
        .separate(track_id.clone(), "demucs-4".to_owned())
        .await
        .unwrap();
    assert!(!first.cache_hit);
    assert_eq!(first.stems.len(), 4);
    let workspace = ingest.workspace_for(&track_id).unwrap();
    let manifest: TrackManifest =
        serde_json::from_slice(&fs::read(workspace.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        manifest
            .artifacts
            .iter()
            .filter(|artifact| artifact.kind == ArtifactKind::Stem)
            .count(),
        4
    );
    assert!(
        manifest
            .artifacts
            .iter()
            .filter(|artifact| artifact.kind == ArtifactKind::Stem)
            .all(|artifact| workspace.join(&artifact.relative_path).is_file())
    );

    let cached = service
        .separate(track_id, "demucs-4".to_owned())
        .await
        .unwrap();
    assert!(cached.cache_hit);
}

#[tokio::test]
async fn cancellation_never_promotes_partial_outputs() {
    let temp = TempDir::new().unwrap();
    let (ingest, track_id) = prepare_workspace(&temp);
    let mut launch = WorkerLaunch::new(
        python(),
        vec![fixture().into_os_string(), "separate_slow".into()],
        Duration::from_secs(1),
    );
    launch.request_timeout = Duration::from_secs(1);
    let service = Arc::new(SeparationService::new(
        Arc::clone(&ingest),
        Arc::new(WorkerManager::new(launch)),
        Arc::new(ResourceScheduler::new()),
    ));
    let running = tokio::spawn({
        let service = Arc::clone(&service);
        let track_id = track_id.clone();
        async move { service.separate(track_id, "demucs-4".to_owned()).await }
    });
    // Python start-up and the bundle checksum vary with machine load, so wait
    // for the worker's second progress event instead of sleeping a fixed time.
    let mut status = None;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(25)).await;
        status = service.status_for_track(&track_id).await;
        if status
            .as_ref()
            .is_some_and(|status| status.stage == "Separating stems locally")
        {
            break;
        }
    }
    let status = status.expect("active separation must expose a status");
    assert_eq!(status.stage, "Separating stems locally");
    assert_eq!(status.progress, 0.5);
    assert!(service.cancel_for_track(&track_id).await);
    let error = running.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    let workspace = ingest.workspace_for(&track_id).unwrap();
    assert!(!workspace.join("stems/demucs-4").exists());
}

/// A separation whose worker would run for a minute, already past its first
/// progress event.
async fn start_hanging_separation(
    temp: &TempDir,
) -> (
    Arc<IngestService>,
    String,
    Arc<WorkerManager>,
    Arc<SeparationService>,
    tokio::task::JoinHandle<
        Result<
            local_music_analyzer_desktop::separation::SeparationReport,
            local_music_analyzer_desktop::separation::SeparationError,
        >,
    >,
) {
    let (ingest, track_id) = prepare_workspace(temp);
    let mut launch = WorkerLaunch::new(
        python(),
        vec![fixture().into_os_string(), "separate_hang".into()],
        Duration::from_secs(5),
    );
    launch.request_timeout = Duration::from_secs(5);
    let workers = Arc::new(WorkerManager::new(launch));
    let service = Arc::new(SeparationService::new(
        Arc::clone(&ingest),
        Arc::clone(&workers),
        Arc::new(ResourceScheduler::new()),
    ));
    let running = tokio::spawn({
        let service = Arc::clone(&service);
        let track_id = track_id.clone();
        async move { service.separate(track_id, "demucs-4".to_owned()).await }
    });
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(25)).await;
        let status = service.status_for_track(&track_id).await;
        if status.is_some_and(|status| status.stage == "Separating stems locally") {
            return (ingest, track_id, workers, service, running);
        }
    }
    panic!("the worker never reported separation progress");
}

#[tokio::test]
async fn cancelling_stops_the_worker_instead_of_waiting_for_it() {
    let temp = TempDir::new().unwrap();
    let (ingest, track_id, workers, service, running) = start_hanging_separation(&temp).await;

    let cancelled_at = std::time::Instant::now();
    assert!(service.cancel_for_track(&track_id).await);
    let error = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("cancel must not wait for the 60 s worker")
        .unwrap()
        .unwrap_err();

    assert!(error.to_string().contains("cancelled"));
    assert!(cancelled_at.elapsed() < Duration::from_secs(10));
    let workspace = ingest.workspace_for(&track_id).unwrap();
    assert!(!workspace.join("stems/demucs-4").exists());
    assert_eq!(
        fs::read_dir(workspace.join("tmp")).unwrap().count(),
        0,
        "the cancelled job's temporary output must be removed"
    );
    // A deliberate cancel is not a crash: the next request starts a fresh worker.
    let report = workers.doctor().await;
    assert!(
        report.analysis_worker.ok,
        "{}",
        report.analysis_worker.detail
    );
}

#[tokio::test]
async fn a_second_separation_is_visibly_queued_and_cancellable_without_touching_the_first() {
    let temp = TempDir::new().unwrap();
    let (_ingest, first, _workers, service, running) = start_hanging_separation(&temp).await;
    // Another track in the same workspace root, requested while the first runs.
    let (_other_ingest, second) = prepare_workspace(&temp);
    let queued = tokio::spawn({
        let service = Arc::clone(&service);
        let second = second.clone();
        async move { service.separate(second, "demucs-4".to_owned()).await }
    });

    let mut status = None;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(25)).await;
        status = service.status_for_track(&second).await;
        if status.is_some() {
            break;
        }
    }
    let status = status.expect("a waiting separation must expose a status");
    assert_eq!(status.stage, "Queued behind another separation");
    assert_eq!(status.progress, 0.0);

    assert!(service.cancel_for_track(&second).await);
    let error = tokio::time::timeout(Duration::from_secs(5), queued)
        .await
        .expect("a queued job must be cancellable without waiting for the running one")
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert!(service.status_for_track(&second).await.is_none());

    // The first separation was not affected.
    let first_status = service.status_for_track(&first).await.unwrap();
    assert_eq!(first_status.stage, "Separating stems locally");
    assert!(!first_status.cancel_requested);

    assert!(service.cancel_for_track(&first).await);
    let _ = tokio::time::timeout(Duration::from_secs(10), running).await;
}

#[tokio::test]
async fn doctor_reports_busy_and_restart_cancels_the_running_job() {
    let temp = TempDir::new().unwrap();
    let (_ingest, _track_id, workers, _service, running) = start_hanging_separation(&temp).await;

    let report = tokio::time::timeout(Duration::from_secs(2), workers.doctor())
        .await
        .expect("the health check must not wait for a running job");
    assert!(report.analysis_worker.ok);
    assert!(report.analysis_worker.detail.contains("busy"));

    let report = tokio::time::timeout(Duration::from_secs(10), workers.restart())
        .await
        .expect("restart must not wait for a running job");
    assert!(
        report.analysis_worker.ok,
        "{}",
        report.analysis_worker.detail
    );
    let error = running.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("cancelled"));
}

#[tokio::test]
async fn rejects_a_model_bundle_whose_checksum_no_longer_matches() {
    let temp = TempDir::new().unwrap();
    let (ingest, track_id) = prepare_workspace(&temp);
    // Simulate corruption/tampering since install: the bundle inventory still
    // claims the original checksum, but the file on disk has changed.
    let workspace = ingest.workspace_for(&track_id).unwrap();
    let model = workspace.parent().unwrap().join("models").join("demucs-4");
    fs::write(model.join("htdemucs.yaml"), "tampered").unwrap();

    let mut launch = WorkerLaunch::new(
        python(),
        vec![fixture().into_os_string(), "separate".into()],
        Duration::from_secs(1),
    );
    launch.request_timeout = Duration::from_secs(1);
    let service = SeparationService::new(
        Arc::clone(&ingest),
        Arc::new(WorkerManager::new(launch)),
        Arc::new(ResourceScheduler::new()),
    );

    let error = service
        .separate(track_id.clone(), "demucs-4".to_owned())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("checksum"));
    // No separation attempt should have run against the tampered bundle.
    assert!(!workspace.join("stems/demucs-4").exists());
}
