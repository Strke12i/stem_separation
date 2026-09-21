use analyzer_domain::{ArtifactKind, TrackManifest};
use local_music_analyzer_desktop::ingest::{IngestError, IngestService, MediaTools};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}"))
}

fn service(root: &Path) -> IngestService {
    IngestService::new(
        root.to_path_buf(),
        MediaTools::new(fixture("fake_ffprobe.cmd"), fixture("fake_ffmpeg.cmd")),
    )
}

fn write_mp3_fixture(temp: &TempDir) -> PathBuf {
    let source = temp.path().join("fixture.mp3");
    fs::write(&source, b"test input preserved verbatim").expect("fixture must be written");
    source
}

#[test]
fn import_creates_a_portable_workspace_and_preserves_the_original() {
    let temp = TempDir::new().expect("temporary root must be created");
    let source = write_mp3_fixture(&temp);
    let expected_source = fs::read(&source).expect("fixture must be readable");
    let workspace_root = temp.path().join("workspace");

    let imported = service(&workspace_root)
        .import(&source)
        .expect("MP3 fixture should import through FFmpeg boundary");
    let workspace = PathBuf::from(&imported.workspace_path);
    let manifest_path = workspace.join("manifest.json");
    let manifest: TrackManifest = serde_json::from_slice(
        &fs::read(&manifest_path).expect("manifest must be atomically published"),
    )
    .expect("manifest must be valid JSON");

    assert_eq!(manifest.track_id.as_str(), imported.track_id);
    assert_eq!(manifest.source.original_name, "fixture.mp3");
    assert_eq!(manifest.source.duration_seconds, 1.25);
    assert_eq!(manifest.source.sample_rate, 44_100);
    assert_eq!(manifest.source.channels, 2);
    assert_eq!(
        fs::read(workspace.join("source/fixture.mp3")).unwrap(),
        expected_source
    );
    assert!(workspace.join("normalized/source.wav").is_file());
    assert!(
        manifest
            .artifacts
            .iter()
            .any(|artifact| artifact.kind == ArtifactKind::NormalizedSource)
    );
    assert!(
        manifest
            .artifacts
            .iter()
            .all(|artifact| !Path::new(&artifact.relative_path).is_absolute())
    );
    assert!(manifest_path.is_file());
    assert!(!workspace.join("manifest.json.tmp").exists());

    let digest = format!("{:x}", Sha256::digest(&expected_source));
    assert_eq!(manifest.source.sha256, digest);
}

#[test]
fn rejects_invalid_source_before_creating_a_workspace() {
    let temp = TempDir::new().expect("temporary root must be created");
    let source = temp.path().join("not-audio.txt");
    fs::write(&source, b"not audio").expect("fixture must be written");
    let workspace_root = temp.path().join("workspace");

    let error = service(&workspace_root)
        .import(&source)
        .expect_err("unsupported extension must be rejected");

    assert!(matches!(error, IngestError::UnsupportedExtension));
    assert!(!workspace_root.exists());
}

#[test]
fn rejects_probe_without_an_audio_stream() {
    let temp = TempDir::new().expect("temporary root must be created");
    let source = write_mp3_fixture(&temp);
    let workspace_root = temp.path().join("workspace");
    let service = IngestService::new(
        workspace_root.clone(),
        MediaTools::new(
            fixture("fake_ffprobe_no_audio.cmd"),
            fixture("fake_ffmpeg.cmd"),
        ),
    );

    let error = service
        .import(&source)
        .expect_err("a video-only probe must be rejected");

    assert!(matches!(error, IngestError::InvalidAudio));
    assert!(!workspace_root.exists());
}

#[test]
fn removes_staging_files_when_ffmpeg_fails() {
    let temp = TempDir::new().expect("temporary root must be created");
    let source = write_mp3_fixture(&temp);
    let workspace_root = temp.path().join("workspace");
    let service = IngestService::new(
        workspace_root.clone(),
        MediaTools::new(
            fixture("fake_ffprobe.cmd"),
            fixture("fake_ffmpeg_fails.cmd"),
        ),
    );

    let error = service
        .import(&source)
        .expect_err("failing FFmpeg must fail import");

    assert!(matches!(error, IngestError::NormalizeFailed { .. }));
    assert!(!workspace_root.join(".tmp").exists());
    assert!(
        fs::read(&source)
            .expect("original must remain readable")
            .starts_with(b"test input")
    );
}

