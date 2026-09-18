use analyzer_audio::{AudioEngine, PlaybackStatus};
use std::path::PathBuf;

#[test]
#[ignore = "requires a real audio fixture and an available output device"]
fn loads_seeks_and_controls_real_audio_without_python() {
    let fixture = std::env::var_os("LOCAL_MUSIC_ANALYZER_AUDIO_FIXTURE")
        .map(PathBuf::from)
        .expect("set LOCAL_MUSIC_ANALYZER_AUDIO_FIXTURE to run this test");
    let mut engine = AudioEngine::new();

    let loaded = engine.load(&fixture).expect("audio fixture should load");
    assert_eq!(loaded.status, PlaybackStatus::Paused);
    assert!(loaded.duration_seconds > 0.0);
    assert!(loaded.waveform.is_some());

    let sought = engine.seek(0.05).expect("seek should succeed");
    assert!(sought.current_position_seconds >= 0.0);
    engine.play().expect("play should succeed");
    engine.pause().expect("pause should succeed");
    let stopped = engine.stop().expect("stop should succeed");
    assert!(stopped.current_position_seconds <= 0.01);
}
