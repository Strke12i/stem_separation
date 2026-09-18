use crate::waveform::WaveformPeaks;
use rodio::{Decoder, Player, Source, stream::MixerDeviceSink};
use serde::Serialize;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, warn};

const TARGET_WAVEFORM_WINDOWS: usize = 1_200;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    Empty,
    Paused,
    Playing,
    Ended,
    DeviceError,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioState {
    pub status: PlaybackStatus,
    pub current_position_seconds: f64,
    pub duration_seconds: f64,
    pub volume: f32,
    pub device_error: Option<String>,
    pub waveform: Option<WaveformPeaks>,
    pub stem_mix: bool,
    pub stems: Vec<StemState>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StemState {
    pub stem: String,
    pub volume: f32,
    pub muted: bool,
    pub solo: bool,
}

#[derive(Clone, Debug)]
pub struct StemInput {
    pub stem: String,
    pub path: PathBuf,
}

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("The selected normalized audio file is unavailable.")]
    MissingAudio,
    #[error("Could not decode the normalized audio file: {detail}")]
    Decode { detail: String },
    #[error("No audio output device is available: {detail}")]
    OutputDevice { detail: String },
    #[error("No track is loaded.")]
    NoTrack,
    #[error("Audio seek failed: {detail}")]
    Seek { detail: String },
    #[error("Volume must be between 0.0 and 2.0.")]
    InvalidVolume,
    #[error("At least two valid stem files are required for a stem mix.")]
    InvalidStemMix,
    #[error("Stem '{stem}' is unavailable or cannot be decoded.")]
    InvalidStem { stem: String },
    #[error("Stem '{stem}' has a duration incompatible with the mix.")]
    UnsynchronizedStem { stem: String },
}

struct LoadedSource {
    stem: Option<String>,
    path: PathBuf,
    volume: f32,
    muted: bool,
    solo: bool,
}

struct LoadedTrack {
    sources: Vec<LoadedSource>,
    duration: Duration,
    waveform: WaveformPeaks,
    parked_position: Duration,
}

