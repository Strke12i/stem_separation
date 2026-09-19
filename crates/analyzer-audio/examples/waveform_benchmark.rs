//! Waveform peak extraction benchmark (B4) and the Fase 13 SIMD experiment.
//!
//! Run in release mode; a debug build says nothing about vectorization:
//!
//! ```text
//! cargo run --release -p analyzer-audio --example waveform_benchmark
//! cargo run --release -p analyzer-audio --example waveform_benchmark -- path/to/track.mp3
//! ```
//!
//! With an audio path it also times a decode-only pass and the streaming peak
//! reduction that `AudioEngine` really performs on load, so the reduction cost
//! can be read against the decode cost that dominates it.

use analyzer_audio::waveform_from_samples;
use rodio::{Decoder, Source};
use std::fs::File;
use std::hint::black_box;
use std::time::Instant;

const RUNS: usize = 7;
const LANES: usize = 8;

fn main() {
    let samples = synthetic_stereo(44_100 * 2 * 300);
    let window = samples.len().div_ceil(1_200);

    let production = median_ms(|| {
        black_box(waveform_from_samples(black_box(&samples), window));
    });
    let lanes = median_ms(|| {
        black_box(lane_peaks(black_box(&samples), window));
    });
    let summed = median_ms(|| {
        black_box(sum_check_peaks(black_box(&samples), window));
    });

    let expected = waveform_from_samples(&samples, window);
    for (name, (min, max)) in [
        ("lane_peaks", lane_peaks(&samples, window)),
        ("sum_check_peaks", sum_check_peaks(&samples, window)),
    ] {
        assert_eq!(expected.min, min, "{name} diverged (min)");
        assert_eq!(expected.max, max, "{name} diverged (max)");
    }

    println!(
        "stage=waveform duration_s=300 samples={} windows={}",
        samples.len(),
        expected.min.len()
    );
    println!("production (scalar)   median_ms={production:.1}");
    println!(
        "lane accumulators x{LANES}  median_ms={lanes:.1}  speedup={:.2}x",
        production / lanes
    );
    println!(
        "lanes + sum check     median_ms={summed:.1}  speedup={:.2}x",
        production / summed
    );

    if let Some(path) = std::env::args().nth(1) {
        decode_context(&path);
    }
}

/// Deterministic music-like signal in [-1, 1]: two tones plus LCG noise.
fn synthetic_stereo(len: usize) -> Vec<f32> {
    let mut state = 0x2545_f491_u32;
    (0..len)
        .map(|i| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 8) as f32 / (1 << 24) as f32 - 0.5;
            let t = (i / 2) as f32 / 44_100.0;
            0.4 * (t * 440.0 * std::f32::consts::TAU).sin()
                + 0.2 * (t * 1_320.0 * std::f32::consts::TAU).sin()
                + 0.2 * noise
        })
        .collect()
}

/// Same contract as `waveform_from_samples`, with independent min/max lanes the
/// compiler can vectorize. Windows holding a non-finite sample fall back to the
/// scalar rule (non-finite counts as silence) so the results stay identical.
fn lane_peaks(samples: &[f32], window: usize) -> (Vec<f32>, Vec<f32>) {
    let mut min = Vec::with_capacity(samples.len().div_ceil(window));
    let mut max = Vec::with_capacity(min.capacity());
    for chunk in samples.chunks(window) {
        let mut low = [1.0_f32; LANES];
        let mut high = [-1.0_f32; LANES];
        let mut non_finite = false;
        let mut blocks = chunk.chunks_exact(LANES);
        for block in &mut blocks {
            for lane in 0..LANES {
                let sample = block[lane];
                non_finite |= !sample.is_finite();
                if sample < low[lane] {
                    low[lane] = sample;
                }
                if sample > high[lane] {
                    high[lane] = sample;
                }
            }
        }
        let mut window_low = low.iter().copied().fold(1.0_f32, f32::min);
        let mut window_high = high.iter().copied().fold(-1.0_f32, f32::max);
        for &sample in blocks.remainder() {
            non_finite |= !sample.is_finite();
            window_low = window_low.min(sample);
            window_high = window_high.max(sample);
        }
        if non_finite {
            window_low = 1.0;
            window_high = -1.0;
            for &sample in chunk {
                let finite = if sample.is_finite() { sample } else { 0.0 };
                window_low = window_low.min(finite);
                window_high = window_high.max(finite);
            }
        }
        min.push(window_low);
        max.push(window_high);
    }
    (min, max)
}