fn track_directories(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .unwrap()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("track-"))
        .collect();
    names.sort();
    names
}

#[test]
fn importing_the_same_audio_again_reuses_the_track_and_its_saved_work() {
    let temp = TempDir::new().unwrap();
    let source = write_mp3_fixture(&temp);
    let root = temp.path().join("workspace");
    let ingest = service(&root);

    let first = ingest.import(&source).unwrap();
    assert!(!first.reused);
    // Work saved on the track must survive the second import untouched.
    let stem_set = PathBuf::from(&first.workspace_path).join("stems/demucs-4/key");
    fs::create_dir_all(&stem_set).unwrap();
    fs::write(stem_set.join("separation.json"), b"{}").unwrap();

    let again = ingest.import(&source).unwrap();
    assert!(again.reused);
    assert_eq!(again.track_id, first.track_id);
    assert_eq!(again.workspace_path, first.workspace_path);
    assert_eq!(again.original_name, "fixture.mp3");
    assert_eq!(again.duration_seconds, 1.25);
    assert_eq!(track_directories(&root), vec![first.track_id]);
    assert!(stem_set.join("separation.json").is_file());
}

#[test]
fn importing_different_audio_creates_its_own_track() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("workspace");
    let ingest = service(&root);
    let first = ingest.import(&write_mp3_fixture(&temp)).unwrap();

    let other = temp.path().join("other.mp3");
    fs::write(&other, b"different audio bytes").unwrap();
    let second = ingest.import(&other).unwrap();

    assert!(!second.reused);
    assert_ne!(second.track_id, first.track_id);
    assert_eq!(track_directories(&root).len(), 2);
}

#[test]
fn reuse_picks_the_duplicate_that_holds_saved_stems() {
    // Earlier versions imported the same song into a new track every time.
    let temp = TempDir::new().unwrap();
    let source = write_mp3_fixture(&temp);
    let root = temp.path().join("workspace");
    let ingest = service(&root);
    let bare = ingest.import(&source).unwrap();

    // Directories list in name order, so without ranking the bare copy would win.
    let worked_id = loop {
        let candidate = analyzer_domain::TrackId::new();
        if candidate.as_str() > bare.track_id.as_str() {
            break candidate;
        }
    };
    let worked = root.join(worked_id.as_str());
    copy_directory(Path::new(&bare.workspace_path), &worked);
    let mut manifest: TrackManifest =
        serde_json::from_slice(&fs::read(worked.join("manifest.json")).unwrap()).unwrap();
    manifest.track_id = worked_id.clone();
    fs::write(
        worked.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(worked.join("stems/demucs-4/key")).unwrap();
    fs::write(worked.join("stems/demucs-4/key/separation.json"), b"{}").unwrap();
    assert_eq!(track_directories(&root).len(), 2);

    let again = ingest.import(&source).unwrap();
    assert!(again.reused);
    assert_eq!(again.track_id, worked_id.as_str());
    let sources = ingest.tracks_with_source(&manifest.source.sha256);
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].track_id, worked_id.as_str());
}

#[test]
fn a_directory_that_is_not_a_usable_track_is_never_reused() {
    let temp = TempDir::new().unwrap();
    let source = write_mp3_fixture(&temp);
    let root = temp.path().join("workspace");
    let ingest = service(&root);
    let first = ingest.import(&source).unwrap();
    // Its normalized audio is gone, so every later step on it would fail.
    fs::remove_file(Path::new(&first.workspace_path).join("normalized/source.wav")).unwrap();

    let again = ingest.import(&source).unwrap();
    assert!(!again.reused);
    assert_ne!(again.track_id, first.track_id);
}

fn copy_directory(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}
