//! Rust-owned interactive audio playback and waveform extraction.

mod engine;
mod waveform;

pub use engine::{AudioEngine, AudioError, AudioState, PlaybackStatus, StemInput, StemState};
pub use waveform::{WaveformPeaks, waveform_from_samples};
