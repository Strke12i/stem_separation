//! Locating a separated stem's audio inside a track workspace.

use analyzer_domain::{ArtifactKind, TrackManifest};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

/// Every stem name a separation model can produce. Analysis caches are laid
/// out by stem name, so anything outside this list is rejected before it can
/// reach a path.
pub const STEM_NAMES: [&str; 6] = ["vocals", "drums", "bass", "other", "guitar", "piano"];

#[must_use]
pub fn is_known_stem(stem: &str) -> bool {
    STEM_NAMES.contains(&stem)
}

#[derive(Debug, Error)]
pub enum StemLookupError {
    #[error("The requested {0} stem is unavailable. Generate local stems first.")]
    Missing(String),
    #[error("Could not read the stem audio: {0}")]
    Io(#[from] std::io::Error),
}

/// The newest WAV for `stem` recorded in the manifest, as a canonical path
/// inside `workspace`, together with the artifact's checksum (which keys the
/// analysis caches so a re-separation invalidates them).
pub fn stem_audio(
    workspace: &Path,
    manifest: &TrackManifest,
    stem: &str,
) -> Result<(PathBuf, String), StemLookupError> {
    let missing = || StemLookupError::Missing(stem.to_owned());
    let artifact = manifest
        .artifacts
        .iter()
        .rev()
        .find(|artifact| {
            artifact.kind == ArtifactKind::Stem
                && artifact
                    .stem
                    .as_ref()
                    .is_some_and(|kind| kind.name() == stem)
        })
        .ok_or_else(missing)?;
    let relative = Path::new(&artifact.relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(missing());
    }
    let input = workspace.join(relative).canonicalize()?;
    if !input.starts_with(workspace) || !is_wav(&input)? {
        return Err(missing());
    }
    Ok((input, artifact.sha256.clone()))
}

fn is_wav(path: &Path) -> Result<bool, std::io::Error> {
    let mut header = [0_u8; 12];
    let mut file = fs::File::open(path)?;
    if file.read_exact(&mut header).is_err() {
        return Ok(false);
    }
    Ok(&header[..4] == b"RIFF" && &header[8..12] == b"WAVE")
}

#[cfg(test)]
mod tests {
    use super::{STEM_NAMES, is_known_stem};

    #[test]
    fn only_separation_stem_names_are_known() {
        assert!(STEM_NAMES.iter().all(|name| is_known_stem(name)));
        assert!(!is_known_stem("../bass"));
        assert!(!is_known_stem(""));
        assert!(!is_known_stem("Bass"));
    }
}
