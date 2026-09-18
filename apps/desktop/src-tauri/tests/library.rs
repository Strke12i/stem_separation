use analyzer_domain::{
    Artifact, ArtifactKind, CreatedBy, JobStatus, SourceMetadata, StageRecord, StemKind, TrackId,
    TrackManifest,
};
use local_music_analyzer_desktop::ingest::{IngestService, MediaTools};
use local_music_analyzer_desktop::library::LibraryService;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

fn make_ingest(temp: &TempDir) -> Arc<IngestService> {
    Arc::new(IngestService::new(
        temp.path().join("workspace"),
        MediaTools::development(),
    ))
}

fn seed_track(ingest: &IngestService, name: &str) -> String {
    let track_id = TrackId::new();
    let workspace = ingest.workspace_root().join(track_id.as_str());
    fs::create_dir_all(&workspace).unwrap();
    let manifest = TrackManifest {
        schema_version: 1,
        track_id: track_id.clone(),
        source: SourceMetadata {
            sha256: "source-hash".to_owned(),
            original_name: name.to_owned(),
            duration_seconds: 30.0,
            sample_rate: 44_100,
            channels: 2,
        },
        analysis: Default::default(),
        artifacts: Vec::new(),
        stages: vec![StageRecord {
            name: "ingest".to_owned(),
            status: JobStatus::Completed,
            cache_hit: false,
            started_at: "2024-01-01T00:00:00Z".to_owned(),
            finished_at: "2024-01-01T00:00:00Z".to_owned(),
            warnings: Vec::new(),
        }],
        jobs: Vec::new(),
    };
    fs::write(
        workspace.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    track_id.to_string()
}

fn read_manifest(ingest: &IngestService, track_id: &str) -> (std::path::PathBuf, TrackManifest) {
    let path = ingest.workspace_root().join(track_id).join("manifest.json");
    let manifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    (path, manifest)
}

#[test]
fn indexes_every_track_manifest_in_the_workspace_root() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    let track_a = seed_track(&ingest, "Track A.mp3");
    let _track_b = seed_track(&ingest, "Track B.mp3");
    // Decoys the scan must skip: a job temp dir and the models directory,
    // neither of which is a `track-*` workspace.
    fs::create_dir_all(ingest.workspace_root().join(".tmp")).unwrap();
    fs::create_dir_all(ingest.workspace_root().join("models")).unwrap();

    let library = LibraryService::in_memory(Arc::clone(&ingest)).unwrap();
    let entries = library.list().unwrap();

    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| entry.track_id == track_a));
    let entry = entries
        .iter()
        .find(|entry| entry.track_id == track_a)
        .unwrap();
    assert_eq!(entry.original_name, "Track A.mp3");
    assert_eq!(entry.imported_at.as_deref(), Some("2024-01-01T00:00:00Z"));
    assert_eq!(entry.bpm, None);
}

#[test]
fn reindexes_a_manifest_that_changed_on_disk() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    let track_id = seed_track(&ingest, "Track.mp3");
    let library = LibraryService::in_memory(Arc::clone(&ingest)).unwrap();

    let first = library.list().unwrap();
    assert_eq!(first[0].bpm, None);

    let (path, mut manifest) = read_manifest(&ingest, &track_id);
    manifest.analysis.insert(
        "rhythm".to_owned(),
        serde_json::json!({"bpm": 128.0, "beatTimes": []}),
    );
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let second = library.list().unwrap();
    assert_eq!(second[0].bpm, Some(128.0));
}

#[test]
fn forgets_tracks_whose_workspace_was_deleted_outside_the_app() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    let track_id = seed_track(&ingest, "Track.mp3");
    let library = LibraryService::in_memory(Arc::clone(&ingest)).unwrap();
    assert_eq!(library.list().unwrap().len(), 1);

    fs::remove_dir_all(ingest.workspace_root().join(&track_id)).unwrap();

    assert!(library.list().unwrap().is_empty());
}

#[test]
fn rebuilds_the_index_when_the_database_file_is_corrupted() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    seed_track(&ingest, "Track.mp3");
    {
        let library = LibraryService::open(Arc::clone(&ingest)).unwrap();
        assert_eq!(library.list().unwrap().len(), 1);
    }

    let database_path = ingest.workspace_root().join("library/index.sqlite3");
    fs::write(&database_path, b"not a database").unwrap();

    let library = LibraryService::open(Arc::clone(&ingest)).unwrap();
    assert_eq!(library.list().unwrap().len(), 1);
}

#[test]
fn refreshes_a_single_track_after_its_manifest_gains_stems() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    let track_a = seed_track(&ingest, "Track A.mp3");
    let track_b = seed_track(&ingest, "Track B.mp3");
    let library = LibraryService::in_memory(Arc::clone(&ingest)).unwrap();
    library.list().unwrap();

    let (path, mut manifest) = read_manifest(&ingest, &track_a);
    manifest.artifacts.push(Artifact {
        artifact_id: "artifact-vocals".to_owned(),
        kind: ArtifactKind::Stem,
        relative_path: "stems/demucs-4/vocals.wav".to_owned(),
        sha256: "stem-hash".to_owned(),
        stem: Some(StemKind::Vocals),
        created_by: CreatedBy {
            stage: "separation".to_owned(),
            engine: "test".to_owned(),
            engine_version: "1".to_owned(),
            model: Some("demucs-4".to_owned()),
        },
    });
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let refreshed = library.refresh(&track_a).unwrap().unwrap();
    assert_eq!(refreshed.stem_models, vec!["demucs-4".to_owned()]);

    // A targeted refresh of one track must not touch any other track's row.
    let entries = library.list().unwrap();
    let other = entries
        .iter()
        .find(|entry| entry.track_id == track_b)
        .unwrap();
    assert!(other.stem_models.is_empty());
}

#[test]
fn refresh_removes_a_track_whose_workspace_vanished() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    let track_id = seed_track(&ingest, "Track.mp3");
    let library = LibraryService::in_memory(Arc::clone(&ingest)).unwrap();
    library.list().unwrap();

    fs::remove_dir_all(ingest.workspace_root().join(&track_id)).unwrap();

    assert!(library.refresh(&track_id).unwrap().is_none());
    assert!(library.list().unwrap().is_empty());
}
