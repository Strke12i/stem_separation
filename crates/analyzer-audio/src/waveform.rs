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

/// Approximates the peaks of several stems played together from each stem's own
/// peaks. Per window the extremes add, which is exact when the stems peak at the
/// same instant and an upper bound otherwise, then clamp to full scale. Stems
/// sum to the original track, so this keeps the mix view comparable with the
/// original's waveform. The result is as long as the shortest input.
#[must_use]
pub fn mix_waveforms(parts: &[WaveformPeaks]) -> WaveformPeaks {
    let windows = parts.iter().map(|part| part.min.len()).min().unwrap_or(0);
    let sum = |select: fn(&WaveformPeaks) -> &Vec<f32>, index: usize| {
        parts
            .iter()
            .map(|part| select(part)[index])
            .sum::<f32>()
            .clamp(-1.0, 1.0)
    };
    WaveformPeaks {
        sample_windows: parts.first().map_or(1, |part| part.sample_windows),
        min: (0..windows).map(|i| sum(|part| &part.min, i)).collect(),
        max: (0..windows).map(|i| sum(|part| &part.max, i)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{WaveformPeaks, mix_waveforms, waveform_from_samples};

    fn peaks(min: &[f32], max: &[f32]) -> WaveformPeaks {
        WaveformPeaks {
            sample_windows: 4,
            min: min.to_vec(),
            max: max.to_vec(),
        }
    }

    #[test]
    fn mixing_adds_stem_peaks_and_clamps_to_full_scale() {
        let mix = mix_waveforms(&[
            peaks(&[-0.25, -0.75], &[0.5, 0.75]),
            peaks(&[-0.25, -0.5], &[0.25, 0.5]),
        ]);

        assert_eq!(mix.sample_windows, 4);
        assert_eq!(mix.min, vec![-0.5, -1.0]);
        assert_eq!(mix.max, vec![0.75, 1.0]);
    }

    #[test]
    fn mixing_stops_at_the_shortest_stem() {
        let mix = mix_waveforms(&[
            peaks(&[-0.1, -0.2, -0.3], &[0.1, 0.2, 0.3]),
            peaks(&[-0.1, -0.2], &[0.1, 0.2]),
        ]);

        assert_eq!(mix.min.len(), 2);
        assert_eq!(mix.max.len(), 2);
    }

    #[test]
    fn mixing_nothing_is_empty() {
        let mix = mix_waveforms(&[]);

        assert!(mix.min.is_empty() && mix.max.is_empty());
    }

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
