//! Where an EMP's copy of the EVSE world lives.

use std::collections::BTreeMap;

use crate::emp::PullEvseDataRecord;
use crate::types::{DateTime, EvseId};

/// What one application of a delta did.
///
/// Worth logging: `updated_unknown` and `deleted_unknown` are how you find out that the local copy
/// had drifted from Hubject's before you noticed any other symptom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApplyOutcome {
    /// Records that were not there before.
    pub inserted: u64,
    /// Records that replaced an existing one.
    pub updated: u64,
    /// Records Hubject called updates that were not there — the local copy had drifted.
    pub updated_unknown: u64,
    /// Records that were removed.
    pub deleted: u64,
    /// Deletions for records that were not there — the local copy had drifted, or the same delta
    /// was applied twice.
    pub deleted_unknown: u64,
}

impl ApplyOutcome {
    /// How many records the delta touched.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.inserted + self.updated + self.updated_unknown + self.deleted + self.deleted_unknown
    }

    /// Whether anything suggests the local copy had drifted from Hubject's.
    ///
    /// Not an error on its own — a `deleted_unknown` happens naturally when the same delta is
    /// applied twice — but a rising count means the watermark logic is wrong somewhere.
    #[must_use]
    pub const fn suggests_drift(&self) -> bool {
        self.updated_unknown > 0 || self.deleted_unknown > 0
    }

    /// Adds another outcome, for accumulating across pages.
    pub fn merge(&mut self, other: Self) {
        self.inserted += other.inserted;
        self.updated += other.updated;
        self.updated_unknown += other.updated_unknown;
        self.deleted += other.deleted;
        self.deleted_unknown += other.deleted_unknown;
    }
}

/// Somewhere to keep an EMP's copy of the charging points it can route to.
///
/// Implement this over your database. The engine needs four operations and no transactions: a
/// delta is idempotent, so a crash mid-page costs a repeat, not a corruption — as long as the
/// watermark is only advanced by [`set_last_call`](Self::set_last_call) *after* the crawl
/// finishes. [`Planner`](super::Planner) makes that ordering hard to get wrong.
pub trait EvseRepository {
    /// What can go wrong in the store.
    type Error;

    /// Stores `record`, replacing any record with the same `EvseID`.
    ///
    /// Returns `true` when the record was new.
    ///
    /// # Errors
    ///
    /// Whatever the store returns.
    fn upsert(&mut self, record: PullEvseDataRecord) -> Result<bool, Self::Error>;

    /// Removes the record for `evse_id`.
    ///
    /// Returns `true` when there was one.
    ///
    /// # Errors
    ///
    /// Whatever the store returns.
    fn delete(&mut self, evse_id: &EvseId) -> Result<bool, Self::Error>;

    /// The record for `evse_id`, if any.
    ///
    /// # Errors
    ///
    /// Whatever the store returns.
    fn get(&self, evse_id: &EvseId) -> Result<Option<PullEvseDataRecord>, Self::Error>;

    /// How many records are stored.
    ///
    /// # Errors
    ///
    /// Whatever the store returns.
    fn len(&self) -> Result<u64, Self::Error>;

    /// Whether the store is empty — which is what makes the planner ask for a full pull.
    ///
    /// # Errors
    ///
    /// Whatever the store returns.
    fn is_empty(&self) -> Result<bool, Self::Error> {
        Ok(self.len()? == 0)
    }

    /// The watermark: when the last **successful** crawl finished.
    ///
    /// `None` means nothing has been pulled yet.
    ///
    /// # Errors
    ///
    /// Whatever the store returns.
    fn last_call(&self) -> Result<Option<DateTime>, Self::Error>;

    /// Advances the watermark.
    ///
    /// Call this **only** after every page of a crawl has been applied. Advancing it early loses
    /// the unapplied changes permanently, because the next delta starts after them.
    ///
    /// # Errors
    ///
    /// Whatever the store returns.
    fn set_last_call(&mut self, at: DateTime) -> Result<(), Self::Error>;

    /// Removes every record, for a re-baseline.
    ///
    /// # Errors
    ///
    /// Whatever the store returns.
    fn clear(&mut self) -> Result<(), Self::Error>;
}

/// The error type of [`InMemoryEvseRepository`], which cannot fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the in-memory repository cannot fail")]
pub enum RepositoryError {}

/// An [`EvseRepository`] in a `BTreeMap`.
///
/// For tests, for the CLI, and for an EMP small enough that its charge points fit in memory.
/// Keyed by [`EvseId`] itself, whose ordering ignores case, the optional separators and the
/// optional DIN `+` — so `DE*ABC*E1` and `DEABCE1` are one charging point, which is what they are,
/// and no key has to be built to find out.
#[derive(Clone, Debug, Default)]
pub struct InMemoryEvseRepository {
    records: BTreeMap<EvseId, PullEvseDataRecord>,
    last_call: Option<DateTime>,
}

impl InMemoryEvseRepository {
    /// An empty repository.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every record, in `EvseID` order.
    pub fn iter(&self) -> impl Iterator<Item = &PullEvseDataRecord> {
        self.records.values()
    }

    /// Every record, consuming the repository.
    #[must_use]
    pub fn into_records(self) -> Vec<PullEvseDataRecord> {
        self.records.into_values().collect()
    }

