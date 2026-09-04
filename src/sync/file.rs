//! A repository that keeps an EMP's copy of the EVSE world in a file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::repository::EvseRepository;
use crate::emp::PullEvseDataRecord;
use crate::types::{DateTime, EvseId};

/// Why a file-backed repository could not do what was asked.
#[derive(Debug, thiserror::Error)]
pub enum FileRepositoryError {
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        #[source]
        source: std::io::Error,
    },
    /// The file's contents are not a snapshot this crate wrote.
    #[error("{path} is not an oicp-kit snapshot: {source}")]
    Malformed {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        #[source]
        source: serde_json::Error,
    },
}

/// The on-disk shape. Versioned, so a future change can migrate rather than guess.
#[derive(Serialize, Deserialize)]
struct Snapshot {
    /// The format version. Bumped when the shape changes incompatibly.
    version: u32,
    /// The watermark: when the last successful crawl finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_call: Option<DateTime>,
    /// The records, keyed by the canonical form of their `EvseID`.
    records: BTreeMap<String, PullEvseDataRecord>,
}

const SNAPSHOT_VERSION: u32 = 1;

/// An [`EvseRepository`] backed by a JSON file.
///
/// # What it is for
///
/// The CLI's `oicp pull`, a small EMP, and anyone who wants a crawl they can inspect with `jq`.
/// It is deliberately simple: the whole set is held in memory and written out on
/// [`save`](Self::save).
///
/// # It is not a database
///
/// Every write rewrites the file, so a fleet of hundreds of thousands of records will be slow and
/// will use memory proportional to the set. For that, implement [`EvseRepository`] over your own
/// store — it is four methods, and the delta engine needs no transactions.
///
/// # Crash safety
///
/// [`save`](Self::save) writes to a temporary file in the same directory and renames it over the
/// target, so a crash mid-write leaves the previous snapshot intact rather than a truncated one.
/// That matters more than it sounds: a truncated snapshot with a *valid* watermark would make the
/// next delta pull apply changes on top of a partial world, and the missing records would never
/// come back without a re-baseline.
#[derive(Debug, Clone)]
pub struct FileEvseRepository {
    path: PathBuf,
    records: BTreeMap<String, PullEvseDataRecord>,
    last_call: Option<DateTime>,
    dirty: bool,
}

impl FileEvseRepository {
    /// Opens `path`, or starts empty if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`FileRepositoryError`] when the file exists but cannot be read or is not a
    /// snapshot this crate wrote. A missing file is not an error — that is a first run.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, FileRepositoryError> {
        let path = path.into();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(Self { path, records: BTreeMap::new(), last_call: None, dirty: false });
        };
        let snapshot: Snapshot = serde_json::from_str(&text)
            .map_err(|source| FileRepositoryError::Malformed { path: path.clone(), source })?;
        if snapshot.version != SNAPSHOT_VERSION {
            // A version this crate does not know: refuse rather than reinterpret. Re-baselining is
            // cheap; a silently misread snapshot is not.
            return Err(FileRepositoryError::Malformed {
                path: path.clone(),
                source: serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("snapshot format version {} is not {SNAPSHOT_VERSION}", snapshot.version),
                )),
            });
        }
        Ok(Self { path, records: snapshot.records, last_call: snapshot.last_call, dirty: false })
    }

    /// Writes the snapshot out, atomically.
    ///
    /// # Errors
    ///
    /// Returns [`FileRepositoryError::Io`] when the file cannot be written or renamed.
    pub fn save(&mut self) -> Result<(), FileRepositoryError> {
        let snapshot = Snapshot {
            version: SNAPSHOT_VERSION,
            last_call: self.last_call.clone(),
            records: self.records.clone(),
        };
        let json = serde_json::to_vec_pretty(&snapshot)
            .map_err(|source| FileRepositoryError::Malformed { path: self.path.clone(), source })?;

        let directory = self.path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(directory)
            .map_err(|source| FileRepositoryError::Io { path: directory.to_path_buf(), source })?;

        // Write beside the target and rename over it: a crash mid-write must not leave a truncated
        // snapshot carrying a watermark that says it is complete.
        let temporary = self.path.with_extension("tmp");
        std::fs::write(&temporary, &json)
            .map_err(|source| FileRepositoryError::Io { path: temporary.clone(), source })?;
        std::fs::rename(&temporary, &self.path)
            .map_err(|source| FileRepositoryError::Io { path: self.path.clone(), source })?;

        self.dirty = false;
        Ok(())
    }

    /// The file this repository is backed by.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether there are changes that [`save`](Self::save) has not written yet.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Every record, in `EvseID` order.
    pub fn iter(&self) -> impl Iterator<Item = &PullEvseDataRecord> {
        self.records.values()
    }
}

impl EvseRepository for FileEvseRepository {
    type Error = FileRepositoryError;

    fn upsert(&mut self, record: PullEvseDataRecord) -> Result<bool, Self::Error> {
        self.dirty = true;
        Ok(self.records.insert(record.evse_id.canonical(), record).is_none())
    }

    fn delete(&mut self, evse_id: &EvseId) -> Result<bool, Self::Error> {
        self.dirty = true;
        Ok(self.records.remove(&evse_id.canonical()).is_some())
    }

