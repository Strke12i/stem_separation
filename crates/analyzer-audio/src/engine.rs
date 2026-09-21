use crate::waveform::{WaveformPeaks, mix_waveforms};
use rodio::{Decoder, Player, Source, stream::MixerDeviceSink};
use serde::Serialize;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, warn};

// Waveform resolution follows the track length so a zoomed-in timeline still
// has detail: about 20 peaks per second, bounded so a very long file cannot
// produce an unreasonable payload.
const WAVEFORM_WINDOWS_PER_SECOND: f64 = 20.0;
const MIN_WAVEFORM_WINDOWS: usize = 1_200;
const MAX_WAVEFORM_WINDOWS: usize = 40_000;

fn waveform_windows(duration: Duration) -> usize {
    ((duration.as_secs_f64() * WAVEFORM_WINDOWS_PER_SECOND).ceil() as usize)
        .clamp(MIN_WAVEFORM_WINDOWS, MAX_WAVEFORM_WINDOWS)
}

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

/// One stem's own peaks, for drawing a per-track waveform.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StemWaveform {
    pub stem: String,
    pub waveform: WaveformPeaks,
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
    waveform: Option<WaveformPeaks>,
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

/// A track whose audio has been inspected (duration and waveform) but that is
/// not yet playing. Inspecting decodes the file, which takes noticeable time,
/// so it is a separate step that does not need the engine.
pub struct PreparedTrack {
    sources: Vec<LoadedSource>,
    duration: Duration,
    waveform: WaveformPeaks,
}

fn source(stem: Option<String>, path: PathBuf, waveform: Option<WaveformPeaks>) -> LoadedSource {
    LoadedSource {
        stem,
        path,
        waveform,
        volume: 1.0,
        muted: false,
        solo: false,
    }
}

pub fn prepare_track(path: &Path) -> Result<PreparedTrack, AudioError> {
    let (duration, waveform) = inspect_audio(path)?;
    Ok(PreparedTrack {
        sources: vec![source(None, path.to_path_buf(), None)],
        duration,
        waveform,
    })
}

