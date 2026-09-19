//! Fase 13 lab: can a Rust extension speed up the analysis worker's hot paths?
//!
//! The only post-processing that stays in pure Python after the NumPy
//! vectorization (see `harmony.py`) is `smooth`, so that is the candidate.
//! `smooth_labels` reproduces its contract exactly: majority label in a
//! `2 * width + 1` window, ties broken by the earliest position in that window.

use pyo3::prelude::*;
use std::collections::HashMap;

#[pyfunction]
fn smooth_labels(labels: Vec<String>, width: usize) -> Vec<String> {
    let mut ids: HashMap<&str, usize> = HashMap::new();
    let mut names: Vec<&str> = Vec::new();
    let encoded: Vec<usize> = labels
        .iter()
        .map(|label| {
            *ids.entry(label.as_str()).or_insert_with(|| {
                names.push(label.as_str());
                names.len() - 1
            })
        })
        .collect();

    let mut counts = vec![0_u32; names.len()];
    let mut first = vec![0_usize; names.len()];
    let mut touched: Vec<usize> = Vec::with_capacity(2 * width + 1);
    let mut smoothed = Vec::with_capacity(encoded.len());

    for index in 0..encoded.len() {
        let begin = index.saturating_sub(width);
        let end = (index + width + 1).min(encoded.len());
        for (offset, &id) in encoded[begin..end].iter().enumerate() {
            if counts[id] == 0 {
                first[id] = offset;
                touched.push(id);
            }
            counts[id] += 1;
        }
        let best = touched
            .iter()
            .copied()
            .min_by_key(|&id| (std::cmp::Reverse(counts[id]), first[id]))
            .expect("window is never empty");
        smoothed.push(names[best].to_owned());
        for id in touched.drain(..) {
            counts[id] = 0;
        }
    }
    smoothed
}

#[pymodule]
fn analyzer_native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(smooth_labels, module)?)?;
    Ok(())
}
