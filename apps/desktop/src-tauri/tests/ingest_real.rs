//! Opt-in smoke test using real FFmpeg binaries supplied through environment variables.

use analyzer_domain::TrackManifest;
use local_music_analyzer_desktop::ingest::{IngestService, MediaTools};
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

fn required_tool(name: &str) -> OsString {
    std::env::var_os(name).unwrap_or_else(|| panic!("set {name} to run this smoke test"))
}

#[test]
#[ignore = "requires explicitly configured real FFmpeg binaries"]
fn imports_and_normalizes_a_real_synthetic_mp3() {
    let ffmpeg = required_tool("LOCAL_MUSIC_ANALYZER_REAL_FFMPEG");
    let ffprobe = required_tool("LOCAL_MUSIC_ANALYZER_REAL_FFPROBE");
    let temporary = TempDir::new().expect("temporary test workspace must be created");
    let source = temporary.path().join("synthetic.mp3");
    let generation = Command::new(&ffmpeg)
        .args([
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=0.2",
        ])
        .args(["-ac", "2", "-ar", "44100", "-q:a", "4", "-y"])
        .arg(&source)
        .output()
        .expect("FFmpeg must start to generate test MP3");
    assert!(
        generation.status.success(),
        "FFmpeg could not create synthetic MP3"
    );

    let service = IngestService::new(
        temporary.path().join("workspace"),
        MediaTools::new(ffprobe.clone(), ffmpeg),
    );
    let imported = service.import(&source).expect("real MP3 must import");
    let workspace = PathBuf::from(imported.workspace_path);
    let normalized = workspace.join("normalized/source.wav");
    assert!(normalized.is_file());

    let manifest: TrackManifest = serde_json::from_slice(
        &fs::read(workspace.join("manifest.json")).expect("manifest must exist"),
    )
    .expect("manifest must be valid JSON");
    assert_eq!(manifest.source.original_name, "synthetic.mp3");
    assert_eq!(manifest.source.sample_rate, 44_100);
    assert_eq!(manifest.source.channels, 2);

    let verification = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=format_name",
            "-of",
            "default=nw=1",
        ])
        .arg(normalized)
        .output()
        .expect("FFprobe must start");
    assert!(verification.status.success());
    assert!(String::from_utf8_lossy(&verification.stdout).contains("wav"));
}
