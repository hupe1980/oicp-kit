//! Keeping two copies of the EVSE world in step — the delta engine, and its mirror image.
//!
//! # The problem every EMP solves badly
//!
//! An EMP needs a local copy of every charging point it can route a driver to. Pulling all of them
//! takes minutes and hundreds of megabytes, so nobody does it often. The spec's answer is the
//! `LastCall` delta: ask for the changes since a given instant, and Hubject tags every record
//! `insert`, `update` or `delete`.
//!
//! That sounds simple and is not, because of four rules that are easy to get wrong and expensive
//! to get wrong quietly:
//!
//! 1. **`LastCall` is exclusive with the filters.** A delta scoped to a country silently omits
//!    charge points that *moved out* of it, and the stale records live forever.
//! 2. **A delete is a tombstone, not an update.** A `delete` record carries an `EvseID` and little
//!    else; applying it as an upsert leaves a corrupted record behind.
//! 3. **The watermark advances on success, not on request.** Advance it before the crawl finishes
//!    and a failed page's changes are lost — permanently, because the next delta starts after them.
//! 4. **Deltas expire.** Hubject has no documented retention for delta history. After a long
//!    outage a delta cannot be trusted and the copy must be rebuilt.
//!
//! [`Planner`] encodes all four. [`apply`] applies a delta to an [`EvseRepository`]. The property
//! test in `tests/properties.rs` proves the thing that matters:
//!
//! > applying any sequence of deltas leaves the same state as a full pull.
//!
//! # And the same problem, from the CPO side
//!
//! A CPO has the mirror image: it must keep Hubject's copy of *its* fleet in step, and its tool is
//! [`ActionType`](crate::types::ActionType) — where `fullLoad` replaces everything. Sending a
//! partial list with `fullLoad` removes the rest of the operator's fleet from the roaming network.
//! [`PushPlanner`] computes the minimal `insert`/`update`/`delete` set from a snapshot, so the
//! destructive action is never the one you reach for by default.

mod file;
mod planner;
mod push;
mod repository;

pub use file::{FileEvseRepository, FileRepositoryError};
pub use planner::{FullPullReason, Plan, Planner, PlannerConfig};
pub use push::{PushPlan, PushPlanner};
pub use repository::{ApplyOutcome, EvseRepository, InMemoryEvseRepository, RepositoryError};

use crate::cpo::DeltaType;
use crate::emp::PullEvseDataRecord;

/// Applies one page of a delta pull to `repository`.
///
/// Records without a [`DeltaType`] are treated as inserts-or-updates: that is what a *full* pull
/// returns, so the same function applies a full pull and a delta.
///
/// # `deltaType` is stripped before storing
///
/// `deltaType` describes *this pull* — "here is what changed since your last call" — not the
/// charging point. Storing it would leave every record in the EMP's copy tagged with whatever
/// happened to it the last time it appeared in a delta, so a copy built from deltas would never
/// equal one built from a full pull, and any re-publication of the record would carry a
/// meaningless tag. `lastUpdate` is kept: that *is* a fact about the charging point.
///
/// The convergence property in `tests/properties.rs` is what pins this down.
///
/// # Errors
///
/// Returns whatever the repository returns. The outcome counts what was done, so a caller can log
/// "412 inserted, 88 updated, 3 deleted" rather than "ok".
pub fn apply<R: EvseRepository>(
    repository: &mut R,
    records: impl IntoIterator<Item = PullEvseDataRecord>,
) -> Result<ApplyOutcome, R::Error> {
    let mut outcome = ApplyOutcome::default();
    for mut record in records {
        let delta_type = record.delta_type.take();
        match delta_type {
            // A delete carries an EvseID and nothing worth keeping; storing it would leave a
            // half-empty record behind that looks like a real charging point.
            Some(DeltaType::Delete) => {
                if repository.delete(&record.evse_id)? {
                    outcome.deleted += 1;
                } else {
                    outcome.deleted_unknown += 1;
                }
            }
            Some(DeltaType::Update) => {
                if repository.upsert(record)? {
                    // Hubject called it an update, but we had never seen it. Keeping it is right —
                    // the alternative is a charge point the EMP can never route to — but it means
                    // the local copy had drifted, which is worth surfacing.
                    outcome.updated_unknown += 1;
                } else {
                    outcome.updated += 1;
                }
            }
            // An insert, a full pull, or a record Hubject did not tag: all of them mean
            // "this is the record", and whether we had it already is the outcome, not the input.
            None | Some(DeltaType::Insert | DeltaType::Custom(_)) => {
                if repository.upsert(record)? {
                    outcome.inserted += 1;
                } else {
                    outcome.updated += 1;
                }
            }
        }
    }
    Ok(outcome)
}
