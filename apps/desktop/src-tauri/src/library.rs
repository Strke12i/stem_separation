//! Rust-owned, filesystem-derived SQLite index over imported tracks.
//!
//! The database at `<workspace_root>/library/index.sqlite3` is a disposable
//! derived cache: everything it stores can be recomputed by rescanning each
//! track's `manifest.json` (plus a small per-track `library.json` sidecar for
//! library-only facts like tags and open history, added in a later phase).
//! Deleting the database file must never lose data; a corrupt or
//! schema-mismatched file is simply recreated empty and repopulated on the
//! next scan.

use crate::ingest::IngestService;
use analyzer_domain::{ArtifactKind, TrackManifest};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex, PoisonError};
use std::time::UNIX_EPOCH;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const LIBRARY_SCHEMA_VERSION: u32 = 1;

pub struct LibraryService {
    ingest: Arc<IngestService>,
    connection: StdMutex<Connection>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub track_id: String,
    pub original_name: String,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub source_sha256: String,
    pub bpm: Option<f64>,
    pub key_label: Option<String>,
    pub stem_models: Vec<String>,
    pub analyzed: Vec<String>,
    pub imported_at: Option<String>,
    pub last_opened_at: Option<String>,
    pub open_count: u32,
    // Populated starting with the tagging phase; always empty until then.
    pub tags: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySyncReport {
    pub indexed: usize,
    pub updated: usize,
    pub removed: usize,
    pub rebuilt: bool,
}

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("Could not read or write library files: {0}")]
    Storage(#[from] std::io::Error),
    #[error("Could not access the library index: {0}")]
    Index(#[from] rusqlite::Error),
    #[error("A track manifest is invalid: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("Could not obtain a timestamp: {0}")]
    Clock(#[from] time::error::Format),
}

impl LibraryService {
    /// Opens (creating if needed) the library index under the ingest
    /// service's workspace root. A corrupt or schema-mismatched existing
    /// file is deleted and recreated empty rather than surfaced as an
    /// error, since it holds no data that cannot be rebuilt from the
    /// tracks already on disk.
    pub fn open(ingest: Arc<IngestService>) -> Result<Self, LibraryError> {
        let directory = ingest.workspace_root().join("library");
        fs::create_dir_all(&directory)?;
        let database_path = directory.join("index.sqlite3");
        let connection = open_or_rebuild(&database_path)?;
        Ok(Self {
            ingest,
            connection: StdMutex::new(connection),
        })
    }

    /// An in-memory index used by tests and as a last-resort fallback when
    /// the on-disk database cannot be created at all (read-only volume,
    /// locked by another process). The Library tab still works for the
    /// session; only persistence across restarts is lost.
    pub fn in_memory(ingest: Arc<IngestService>) -> Result<Self, LibraryError> {
        let connection = Connection::open_in_memory()?;
        configure(&connection)?;
        apply_schema(&connection)?;
        Ok(Self {
            ingest,
            connection: StdMutex::new(connection),
        })
    }

    /// Rescans the workspace root, indexing new/changed tracks and dropping
    /// ones whose workspace directory no longer exists. Unchanged tracks
    /// (same manifest mtime and size) are skipped without re-parsing.
    pub fn reconcile(&self) -> Result<LibrarySyncReport, LibraryError> {
        self.scan(false)
    }

    /// Drops the entire index and rebuilds it from scratch. Tags and open
    /// history are unaffected: they live in each track's `library.json`
    /// sidecar, not in this database.
    pub fn rebuild(&self) -> Result<LibrarySyncReport, LibraryError> {
        {
            let connection = self
                .connection
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            connection.execute("DELETE FROM tracks", [])?;
        }
        let mut report = self.scan(true)?;
        report.rebuilt = true;
        Ok(report)
    }

    /// Re-reads one track's manifest and updates just its row, without
    /// scanning the whole workspace. Returns `Ok(None)` if the track no
    /// longer exists (and removes it from the index in that case).
    pub fn refresh(&self, track_id: &str) -> Result<Option<LibraryEntry>, LibraryError> {
        let Ok(workspace) = self.ingest.workspace_for(track_id) else {
            let connection = self
                .connection
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            connection.execute("DELETE FROM tracks WHERE track_id = ?1", [track_id])?;
            return Ok(None);
        };
        let manifest_path = workspace.join("manifest.json");
        let Ok(metadata) = fs::metadata(&manifest_path) else {
            let connection = self
                .connection
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            connection.execute("DELETE FROM tracks WHERE track_id = ?1", [track_id])?;
            return Ok(None);
        };
        let manifest: TrackManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        let summary = summarize(&manifest, &manifest_path);
        let now = now_rfc3339()?;
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        upsert_track(
            &connection,
            track_id,
            &summary,
            unix_seconds(&metadata),
            metadata.len() as i64,
            &now,
        )?;
        connection
            .query_row(
                &format!("{ENTRY_COLUMNS} FROM tracks WHERE track_id = ?1"),
                [track_id],
                row_to_entry,
            )
            .optional()
            .map_err(LibraryError::from)
    }

    /// Reconciles the index, then returns every indexed track, most
    /// recently opened first.
    pub fn list(&self) -> Result<Vec<LibraryEntry>, LibraryError> {
        self.reconcile()?;
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut statement = connection.prepare(&format!(
            "{ENTRY_COLUMNS} FROM tracks \
             ORDER BY last_opened_at IS NULL, last_opened_at DESC, imported_at DESC, original_name_folded ASC"
        ))?;
        let rows = statement.query_map([], row_to_entry)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(LibraryError::from)
    }

    fn scan(&self, force: bool) -> Result<LibrarySyncReport, LibraryError> {
        let workspace_root = self.ingest.workspace_root().to_path_buf();
        let mut seen = HashSet::new();
        let mut indexed = 0_usize;
        let mut updated = 0_usize;
        let now = now_rfc3339()?;

        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let transaction = connection.transaction()?;
        if let Ok(entries) = fs::read_dir(&workspace_root) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let Some(name) = file_name.to_str() else {
                    continue;
                };
                if !name.starts_with("track-") {
                    continue;
                }
                let manifest_path = entry.path().join("manifest.json");
                let Ok(metadata) = fs::metadata(&manifest_path) else {
                    continue;
                };
                seen.insert(name.to_owned());
                let modified_unix = unix_seconds(&metadata);
                let size_bytes = metadata.len() as i64;
                let current: Option<(i64, i64)> = transaction
                    .query_row(
                        "SELECT manifest_modified_unix, manifest_size_bytes FROM tracks WHERE track_id = ?1",
                        [name],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                if !force && current == Some((modified_unix, size_bytes)) {
                    continue;
                }
                let Ok(bytes) = fs::read(&manifest_path) else {
                    continue;
                };
                let Ok(manifest) = serde_json::from_slice::<TrackManifest>(&bytes) else {
                    tracing::warn!(
                        track = name,
                        "could not parse manifest while indexing the library; skipping"
                    );
                    continue;
                };
                let summary = summarize(&manifest, &manifest_path);
                upsert_track(
                    &transaction,
                    name,
                    &summary,
                    modified_unix,
                    size_bytes,
                    &now,
                )?;
                if current.is_some() {
                    updated += 1;
                } else {
                    indexed += 1;
                }
            }
        }

        let mut removed = 0_usize;
        let existing: Vec<String> = {
            let mut statement = transaction.prepare("SELECT track_id FROM tracks")?;
            statement
                .query_map([], |row| row.get(0))?
                .collect::<Result<_, _>>()?
        };
        for track_id in existing {
            if !seen.contains(&track_id) {
                transaction.execute("DELETE FROM tracks WHERE track_id = ?1", [&track_id])?;
                removed += 1;
            }
        }
        transaction.commit()?;
        Ok(LibrarySyncReport {
            indexed,
            updated,
            removed,
            rebuilt: false,
        })
    }
}

const ENTRY_COLUMNS: &str = "SELECT track_id, original_name, duration_seconds, sample_rate, channels, \
     source_sha256, bpm, key_label, stem_models, analyzed, imported_at, last_opened_at, open_count";

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<LibraryEntry> {
    let stem_models: String = row.get(8)?;
    let analyzed: String = row.get(9)?;
    let sample_rate: i64 = row.get(3)?;
    let channels: i64 = row.get(4)?;
    let open_count: i64 = row.get(12)?;
    Ok(LibraryEntry {
        track_id: row.get(0)?,
        original_name: row.get(1)?,
        duration_seconds: row.get(2)?,
        sample_rate: sample_rate as u32,
        channels: channels as u16,
        source_sha256: row.get(5)?,
        bpm: row.get(6)?,
        key_label: row.get(7)?,
        stem_models: split_list(&stem_models),
        analyzed: split_list(&analyzed),
        imported_at: row.get(10)?,
        last_opened_at: row.get(11)?,
        open_count: open_count as u32,
        tags: Vec::new(),
    })
}

fn split_list(value: &str) -> Vec<String> {
    if value.is_empty() {
        Vec::new()
    } else {
        value.split(',').map(str::to_owned).collect()
    }
}

struct ManifestSummary {
    original_name: String,
    source_sha256: String,
    duration_seconds: f64,
    sample_rate: u32,
    channels: u16,
    bpm: Option<f64>,
    key_label: Option<String>,
    stem_models: String,
    analyzed: String,
    imported_at: Option<String>,
}

/// Pure and tolerant: a malformed or half-written `analysis` block yields
/// `None` for the fields that read it rather than an error, so a partially
/// analyzed track is still indexed with whatever is actually available.
fn summarize(manifest: &TrackManifest, manifest_path: &Path) -> ManifestSummary {
    let bpm = manifest
        .analysis
        .get("rhythm")
        .and_then(|value| value.get("bpm"))
        .and_then(Value::as_f64);
    let key_label = manifest
        .analysis
        .get("harmony")
        .and_then(|value| value.get("key"))
        .and_then(|value| value.get("label"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut stem_models: Vec<&str> = manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.kind == ArtifactKind::Stem)
        .filter_map(|artifact| artifact.created_by.model.as_deref())
        .collect();
    stem_models.sort_unstable();
    stem_models.dedup();
    let mut analyzed: Vec<&str> = manifest.analysis.keys().map(String::as_str).collect();
    analyzed.sort_unstable();
    let imported_at = manifest
        .stages
        .iter()
        .find(|stage| stage.name == "ingest")
        .map(|stage| stage.started_at.clone())
        .or_else(|| {
            fs::metadata(manifest_path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .map(OffsetDateTime::from)
                .and_then(|time| time.format(&Rfc3339).ok())
        });
    ManifestSummary {
        original_name: manifest.source.original_name.clone(),
        source_sha256: manifest.source.sha256.clone(),
        duration_seconds: manifest.source.duration_seconds,
        sample_rate: manifest.source.sample_rate,
        channels: manifest.source.channels,
        bpm,
        key_label,
        stem_models: stem_models.join(","),
        analyzed: analyzed.join(","),
        imported_at,
    }
}

fn upsert_track(
    connection: &Connection,
    track_id: &str,
    summary: &ManifestSummary,
    modified_unix: i64,
    size_bytes: i64,
    indexed_at: &str,
) -> Result<(), LibraryError> {
    connection.execute(
        "INSERT INTO tracks (
            track_id, original_name, original_name_folded, source_sha256,
            duration_seconds, sample_rate, channels, bpm, key_label,
            stem_models, analyzed, imported_at, manifest_modified_unix,
            manifest_size_bytes, indexed_at, last_opened_at, open_count
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, NULL, 0)
        ON CONFLICT(track_id) DO UPDATE SET
            original_name = excluded.original_name,
            original_name_folded = excluded.original_name_folded,
            source_sha256 = excluded.source_sha256,
            duration_seconds = excluded.duration_seconds,
            sample_rate = excluded.sample_rate,
            channels = excluded.channels,
            bpm = excluded.bpm,
            key_label = excluded.key_label,
            stem_models = excluded.stem_models,
            analyzed = excluded.analyzed,
            imported_at = excluded.imported_at,
            manifest_modified_unix = excluded.manifest_modified_unix,
            manifest_size_bytes = excluded.manifest_size_bytes,
            indexed_at = excluded.indexed_at",
        rusqlite::params![
            track_id,
            summary.original_name,
            summary.original_name.to_lowercase(),
            summary.source_sha256,
            summary.duration_seconds,
            i64::from(summary.sample_rate),
            i64::from(summary.channels),
            summary.bpm,
            summary.key_label,
            summary.stem_models,
            summary.analyzed,
            summary.imported_at,
            modified_unix,
            size_bytes,
            indexed_at,
        ],
    )?;
    Ok(())
}

fn unix_seconds(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn now_rfc3339() -> Result<String, LibraryError> {
    Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

fn open_or_rebuild(path: &Path) -> Result<Connection, LibraryError> {
    if path.is_file() && !is_healthy(path) {
        fs::remove_file(path)?;
    }
    let connection = Connection::open(path)?;
    configure(&connection)?;
    apply_schema(&connection)?;
    Ok(connection)
}

/// A lightweight probe: the schema version must match and the `tracks` table
/// must actually exist. Anything else (not a database file, truncated, wrong
/// version) is treated as unhealthy and triggers a rebuild.
fn is_healthy(path: &Path) -> bool {
    let Ok(connection) = Connection::open(path) else {
        return false;
    };
    let version_matches = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map(|version| version as u32 == LIBRARY_SCHEMA_VERSION)
        .unwrap_or(false);
    if !version_matches {
        return false;
    }
    connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'tracks'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false)
}

fn configure(connection: &Connection) -> Result<(), LibraryError> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    // This is a derived cache, not a source of truth: a crash-torn write is
    // recovered by rebuilding from the manifests, so the extra durability of
    // `FULL` synchronization is not worth its cost here.
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn apply_schema(connection: &Connection) -> Result<(), LibraryError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS tracks (
            track_id               TEXT PRIMARY KEY,
            original_name          TEXT    NOT NULL,
            original_name_folded   TEXT    NOT NULL,
            source_sha256          TEXT    NOT NULL,
            duration_seconds       REAL    NOT NULL,
            sample_rate            INTEGER NOT NULL,
            channels               INTEGER NOT NULL,
            bpm                    REAL,
            key_label              TEXT,
            stem_models            TEXT    NOT NULL DEFAULT '',
            analyzed               TEXT    NOT NULL DEFAULT '',
            imported_at            TEXT,
            manifest_modified_unix INTEGER NOT NULL,
            manifest_size_bytes    INTEGER NOT NULL,
            indexed_at             TEXT    NOT NULL,
            last_opened_at         TEXT,
            open_count             INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS track_tags (
            track_id TEXT NOT NULL REFERENCES tracks(track_id) ON DELETE CASCADE,
            tag      TEXT NOT NULL,
            PRIMARY KEY (track_id, tag)
        );
        CREATE INDEX IF NOT EXISTS track_tags_by_tag ON track_tags(tag);
        CREATE INDEX IF NOT EXISTS tracks_by_last_opened ON tracks(last_opened_at DESC);
        CREATE INDEX IF NOT EXISTS tracks_by_name ON tracks(original_name_folded);",
    )?;
    connection.pragma_update(None, "user_version", i64::from(LIBRARY_SCHEMA_VERSION))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn track_count(connection: &Connection) -> i64 {
        connection
            .query_row("SELECT count(*) FROM tracks", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn applies_the_schema_idempotently() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("index.sqlite3");
        open_or_rebuild(&path).unwrap();
        let connection = open_or_rebuild(&path).unwrap();
        assert_eq!(track_count(&connection), 0);
    }

    #[test]
    fn recreates_the_database_when_the_schema_version_differs() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("index.sqlite3");
        open_or_rebuild(&path).unwrap();
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .pragma_update(None, "user_version", 0_i64)
                .unwrap();
        }
        let connection = open_or_rebuild(&path).unwrap();
        assert_eq!(track_count(&connection), 0);
    }

    #[test]
    fn recreates_the_database_when_the_file_is_not_a_database() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("index.sqlite3");
        fs::write(&path, b"not a database").unwrap();
        let connection = open_or_rebuild(&path).unwrap();
        assert_eq!(track_count(&connection), 0);
    }

    #[test]
    fn summarizes_manifest_analysis_into_index_fields() {
        use analyzer_domain::{SourceMetadata, TrackId};

        let mut manifest = TrackManifest {
            schema_version: 1,
            track_id: TrackId::new(),
            source: SourceMetadata {
                sha256: "hash".to_owned(),
                original_name: "Song.mp3".to_owned(),
                duration_seconds: 10.0,
                sample_rate: 44_100,
                channels: 2,
            },
            analysis: Default::default(),
            artifacts: Vec::new(),
            stages: Vec::new(),
            jobs: Vec::new(),
        };
        let summary = summarize(&manifest, Path::new("does-not-exist"));
        assert_eq!(summary.bpm, None);
        assert_eq!(summary.key_label, None);
        assert_eq!(summary.analyzed, "");

        manifest.analysis.insert(
            "rhythm".to_owned(),
            serde_json::json!({"bpm": 120.0, "beatTimes": []}),
        );
        manifest.analysis.insert(
            "harmony".to_owned(),
            serde_json::json!({"key": {"label": "C major"}, "chords": []}),
        );
        let summary = summarize(&manifest, Path::new("does-not-exist"));
        assert_eq!(summary.bpm, Some(120.0));
        assert_eq!(summary.key_label, Some("C major".to_owned()));
        assert_eq!(summary.analyzed, "harmony,rhythm");
    }
}