/// Like `lane_peaks`, but keeps the hot loop free of per-sample finite tests:
/// comparisons ignore NaN and a running sum propagates NaN/inf, so a non-finite
/// sum flags the window for the scalar fallback.
fn sum_check_peaks(samples: &[f32], window: usize) -> (Vec<f32>, Vec<f32>) {
    let mut min = Vec::with_capacity(samples.len().div_ceil(window));
    let mut max = Vec::with_capacity(min.capacity());
    for chunk in samples.chunks(window) {
        let mut low = [1.0_f32; LANES];
        let mut high = [-1.0_f32; LANES];
        let mut sum = [0.0_f32; LANES];
        let mut blocks = chunk.chunks_exact(LANES);
        for block in &mut blocks {
            for lane in 0..LANES {
                let sample = block[lane];
                sum[lane] += sample;
                if sample < low[lane] {
                    low[lane] = sample;
                }
                if sample > high[lane] {
                    high[lane] = sample;
                }
            }
        }
        let mut window_low = low.iter().copied().fold(1.0_f32, f32::min);
        let mut window_high = high.iter().copied().fold(-1.0_f32, f32::max);
        let mut total: f32 = sum.iter().sum();
        for &sample in blocks.remainder() {
            total += sample;
            window_low = window_low.min(sample);
            window_high = window_high.max(sample);
        }
        if !total.is_finite() {
            window_low = 1.0;
            window_high = -1.0;
            for &sample in chunk {
                let finite = if sample.is_finite() { sample } else { 0.0 };
                window_low = window_low.min(finite);
                window_high = window_high.max(finite);
            }
        }
        min.push(window_low);
        max.push(window_high);
    }
    (min, max)
}

/// Decode-only versus decode plus the streaming reduction used on track load.
fn decode_context(path: &str) {
    let open = || Decoder::try_from(File::open(path).expect("audio file")).expect("decodable");
    let decode = median_ms(|| {
        let mut acc = 0.0_f32;
        for sample in open() {
            acc += sample;
        }
        black_box(acc);
    });
    let streaming = median_ms(|| {
        let decoder = open();
        let total = decoder.total_duration().expect("duration").as_secs_f64()
            * f64::from(decoder.sample_rate().get())
            * f64::from(decoder.channels().get());
        let window = (total / 1_200.0).ceil().max(1.0) as usize;
        let (mut count, mut low, mut high) = (0_usize, 1.0_f32, -1.0_f32);
        let (mut min, mut max) = (Vec::new(), Vec::new());
        for sample in decoder {
            let finite = if sample.is_finite() { sample } else { 0.0 };
            low = low.min(finite);
            high = high.max(finite);
            count += 1;
            if count == window {
                min.push(low);
                max.push(high);
                (count, low, high) = (0, 1.0, -1.0);
            }
        }
        black_box((min, max));
    });
    println!("decode only           median_ms={decode:.1}");
    println!(
        "decode + peaks        median_ms={streaming:.1}  peaks_share={:.1}%",
        (streaming - decode) / streaming * 100.0
    );
}

fn median_ms(mut work: impl FnMut()) -> f64 {
    let mut times: Vec<f64> = (0..RUNS)
        .map(|_| {
            let started = Instant::now();
            work();
            started.elapsed().as_secs_f64() * 1_000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[RUNS / 2]
}