/// Keeps OS output and transport state inside Rust. The UI only consumes snapshots.
pub struct AudioEngine {
    output: Option<MixerDeviceSink>,
    players: Vec<Player>,
    track: Option<LoadedTrack>,
    volume: f32,
    last_device_error: Option<String>,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            output: None,
            players: Vec::new(),
            track: None,
            volume: 1.0,
            last_device_error: None,
        }
    }

    pub fn load(&mut self, path: &Path) -> Result<AudioState, AudioError> {
        let (duration, waveform) = inspect_audio(path)?;
        self.stop_players();
        self.track = Some(LoadedTrack {
            sources: vec![LoadedSource {
                stem: None,
                path: path.to_path_buf(),
                volume: 1.0,
                muted: false,
                solo: false,
            }],
            duration,
            waveform,
            parked_position: Duration::ZERO,
        });
        self.open_players_at(Duration::ZERO, false)?;
        Ok(self.snapshot())
    }

    pub fn load_stems(&mut self, stems: Vec<StemInput>) -> Result<AudioState, AudioError> {
        if stems.len() < 2 || stems.iter().any(|stem| stem.stem.is_empty()) {
            return Err(AudioError::InvalidStemMix);
        }
        let (duration, waveform) =
            inspect_audio(&stems[0].path).map_err(|_| AudioError::InvalidStem {
                stem: stems[0].stem.clone(),
            })?;
        for stem in &stems[1..] {
            let stem_duration =
                inspect_duration(&stem.path).map_err(|_| AudioError::InvalidStem {
                    stem: stem.stem.clone(),
                })?;
            if stem_duration.abs_diff(duration) > Duration::from_millis(100) {
                return Err(AudioError::UnsynchronizedStem {
                    stem: stem.stem.clone(),
                });
            }
        }
        self.stop_players();
        self.track = Some(LoadedTrack {
            sources: stems
                .into_iter()
                .map(|stem| LoadedSource {
                    stem: Some(stem.stem),
                    path: stem.path,
                    volume: 1.0,
                    muted: false,
                    solo: false,
                })
                .collect(),
            duration,
            waveform,
            parked_position: Duration::ZERO,
        });
        self.open_players_at(Duration::ZERO, false)?;
        Ok(self.snapshot())
    }

    pub fn play(&mut self) -> Result<AudioState, AudioError> {
        if self.track.is_none() {
            return Err(AudioError::NoTrack);
        }
        if self.players.is_empty() || self.players.iter().all(Player::empty) {
            let position = self.current_position();
            self.open_players_at(position, true)?;
        } else {
            for player in &self.players {
                player.play();
            }
        }
        Ok(self.snapshot())
    }

    pub fn pause(&mut self) -> Result<AudioState, AudioError> {
        if self.players.is_empty() {
            return Err(AudioError::NoTrack);
        }
        for player in &self.players {
            player.pause();
        }
        self.park_current_position();
        Ok(self.snapshot())
    }

    pub fn stop(&mut self) -> Result<AudioState, AudioError> {
        if self.track.is_none() {
            return Err(AudioError::NoTrack);
        }
        self.stop_players();
        if let Some(track) = self.track.as_mut() {
            track.parked_position = Duration::ZERO;
        }
        self.open_players_at(Duration::ZERO, false)?;
        Ok(self.snapshot())
    }

    pub fn seek(&mut self, seconds: f64) -> Result<AudioState, AudioError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(AudioError::Seek {
                detail: "position must be a non-negative finite number".to_owned(),
            });
        }
        let track = self.track.as_ref().ok_or(AudioError::NoTrack)?;
        let position = Duration::from_secs_f64(seconds).min(track.duration);
        let was_playing = self.is_playing();
        self.stop_players();
        self.open_players_at(position, was_playing)?;
        if let Some(track) = self.track.as_mut() {
            track.parked_position = position;
        }
        Ok(self.snapshot())
    }

    pub fn set_volume(&mut self, volume: f32) -> Result<AudioState, AudioError> {
        if !volume.is_finite() || !(0.0..=2.0).contains(&volume) {
            return Err(AudioError::InvalidVolume);
        }
        self.volume = volume;
        self.apply_volumes();
        Ok(self.snapshot())
    }

    pub fn set_stem_volume(&mut self, stem: &str, volume: f32) -> Result<AudioState, AudioError> {
        if !volume.is_finite() || !(0.0..=2.0).contains(&volume) {
            return Err(AudioError::InvalidVolume);
        }
        let source = self.stem_mut(stem)?;
        source.volume = volume;
        self.apply_volumes();
        Ok(self.snapshot())
    }

    pub fn set_stem_muted(&mut self, stem: &str, muted: bool) -> Result<AudioState, AudioError> {
        self.stem_mut(stem)?.muted = muted;
        self.apply_volumes();
        Ok(self.snapshot())
    }

    pub fn set_stem_solo(&mut self, stem: &str, solo: bool) -> Result<AudioState, AudioError> {
        self.stem_mut(stem)?.solo = solo;
        self.apply_volumes();
        Ok(self.snapshot())
    }

    /// Reopens the default output device while retaining the loaded track and position.
    pub fn reopen_output_device(&mut self) -> Result<AudioState, AudioError> {
        let position = self.current_position();
        let was_playing = self.is_playing();
        self.stop_players();
        self.output = None;
        self.open_players_at(position, was_playing)?;
        Ok(self.snapshot())
    }

    #[must_use]
    pub fn snapshot(&self) -> AudioState {
        let Some(track) = &self.track else {
            return AudioState {
                status: PlaybackStatus::Empty,
                current_position_seconds: 0.0,
                duration_seconds: 0.0,
                volume: self.volume,
                device_error: self.last_device_error.clone(),
                waveform: None,
                stem_mix: false,
                stems: Vec::new(),
            };
        };

        let position = self.current_position();
        let status = if self.players.is_empty() && self.last_device_error.is_some() {
            PlaybackStatus::DeviceError
        } else if !self.players.is_empty()
            && self.players.iter().all(Player::empty)
            && position >= track.duration
            && !track.duration.is_zero()
        {
            PlaybackStatus::Ended
        } else if self
            .players
            .iter()
            .any(|player| !player.is_paused() && !player.empty())
        {
            PlaybackStatus::Playing
        } else {
            PlaybackStatus::Paused
        };

        AudioState {
            status,
            current_position_seconds: position.as_secs_f64(),
            duration_seconds: track.duration.as_secs_f64(),
            volume: self.volume,
            device_error: self.last_device_error.clone(),
            waveform: Some(track.waveform.clone()),
            stem_mix: track.sources.iter().any(|source| source.stem.is_some()),
            stems: track
                .sources
                .iter()
                .filter_map(|source| {
                    source.stem.as_ref().map(|stem| StemState {
                        stem: stem.clone(),
                        volume: source.volume,
                        muted: source.muted,
                        solo: source.solo,
                    })
                })
                .collect(),
        }
    }

    fn open_players_at(&mut self, position: Duration, play: bool) -> Result<(), AudioError> {
        let track = self.track.as_ref().ok_or(AudioError::NoTrack)?;
        if self.output.is_none() {
            self.output = Some(open_default_output().inspect_err(|error| {
                self.last_device_error = Some(error.to_string());
            })?);
        }
        let output = self.output.as_ref().ok_or(AudioError::NoTrack)?;
        let volumes = self.effective_volumes();
        let mut players = Vec::with_capacity(track.sources.len());
        for (source, volume) in track.sources.iter().zip(volumes) {
            let file = File::open(&source.path).map_err(|_| AudioError::MissingAudio)?;
            let decoder = Decoder::try_from(file).map_err(|error| AudioError::Decode {
                detail: error.to_string(),
            })?;
            let player = Player::connect_new(output.mixer());
            player.pause();
            player.set_volume(volume);
            player.append(decoder);
            if !position.is_zero() {
                player
                    .try_seek(position)
                    .map_err(|error| AudioError::Seek {
                        detail: error.to_string(),
                    })?;
            }
            if play {
                player.play();
            }
            players.push(player);
        }
        self.last_device_error = None;
        self.players = players;
        if let Some(track) = self.track.as_mut() {
            track.parked_position = position;
        }
        Ok(())
    }

    fn current_position(&self) -> Duration {
        let Some(track) = &self.track else {
            return Duration::ZERO;
        };
        self.players
            .first()
            .map_or(track.parked_position, Player::get_pos)
            .min(track.duration)
    }

    fn park_current_position(&mut self) {
        let position = self.current_position();
        if let Some(track) = self.track.as_mut() {
            track.parked_position = position;
        }
    }

    fn stop_players(&mut self) {
        for player in self.players.drain(..) {
            player.stop();
        }
    }

    fn stem_mut(&mut self, stem: &str) -> Result<&mut LoadedSource, AudioError> {
        self.track
            .as_mut()
            .and_then(|track| {
                track
                    .sources
                    .iter_mut()
                    .find(|source| source.stem.as_deref() == Some(stem))
            })
            .ok_or(AudioError::InvalidStemMix)
    }

    fn is_playing(&self) -> bool {
        self.players
            .iter()
            .any(|player| !player.is_paused() && !player.empty())
    }

    fn effective_volumes(&self) -> Vec<f32> {
        let Some(track) = &self.track else {
            return Vec::new();
        };
        let has_solo = track.sources.iter().any(|source| source.solo);
        let active_count = track
            .sources
            .iter()
            .filter(|source| !source.muted && (!has_solo || source.solo))
            .count()
            .max(1);
        track
            .sources
            .iter()
            .map(|source| {
                if source.muted || (has_solo && !source.solo) {
                    0.0
                } else {
                    self.volume * source.volume / active_count as f32
                }
            })
            .collect()
    }

    fn apply_volumes(&self) {
        for (player, volume) in self.players.iter().zip(self.effective_volumes()) {
            player.set_volume(volume);
        }
    }
}

