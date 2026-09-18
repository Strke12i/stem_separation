//! Canonical, dependency-light types owned by the Rust desktop host.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::{Display, Formatter};
use uuid::Uuid;

macro_rules! identifier {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(format!("{}{}", $prefix, Uuid::new_v4()))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

identifier!(JobId, "job-");
identifier!(RequestId, "req-");
identifier!(TrackId, "track-");

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Created,
    Queued,
    Preparing,
    Running,
    Finalizing,
    Completed,
    CompletedWithWarnings,
    Cancelled,
    Failed,
}

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct TrackManifest {
    pub schema_version: u32,
    pub track_id: TrackId,
    pub source: SourceMetadata,
    #[serde(default)]
    pub analysis: serde_json::Map<String, Value>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub stages: Vec<StageRecord>,
    #[serde(default)]
    pub jobs: Vec<JobRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SourceMetadata {
    pub sha256: String,
    pub original_name: String,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Artifact {
    pub artifact_id: String,
    pub kind: ArtifactKind,
    pub relative_path: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stem: Option<StemKind>,
    pub created_by: CreatedBy,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Source,
    NormalizedSource,
    Stem,
    Analysis,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StemKind {
    Vocals,
    Drums,
    Bass,
    Other,
    Guitar,
    Piano,
}

impl StemKind {
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Vocals => "vocals",
            Self::Drums => "drums",
            Self::Bass => "bass",
            Self::Other => "other",
            Self::Guitar => "guitar",
            Self::Piano => "piano",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreatedBy {
    pub stage: String,
    pub engine: String,
    pub engine_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct StageRecord {
    pub name: String,
    pub status: JobStatus,
    pub cache_hit: bool,
    pub started_at: String,
    pub finished_at: String,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct JobRecord {
    pub job_id: JobId,
    pub status: JobStatus,
}

#[cfg(test)]
mod tests {
    use super::RequestId;

    #[test]
    fn generated_ids_are_prefixed_and_unique() {
        let first = RequestId::new();
        let second = RequestId::new();

        assert!(first.as_str().starts_with("req-"));
        assert_ne!(first, second);
    }
}