    fn get(&self, evse_id: &EvseId) -> Result<Option<PullEvseDataRecord>, Self::Error> {
        Ok(self.records.get(&evse_id.canonical()).cloned())
    }

    fn len(&self) -> Result<u64, Self::Error> {
        Ok(self.records.len() as u64)
    }

    fn last_call(&self) -> Result<Option<DateTime>, Self::Error> {
        Ok(self.last_call.clone())
    }

    fn set_last_call(&mut self, at: DateTime) -> Result<(), Self::Error> {
        self.dirty = true;
        self.last_call = Some(at);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.dirty = true;
        self.records.clear();
        Ok(())
    }
}

// The tests here build fixtures with `testkit::samples`, so they compile when that feature
// is on. Without the gate `cargo test --features sync` fails to build.
#[cfg(all(test, feature = "testkit"))]
mod tests {
    use super::*;
    use crate::testkit::samples;

    fn temporary() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("oicp-kit-test-{}-{:?}.json", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn a_missing_file_is_a_first_run_not_an_error() {
        let path = temporary();
        let repository = FileEvseRepository::open(&path).expect("a missing file starts empty");
        assert_eq!(repository.len().unwrap(), 0);
        assert_eq!(repository.last_call().unwrap(), None);
        assert!(!repository.is_dirty());
    }

    #[test]
    fn iterating_yields_what_was_stored() {
        // `iter` is how a caller reads a snapshot back — the CLI's `pull` prints from it. An
        // iterator that yields nothing looks exactly like an empty fleet.
        let path = temporary();
        let mut repository = FileEvseRepository::open(&path).unwrap();
        repository.upsert(samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
        repository.upsert(samples::pull_evse_data_record("DE*ABC*E2")).unwrap();

        let seen: Vec<String> = repository.iter().map(|r| r.evse_id.canonical()).collect();
        assert_eq!(seen, vec!["DEABCE1".to_owned(), "DEABCE2".to_owned()], "in EvseID order");
        assert_eq!(seen.len() as u64, repository.len().unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn deleting_and_clearing_survive_a_reopen() {
        // A delta's tombstones and a re-baseline both go through these two, and the CLI's snapshot
        // is where they land. A `delete` that quietly does nothing leaves withdrawn charging
        // points on an EMP's map forever, and the file it reopens looks perfectly healthy.
        let path = temporary();
        let mut repository = FileEvseRepository::open(&path).unwrap();
        repository.upsert(samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
        repository.upsert(samples::pull_evse_data_record("DE*ABC*E2")).unwrap();
        repository.save().unwrap();

        let gone: EvseId = "DE*ABC*E1".parse().unwrap();
        assert!(repository.delete(&gone).unwrap(), "the record was there");
        assert!(!repository.delete(&gone).unwrap(), "and is not there twice");
        assert_eq!(repository.len().unwrap(), 1);
        assert!(repository.get(&gone).unwrap().is_none());
        repository.save().unwrap();

        let reopened = FileEvseRepository::open(&path).unwrap();
        assert_eq!(reopened.len().unwrap(), 1, "the deletion reached the file");
        assert!(reopened.get(&gone).unwrap().is_none());

        let mut repository = FileEvseRepository::open(&path).unwrap();
        repository.clear().unwrap();
        assert_eq!(repository.len().unwrap(), 0);
        assert!(repository.is_empty().unwrap());
        repository.save().unwrap();
        assert_eq!(FileEvseRepository::open(&path).unwrap().len().unwrap(), 0);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_snapshot_survives_a_round_trip() {
        let path = temporary();
        let mut repository = FileEvseRepository::open(&path).unwrap();
        repository.upsert(samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
        repository.upsert(samples::pull_evse_data_record("DE*ABC*E2")).unwrap();
        repository.set_last_call(samples::fixed_time()).unwrap();
        assert!(repository.is_dirty());
        repository.save().unwrap();
        assert!(!repository.is_dirty());

        let reopened = FileEvseRepository::open(&path).expect("reopens");
        assert_eq!(reopened.len().unwrap(), 2);
        assert_eq!(reopened.last_call().unwrap(), Some(samples::fixed_time()));
        assert_eq!(
            reopened.get(&"DEABCE1".parse().unwrap()).unwrap().map(|r| r.evse_id.canonical()),
            Some("DEABCE1".to_owned()),
            "and the identifier still matches however it is written"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_file_that_is_not_a_snapshot_is_refused_rather_than_reinterpreted() {
        let path = temporary();
        std::fs::write(&path, b"{\"not\": \"a snapshot\"}").unwrap();
        let error = FileEvseRepository::open(&path).expect_err("refused");
        assert!(matches!(error, FileRepositoryError::Malformed { .. }));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_future_snapshot_version_is_refused_rather_than_guessed_at() {
        let path = temporary();
        std::fs::write(&path, br#"{"version": 99, "records": {}}"#).unwrap();
        let error = FileEvseRepository::open(&path).expect_err("refused");
        assert!(error.to_string().contains("version"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let path = temporary();
        let mut repository = FileEvseRepository::open(&path).unwrap();
        repository.upsert(samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
        repository.save().unwrap();
        assert!(!path.with_extension("tmp").exists(), "the temporary file was renamed, not left");
        std::fs::remove_file(&path).ok();
    }
}