fn open_default_output() -> Result<MixerDeviceSink, AudioError> {
    rodio::stream::DeviceSinkBuilder::open_default_sink().map_err(|error| {
        AudioError::OutputDevice {
            detail: error.to_string(),
        }
    })
}

fn inspect_audio(path: &Path) -> Result<(Duration, WaveformPeaks), AudioError> {
    let file = File::open(path).map_err(|_| AudioError::MissingAudio)?;
    let decoder = Decoder::try_from(file).map_err(|error| AudioError::Decode {
        detail: error.to_string(),
    })?;
    let duration = decoder.total_duration().ok_or_else(|| AudioError::Decode {
        detail: "decoder did not report a duration".to_owned(),
    })?;
    let total_samples = duration.as_secs_f64()
        * f64::from(decoder.sample_rate().get())
        * f64::from(decoder.channels().get());
    let window_size = (total_samples / TARGET_WAVEFORM_WINDOWS as f64)
        .ceil()
        .max(1.0) as usize;
    let mut min = Vec::with_capacity(TARGET_WAVEFORM_WINDOWS + 1);
    let mut max = Vec::with_capacity(TARGET_WAVEFORM_WINDOWS + 1);
    let mut samples_in_window = 0_usize;
    let mut low = 1.0_f32;
    let mut high = -1.0_f32;
    for sample in decoder {
        let finite_sample = if sample.is_finite() { sample } else { 0.0 };
        low = low.min(finite_sample);
        high = high.max(finite_sample);
        samples_in_window += 1;
        if samples_in_window == window_size {
            min.push(low);
            max.push(high);
            samples_in_window = 0;
            low = 1.0;
            high = -1.0;
        }
    }
    if samples_in_window > 0 {
        min.push(low);
        max.push(high);
    }
    debug!(windows = min.len(), window_size, "generated waveform peaks");
    Ok((
        duration,
        WaveformPeaks {
            sample_windows: window_size,
            min,
            max,
        },
    ))
}

