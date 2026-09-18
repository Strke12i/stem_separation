use analyzer_audio::waveform_from_samples;
use std::time::Instant;

fn main() {
    let samples = vec![0.25_f32; 44_100 * 2 * 300];
    let started = Instant::now();
    let peaks = waveform_from_samples(&samples, samples.len().div_ceil(1_200));
    let elapsed = started.elapsed();
    println!(
        "stage=waveform duration_s=300 windows={} wall_ms={}",
        peaks.min.len(),
        elapsed.as_millis()
    );
}
