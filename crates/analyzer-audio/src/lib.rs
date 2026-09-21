//! Rust-owned interactive audio playback and waveform extraction.

mod engine;
mod waveform;

pub use engine::{
    AudioEngine, AudioError, AudioState, PlaybackStatus, PreparedTrack, StemInput, StemState,
    StemWaveform, prepare_stem_mix, prepare_track,
};
pub use waveform::{WaveformPeaks, mix_waveforms, waveform_from_samples};