    /// A snapshot of the `EvseID`s held, in order, for comparing two repositories.
    #[must_use]
    pub fn keys(&self) -> Vec<&EvseId> {
        self.records.keys().collect()
    }
}

impl EvseRepository for InMemoryEvseRepository {
    type Error = RepositoryError;

    fn upsert(&mut self, record: PullEvseDataRecord) -> Result<bool, Self::Error> {
        Ok(self.records.insert(record.evse_id.clone(), record).is_none())
    }

    fn delete(&mut self, evse_id: &EvseId) -> Result<bool, Self::Error> {
        Ok(self.records.remove(evse_id).is_some())
    }

    fn get(&self, evse_id: &EvseId) -> Result<Option<PullEvseDataRecord>, Self::Error> {
        Ok(self.records.get(evse_id).cloned())
    }

    fn len(&self) -> Result<u64, Self::Error> {
        Ok(self.records.len() as u64)
    }

    fn last_call(&self) -> Result<Option<DateTime>, Self::Error> {
        Ok(self.last_call.clone())
    }

    fn set_last_call(&mut self, at: DateTime) -> Result<(), Self::Error> {
        self.last_call = Some(at);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.records.clear();
        Ok(())
    }
}

// Fixtures come from `testkit::samples`, so these tests compile when that feature is on.
#[cfg(all(test, feature = "testkit"))]
mod tests {
    use super::*;
    use crate::testkit::samples;

    #[test]
    fn the_same_charging_spot_written_two_ways_is_one_record() {
        let mut repo = InMemoryEvseRepository::new();
        let spaced = samples::pull_evse_data_record("DE*ABC*E1");
        let packed = samples::pull_evse_data_record("DEABCE1");

        assert!(repo.upsert(spaced).unwrap(), "the first is new");
        assert!(!repo.upsert(packed).unwrap(), "the second is the same charging spot");
        assert_eq!(repo.len().unwrap(), 1);
    }

    #[test]
    fn reading_the_whole_copy_back_yields_the_whole_copy() {
        // `iter`, `keys` and `into_records` are the three ways out of the repository, and each
        // returns *everything*. An empty answer from any of them looks exactly like an EMP with no
        // charging points, which is a state that also happens for real.
        let mut repository = InMemoryEvseRepository::new();
        repository.upsert(samples::pull_evse_data_record("DE*ABC*E2")).unwrap();
        repository.upsert(samples::pull_evse_data_record("DE*ABC*E1")).unwrap();

        let keys: Vec<String> = repository.keys().into_iter().map(EvseId::canonical).collect();
        assert_eq!(keys, vec!["DEABCE1".to_owned(), "DEABCE2".to_owned()], "in EvseID order");

        let iterated: Vec<String> = repository.iter().map(|r| r.evse_id.canonical()).collect();
        assert_eq!(iterated, keys);

        let records = repository.into_records();
        assert_eq!(records.len(), 2);
        assert_eq!(records.iter().map(|r| r.evse_id.canonical()).collect::<Vec<_>>(), keys);
    }

    #[test]
    fn an_outcome_counts_drift_separately_from_normal_work() {
        let mut outcome = ApplyOutcome { inserted: 10, updated: 5, ..ApplyOutcome::default() };
        assert!(!outcome.suggests_drift());
        assert_eq!(outcome.total(), 15);

        outcome.merge(ApplyOutcome { deleted_unknown: 1, ..ApplyOutcome::default() });
        assert!(outcome.suggests_drift());
        assert_eq!(outcome.total(), 16);
    }

    #[test]
    fn the_arithmetic_holds_with_every_counter_in_play() {
        // `ApplyOutcome` is the line an operator reads after a crawl. Each field gets a distinct
        // non-zero value, because a sum checked with four zeroes in it agrees with almost any
        // arithmetic — and a total that is quietly a product or a difference reads as a calm night.
        let one =
            ApplyOutcome { inserted: 1, updated: 2, updated_unknown: 4, deleted: 8, deleted_unknown: 16 };
        assert_eq!(one.total(), 31);

        let two = ApplyOutcome {
            inserted: 32,
            updated: 64,
            updated_unknown: 128,
            deleted: 256,
            deleted_unknown: 512,
        };
        let mut merged = one;
        merged.merge(two);
        assert_eq!(merged.inserted, 33);
        assert_eq!(merged.updated, 66);
        assert_eq!(merged.updated_unknown, 132);
        assert_eq!(merged.deleted, 264);
        assert_eq!(merged.deleted_unknown, 528);
        assert_eq!(merged.total(), 1023);
    }

    #[test]
    fn either_kind_of_drift_is_drift_on_its_own() {
        // Two conditions joined by `||`, so each needs its own case: a test that only ever trips
        // the second leaves the first free to be inverted.
        let updated = ApplyOutcome { updated_unknown: 1, ..ApplyOutcome::default() };
        assert!(updated.suggests_drift(), "an update for a record we never had is drift");

        let deleted = ApplyOutcome { deleted_unknown: 1, ..ApplyOutcome::default() };
        assert!(deleted.suggests_drift(), "a delete for a record we never had is drift");

        let ordinary = ApplyOutcome { inserted: 99, updated: 99, deleted: 99, ..ApplyOutcome::default() };
        assert!(!ordinary.suggests_drift(), "ordinary work is not drift");
        assert!(!ApplyOutcome::default().suggests_drift());
    }
}
