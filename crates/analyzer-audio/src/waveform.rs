use serde::Serialize;

/// UI-safe waveform summary. It intentionally excludes raw PCM samples.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveformPeaks {
    pub sample_windows: usize,
    pub min: Vec<f32>,
    pub max: Vec<f32>,
}

/// Downsamples interleaved PCM values into bounded min/max waveform peaks.
#[must_use]
pub fn waveform_from_samples(samples: &[f32], window_size: usize) -> WaveformPeaks {
    let window_size = window_size.max(1);
    let mut min = Vec::with_capacity(samples.len().div_ceil(window_size));
    let mut max = Vec::with_capacity(samples.len().div_ceil(window_size));

    for window in samples.chunks(window_size) {
        let mut low = 1.0_f32;
        let mut high = -1.0_f32;
        for &sample in window {
            let finite_sample = if sample.is_finite() { sample } else { 0.0 };
            low = low.min(finite_sample);
            high = high.max(finite_sample);
        }
        min.push(low);
        max.push(high);
    }

    WaveformPeaks {
        sample_windows: window_size,
        min,
        max,
    }
}

#[cfg(test)]
mod tests {
    use super::waveform_from_samples;

    #[test]
    fn creates_bounded_min_max_windows() {
        let peaks = waveform_from_samples(&[-0.5, 0.25, 0.8, -0.2, 0.1], 2);

        assert_eq!(peaks.sample_windows, 2);
        assert_eq!(peaks.min, vec![-0.5, -0.2, 0.1]);
        assert_eq!(peaks.max, vec![0.25, 0.8, 0.1]);
    }

    #[test]
    fn replaces_non_finite_samples_with_silence() {
        let peaks = waveform_from_samples(&[f32::NAN, f32::INFINITY], 8);

        assert_eq!(peaks.min, vec![0.0]);
        assert_eq!(peaks.max, vec![0.0]);
    }
}