/// Inspects every stem (in parallel, so the wall time is that of the slowest
/// one), checks they line up, and combines their peaks into the mix waveform.
pub fn prepare_stem_mix(stems: Vec<StemInput>) -> Result<PreparedTrack, AudioError> {
    if stems.len() < 2 || stems.iter().any(|stem| stem.stem.is_empty()) {
        return Err(AudioError::InvalidStemMix);
    }
    let inspected: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = stems
            .iter()
            .map(|stem| scope.spawn(|| inspect_audio(&stem.path)))
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle.join().unwrap_or_else(|_| {
                    Err(AudioError::Decode {
                        detail: "stem inspection panicked".to_owned(),
                    })
                })
            })
            .collect()
    });

    let mut duration: Option<Duration> = None;
    let mut parts = Vec::with_capacity(stems.len());
    for (stem, result) in stems.iter().zip(inspected) {
        let (stem_duration, waveform) = result.map_err(|_| AudioError::InvalidStem {
            stem: stem.stem.clone(),
        })?;
        let first = *duration.get_or_insert(stem_duration);
        if stem_duration.abs_diff(first) > Duration::from_millis(100) {
            return Err(AudioError::UnsynchronizedStem {
                stem: stem.stem.clone(),
            });
        }
        parts.push(waveform);
    }
    let waveform = mix_waveforms(&parts);
    Ok(PreparedTrack {
        sources: stems
            .into_iter()
            .zip(parts)
            .map(|(stem, peaks)| source(Some(stem.stem), stem.path, Some(peaks)))
            .collect(),
        duration: duration.unwrap_or_default(),
        waveform,
    })
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
        self.install(prepare_track(path)?)
    }

    pub fn load_stems(&mut self, stems: Vec<StemInput>) -> Result<AudioState, AudioError> {
        self.install(prepare_stem_mix(stems)?)
    }

    /// Makes a prepared track current and opens its players. Cheap compared with
    /// preparing it, so callers that share the engine behind a lock only hold
    /// the lock for this step.
    pub fn install(&mut self, prepared: PreparedTrack) -> Result<AudioState, AudioError> {
        self.stop_players();
        self.track = Some(LoadedTrack {
            sources: prepared.sources,
            duration: prepared.duration,
            waveform: prepared.waveform,
            parked_position: Duration::ZERO,
        });
        self.open_players_at(Duration::ZERO, false)?;
        Ok(self.snapshot_with_waveform())
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

    /// Peaks of every stem in a stem mix, in mix order; empty otherwise.
    #[must_use]
    pub fn stem_waveforms(&self) -> Vec<StemWaveform> {
        self.track.as_ref().map_or_else(Vec::new, |track| {
            track
                .sources
                .iter()
                .filter_map(|source| {
                    Some(StemWaveform {
                        stem: source.stem.clone()?,
                        waveform: source.waveform.clone()?,
                    })
                })
                .collect()
        })
    }

    /// Transport state without the waveform. The UI polls this several times a
    /// second, and the waveform (thousands of points) only changes on load, so
    /// it travels with the load responses instead (`snapshot_with_waveform`).
    #[must_use]
    pub fn snapshot(&self) -> AudioState {
        AudioState {
            waveform: None,
            ..self.snapshot_with_waveform()
        }
    }

    #[must_use]
    pub fn snapshot_with_waveform(&self) -> AudioState {
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
        // Every source is opened, decoded, and seeked before any of them are
        // started: starting each player as soon as it is ready (the previous
        // approach) let later stems begin audibly later than earlier ones on
        // a multi-stem mix, growing with file size and cold cache.
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
            players.push(player);
        }
        if play {
            for player in &players {
                player.play();
            }
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
    let target_windows = waveform_windows(duration);
    let window_size = (total_samples / target_windows as f64).ceil().max(1.0) as usize;
    let mut min = Vec::with_capacity(target_windows + 1);
    let mut max = Vec::with_capacity(target_windows + 1);
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
    use super::{
        AudioEngine, AudioError, LoadedSource, LoadedTrack, PlaybackStatus, StemInput,
        prepare_stem_mix, waveform_windows,
    };
    use crate::waveform::WaveformPeaks;
    use std::path::PathBuf;
    use std::time::Duration;

    /// Mono 8 kHz 16-bit WAV alternating +amplitude / -amplitude for `seconds`.
    fn write_square_wav(path: &std::path::Path, amplitude: f32, seconds: u32) {
        let rate = 8_000_u32;
        let samples = rate * seconds;
        let value = (amplitude * 32_768.0) as i16;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1_u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(samples * 2).to_le_bytes());
        for index in 0..samples {
            let sample = if index % 2 == 0 { value } else { -value };
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(path, bytes).unwrap();
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lma-audio-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn stem(name: &str, path: PathBuf) -> StemInput {
        StemInput {
            stem: name.to_owned(),
            path,
        }
    }

    #[test]
    fn stem_mix_waveform_combines_every_stem() {
        let dir = scratch_dir("mix");
        write_square_wav(&dir.join("vocals.wav"), 0.25, 1);
        write_square_wav(&dir.join("drums.wav"), 0.5, 1);

        let prepared = prepare_stem_mix(vec![
            stem("vocals", dir.join("vocals.wav")),
            stem("drums", dir.join("drums.wav")),
        ])
        .unwrap();

        assert!(
            prepared
                .waveform
                .max
                .iter()
                .all(|peak| (peak - 0.75).abs() < 1e-3)
        );
        assert!(
            prepared
                .waveform
                .min
                .iter()
                .all(|peak| (peak + 0.75).abs() < 1e-3)
        );
        assert_eq!(prepared.sources.len(), 2);
        assert!(
            prepared
                .sources
                .iter()
                .all(|source| source.waveform.is_some())
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn waveform_resolution_follows_duration_within_bounds() {
        assert_eq!(waveform_windows(Duration::from_secs(10)), 1_200);
        assert_eq!(waveform_windows(Duration::from_secs(300)), 6_000);
        assert_eq!(waveform_windows(Duration::from_secs(3 * 3_600)), 40_000);
    }

    #[test]
    fn polling_snapshots_omit_the_waveform_but_stems_keep_their_own() {
        let peaks = |value: f32| WaveformPeaks {
            sample_windows: 1,
            min: vec![-value],
            max: vec![value],
        };
        let mut engine = AudioEngine::new();
        engine.track = Some(LoadedTrack {
            sources: ["vocals", "drums"]
                .into_iter()
                .enumerate()
                .map(|(index, stem)| LoadedSource {
                    stem: Some(stem.to_owned()),
                    path: PathBuf::from(format!("{stem}.wav")),
                    waveform: Some(peaks(0.25 * (index + 1) as f32)),
                    volume: 1.0,
                    muted: false,
                    solo: false,
                })
                .collect(),
            duration: Duration::from_secs(1),
            waveform: peaks(0.75),
            parked_position: Duration::ZERO,
        });

        assert!(engine.snapshot().waveform.is_none());
        assert!(engine.snapshot_with_waveform().waveform.is_some());
        let stems = engine.stem_waveforms();
        assert_eq!(
            stems.iter().map(|s| s.stem.as_str()).collect::<Vec<_>>(),
            ["vocals", "drums"]
        );
        assert_eq!(stems[1].waveform.max, vec![0.5]);
    }

    #[test]
    fn stem_mix_rejects_stems_of_different_length() {
        let dir = scratch_dir("length");
        write_square_wav(&dir.join("vocals.wav"), 0.25, 1);
        write_square_wav(&dir.join("drums.wav"), 0.25, 2);

        let error = prepare_stem_mix(vec![
            stem("vocals", dir.join("vocals.wav")),
            stem("drums", dir.join("drums.wav")),
        ])
        .err()
        .unwrap();

        assert!(matches!(error, AudioError::UnsynchronizedStem { stem } if stem == "drums"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn stem_mix_names_the_stem_that_cannot_be_decoded() {
        let dir = scratch_dir("missing");
        write_square_wav(&dir.join("vocals.wav"), 0.25, 1);

        let error = prepare_stem_mix(vec![
            stem("vocals", dir.join("vocals.wav")),
            stem("bass", dir.join("bass.wav")),
        ])
        .err()
        .unwrap();

        assert!(matches!(error, AudioError::InvalidStem { stem } if stem == "bass"));
        std::fs::remove_dir_all(dir).ok();
    }

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
                    waveform: None,
                    volume: 1.0,
                    muted: false,
                    solo: false,
                },
                LoadedSource {
                    stem: Some("drums".to_owned()),
                    path: PathBuf::from("drums.wav"),
                    waveform: None,
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
