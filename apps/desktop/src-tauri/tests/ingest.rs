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
