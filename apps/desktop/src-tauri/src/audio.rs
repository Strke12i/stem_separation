use analyzer_audio::{
    AudioEngine, AudioState, PreparedTrack, StemInput, prepare_stem_mix, prepare_track,
};
use std::path::Path;
use std::sync::Mutex;

pub struct AudioManager {
    engine: Mutex<AudioEngine>,
}

impl Default for AudioManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            engine: Mutex::new(AudioEngine::new()),
        }
    }

    // Inspecting a track decodes it, which takes seconds for a stem mix. That
    // happens before the engine lock is taken: the playback controls and the
    // position poll share the lock and must not stall behind a load.
    pub fn load_normalized(&self, path: &Path) -> Result<AudioState, String> {
        let prepared = prepare_track(path).map_err(|error| error.to_string())?;
        self.install(prepared)
    }

    pub fn load_stems(&self, stems: Vec<StemInput>) -> Result<AudioState, String> {
        let prepared = prepare_stem_mix(stems).map_err(|error| error.to_string())?;
        self.install(prepared)
    }

    fn install(&self, prepared: PreparedTrack) -> Result<AudioState, String> {
        self.engine
            .lock()
            .map_err(|_| "Audio engine state is unavailable.".to_owned())?
            .install(prepared)
            .map_err(|error| error.to_string())
    }

    pub fn play(&self) -> Result<AudioState, String> {
        self.with_engine(|engine| engine.play())
    }

    pub fn pause(&self) -> Result<AudioState, String> {
        self.with_engine(|engine| engine.pause())
    }

    pub fn stop(&self) -> Result<AudioState, String> {
        self.with_engine(|engine| engine.stop())
    }

    pub fn seek(&self, seconds: f64) -> Result<AudioState, String> {
        self.with_engine(|engine| engine.seek(seconds))
    }

    pub fn set_volume(&self, volume: f32) -> Result<AudioState, String> {
        self.with_engine(|engine| engine.set_volume(volume))
    }

    pub fn set_stem_volume(&self, stem: &str, volume: f32) -> Result<AudioState, String> {
        self.with_engine(|engine| engine.set_stem_volume(stem, volume))
    }

    pub fn set_stem_muted(&self, stem: &str, muted: bool) -> Result<AudioState, String> {
        self.with_engine(|engine| engine.set_stem_muted(stem, muted))
    }

    pub fn set_stem_solo(&self, stem: &str, solo: bool) -> Result<AudioState, String> {
        self.with_engine(|engine| engine.set_stem_solo(stem, solo))
    }

    pub fn reopen_output_device(&self) -> Result<AudioState, String> {
        self.with_engine(|engine| engine.reopen_output_device())
    }

    pub fn snapshot(&self) -> Result<AudioState, String> {
        self.engine
            .lock()
            .map_err(|_| "Audio engine state is unavailable.".to_owned())
            .map(|engine| engine.snapshot())
    }

    fn with_engine(
        &self,
        operation: impl FnOnce(&mut AudioEngine) -> Result<AudioState, analyzer_audio::AudioError>,
    ) -> Result<AudioState, String> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|_| "Audio engine state is unavailable.".to_owned())?;
        operation(&mut engine).map_err(|error| error.to_string())
    }
}
