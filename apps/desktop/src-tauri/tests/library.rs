use analyzer_domain::{
    Artifact, ArtifactKind, CreatedBy, JobStatus, SourceMetadata, StageRecord, StemKind, TrackId,
    TrackManifest,
};
use local_music_analyzer_desktop::ingest::{IngestService, MediaTools};
use local_music_analyzer_desktop::library::{LibraryQuery, LibraryService, LibrarySort};
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
    let track_id = seed_track(&ingest, "Track.mp3");
    {
        let library = LibraryService::open(Arc::clone(&ingest)).unwrap();
        assert_eq!(library.list().unwrap().len(), 1);
        library.tag(&track_id, "practice").unwrap();
        library.touch_opened(&track_id).unwrap();
    }

    let database_path = ingest.workspace_root().join("library/index.sqlite3");
    fs::write(&database_path, b"not a database").unwrap();

    let library = LibraryService::open(Arc::clone(&ingest)).unwrap();
    let entries = library.list().unwrap();
    assert_eq!(entries.len(), 1);
    // Tags and open history live in the per-track sidecar, not the
    // database, so they must survive even though the database itself did
    // not.
    assert_eq!(entries[0].tags, vec!["practice".to_owned()]);
    assert_eq!(entries[0].open_count, 1);
    assert!(entries[0].last_opened_at.is_some());
}

#[test]
fn tags_and_history_survive_a_full_rebuild() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    let track_id = seed_track(&ingest, "Track.mp3");
    let library = LibraryService::in_memory(Arc::clone(&ingest)).unwrap();
    library.tag(&track_id, "  Funk  ").unwrap();
    library.touch_opened(&track_id).unwrap();
    library.touch_opened(&track_id).unwrap();

    let report = library.rebuild().unwrap();
    assert!(report.rebuilt);

    let entries = library.list().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].tags, vec!["funk".to_owned()]);
    assert_eq!(entries[0].open_count, 2);
}

#[test]
fn searches_and_orders_by_recently_opened() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    let track_a = seed_track(&ingest, "A Song.mp3");
    let track_b = seed_track(&ingest, "B Song.mp3");
    let library = LibraryService::in_memory(Arc::clone(&ingest)).unwrap();
    library.list().unwrap();

    library.touch_opened(&track_b).unwrap();

    let entries = library.list().unwrap();
    assert_eq!(entries[0].track_id, track_b);
    assert_eq!(entries[1].track_id, track_a);
}

#[test]
fn lists_tags_with_track_counts() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    let track_a = seed_track(&ingest, "A Song.mp3");
    let track_b = seed_track(&ingest, "B Song.mp3");
    let library = LibraryService::in_memory(Arc::clone(&ingest)).unwrap();
    library.tag(&track_a, "funk").unwrap();
    library.tag(&track_b, "funk").unwrap();
    library.tag(&track_b, "practice").unwrap();

    let tags = library.tags().unwrap();
    assert_eq!(tags.len(), 2);
    let funk = tags.iter().find(|tag| tag.tag == "funk").unwrap();
    assert_eq!(funk.track_count, 2);
    let practice = tags.iter().find(|tag| tag.tag == "practice").unwrap();
    assert_eq!(practice.track_count, 1);

    library.untag(&track_a, "funk").unwrap();
    let tags = library.tags().unwrap();
    let funk = tags.iter().find(|tag| tag.tag == "funk").unwrap();
    assert_eq!(funk.track_count, 1);
}

#[test]
fn searches_by_name_key_and_tag() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    let track_a = seed_track(&ingest, "Funky Town.mp3");
    let track_b = seed_track(&ingest, "Ballad.mp3");
    let track_c = seed_track(&ingest, "Another Song.mp3");

    let (path_b, mut manifest_b) = read_manifest(&ingest, &track_b);
    manifest_b.analysis.insert(
        "harmony".to_owned(),
        serde_json::json!({"key": {"label": "F# minor"}, "chords": []}),
    );
    fs::write(&path_b, serde_json::to_vec(&manifest_b).unwrap()).unwrap();

    let library = LibraryService::in_memory(Arc::clone(&ingest)).unwrap();
    library.list().unwrap();
    library.tag(&track_a, "funk").unwrap();
    library.tag(&track_c, "funk").unwrap();
    library.tag(&track_c, "practice").unwrap();

    let by_name = library
        .search(&LibraryQuery {
            text: "funky".to_owned(),
            tags: Vec::new(),
            sort: LibrarySort::Name,
        })
        .unwrap();
    assert_eq!(
        by_name
            .iter()
            .map(|e| e.track_id.clone())
            .collect::<Vec<_>>(),
        vec![track_a.clone()]
    );

    let by_key = library
        .search(&LibraryQuery {
            text: "f# minor".to_owned(),
            tags: Vec::new(),
            sort: LibrarySort::Name,
        })
        .unwrap();
    assert_eq!(by_key.len(), 1);
    assert_eq!(by_key[0].track_id, track_b);

    let by_tag_text = library
        .search(&LibraryQuery {
            text: "funk".to_owned(),
            tags: Vec::new(),
            sort: LibrarySort::Name,
        })
        .unwrap();
    let mut ids: Vec<_> = by_tag_text.iter().map(|e| e.track_id.clone()).collect();
    ids.sort();
    let mut expected = vec![track_a.clone(), track_c.clone()];
    expected.sort();
    assert_eq!(ids, expected);

    let intersection = library
        .search(&LibraryQuery {
            text: String::new(),
            tags: vec!["funk".to_owned(), "practice".to_owned()],
            sort: LibrarySort::Name,
        })
        .unwrap();
    assert_eq!(intersection.len(), 1);
    assert_eq!(intersection[0].track_id, track_c);
}

#[test]
fn search_escapes_literal_wildcard_characters() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    let track_id = seed_track(&ingest, "100%_mix.mp3");
    let _decoy = seed_track(&ingest, "unrelated.mp3");
    let library = LibraryService::in_memory(Arc::clone(&ingest)).unwrap();
    library.list().unwrap();

    let matches = library
        .search(&LibraryQuery {
            text: "100%".to_owned(),
            tags: Vec::new(),
            sort: LibrarySort::Name,
        })
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].track_id, track_id);
}

#[test]
fn rejects_an_invalid_tag() {
    let temp = TempDir::new().unwrap();
    let ingest = make_ingest(&temp);
    let track_id = seed_track(&ingest, "Track.mp3");
    let library = LibraryService::in_memory(Arc::clone(&ingest)).unwrap();

    assert!(library.tag(&track_id, "   ").is_err());
    assert!(library.tag(&track_id, &"x".repeat(64)).is_err());
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
