//! Any sequence of deltas must leave the repository in a consistent state, and never panic.
//!
//! `tests/properties.rs` proves convergence for generated sequences. This looks for the inputs a
//! generator would not think of: a delete for a record that was never inserted, an unknown
//! `deltaType`, the same record twice on one page.

#![no_main]

use libfuzzer_sys::fuzz_target;
use oicp_kit::emp::PullEvseDataRecord;
use oicp_kit::sync::{self, InMemoryEvseRepository};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    // A page of records, as a peer would send it.
    let Ok(records) = serde_json::from_str::<Vec<PullEvseDataRecord>>(text) else { return };
    if records.len() > 500 {
        return;
    }

    let mut repository = InMemoryEvseRepository::new();
    let outcome = sync::apply(&mut repository, records.clone()).expect("in-memory apply cannot fail");

    // Every record was accounted for exactly once.
    assert_eq!(outcome.total(), records.len() as u64);

    // Applying the same page again is idempotent in what it leaves behind, so a retried page is
    // harmless — the property a crawl that loses its connection depends on.
    let before = repository.keys().into_iter().cloned().collect::<Vec<_>>();
    sync::apply(&mut repository, records).expect("in-memory apply cannot fail");
    let after = repository.keys().into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(before, after, "applying a page twice changed the world");

    // Nothing stored carries the tag that describes the pull rather than the charging point.
    for record in repository.iter() {
        assert!(record.delta_type.is_none(), "deltaType must not be stored");
    }
});