fn inspect_duration(path: &Path) -> Result<Duration, AudioError> {
    let file = File::open(path).map_err(|_| AudioError::MissingAudio)?;
    let decoder = Decoder::try_from(file).map_err(|error| AudioError::Decode {
        detail: error.to_string(),
    })?;
    decoder.total_duration().ok_or_else(|| AudioError::Decode {
        detail: "decoder did not report a duration".to_owned(),
    })
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        if !self.players.is_empty() {
            warn!("stopping active audio player during engine shutdown");
        }
        self.stop_players();
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioEngine, LoadedSource, LoadedTrack, PlaybackStatus};
    use crate::waveform::WaveformPeaks;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn new_engine_has_an_empty_snapshot() {
        let engine = AudioEngine::new();
        let state = engine.snapshot();

        assert_eq!(state.status, PlaybackStatus::Empty);
        assert_eq!(state.duration_seconds, 0.0);
        assert!(state.waveform.is_none());
    }

    #[test]
    fn rejects_volume_outside_supported_range() {
        let mut engine = AudioEngine::new();

        assert!(engine.set_volume(2.1).is_err());
    }

    #[test]
    fn solo_and_mute_apply_headroom_to_the_shared_mix() {
        let mut engine = AudioEngine::new();
        engine.track = Some(LoadedTrack {
            sources: vec![
                LoadedSource {
                    stem: Some("vocals".to_owned()),
                    path: PathBuf::from("vocals.wav"),
                    volume: 1.0,
                    muted: false,
                    solo: false,
                },
                LoadedSource {
                    stem: Some("drums".to_owned()),
                    path: PathBuf::from("drums.wav"),
                    volume: 1.0,
                    muted: false,
                    solo: false,
                },
            ],
            duration: Duration::from_secs(1),
            waveform: WaveformPeaks {
                sample_windows: 1,
                min: vec![0.0],
                max: vec![0.0],
            },
            parked_position: Duration::ZERO,
        });

        assert_eq!(engine.effective_volumes(), vec![0.5, 0.5]);
        engine.set_stem_solo("vocals", true).unwrap();
        assert_eq!(engine.effective_volumes(), vec![1.0, 0.0]);
        engine.set_stem_muted("vocals", true).unwrap();
        assert_eq!(engine.effective_volumes(), vec![0.0, 0.0]);
    }
}
