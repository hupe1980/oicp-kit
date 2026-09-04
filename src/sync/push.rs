//! The CPO's side: keeping Hubject's copy of a fleet in step without ever needing `fullLoad`.

use std::collections::BTreeMap;

use crate::cpo::{EvseDataRecord, OperatorEvseData, PushEvseDataRequest};
use crate::types::{ActionType, Extensions, OperatorId, Text};

/// The pushes needed to turn Hubject's copy of a fleet into the current one.
///
/// Ordered: **inserts and updates first, deletions last**. A charging point that is being moved
/// between stations exists throughout; the other order would blink it out of the roaming network
/// for as long as the two requests take.
#[derive(Clone, Debug, PartialEq)]
pub struct PushPlan {
    /// Records Hubject has not seen.
    pub inserts: Vec<EvseDataRecord>,
    /// Records that have changed.
    pub updates: Vec<EvseDataRecord>,
    /// Records that are no longer part of the fleet.
    pub deletes: Vec<EvseDataRecord>,
    /// Records that are unchanged, and so are in no request at all.
    pub unchanged: u64,
}

impl PushPlan {
    /// Whether anything needs sending.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inserts.is_empty() && self.updates.is_empty() && self.deletes.is_empty()
    }

    /// How many records the plan will send.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.inserts.len() + self.updates.len() + self.deletes.len()
    }

    /// The requests to send, in order.
    ///
    /// Never [`ActionType::FullLoad`]: that is what [`PushPlanner::full_load`] is for, and it has
    /// its own name so that nobody reaches it by accident.
    #[must_use]
    pub fn into_requests(
        self,
        operator_id: &OperatorId,
        operator_name: &Text<100>,
    ) -> Vec<PushEvseDataRequest> {
        let mut requests = Vec::with_capacity(3);
        for (action, records) in [
            (ActionType::Insert, self.inserts),
            (ActionType::Update, self.updates),
            (ActionType::Delete, self.deletes),
        ] {
            if !records.is_empty() {
                requests.push(PushEvseDataRequest {
                    action_type: action,
                    operator_evse_data: OperatorEvseData {
                        operator_id: operator_id.clone(),
                        operator_name: operator_name.clone(),
                        evse_data_record: records,
                        extensions: Extensions::new(),
                    },
                });
            }
        }
        requests
    }
}

/// Works out the minimal set of pushes that brings Hubject's copy of a fleet up to date.
///
/// # Why this exists
///
/// A CPO has one field with which to tell Hubject what to do — [`ActionType`] — and one of its
/// four values, `fullLoad`, **replaces everything Hubject holds for the operator**. The failure
/// mode is not subtle and not rare: a nightly job pushes the fleet with `fullLoad`, a filter in the
/// query is wrong one night, and every charge point the query missed vanishes from the roaming
/// network until someone notices the drop in sessions.
///
/// So this crate makes the safe thing the easy thing. Give the planner the last snapshot and the
/// current fleet, and it computes the `insert`/`update`/`delete` sets. `fullLoad` is a separate
/// method with a name that says what it does.
///
/// ```
/// # use oicp_kit::sync::PushPlanner;
/// # use oicp_kit::testkit::samples;
/// let previous = vec![samples::evse_data_record("DE*ABC*E1"), samples::evse_data_record("DE*ABC*E2")];
/// let current = vec![samples::evse_data_record("DE*ABC*E1"), samples::evse_data_record("DE*ABC*E3")];
///
/// let plan = PushPlanner::plan(&previous, &current);
/// assert_eq!(plan.inserts.len(), 1);   // E3 is new
/// assert_eq!(plan.deletes.len(), 1);   // E2 is gone
/// assert_eq!(plan.unchanged, 1);       // E1 is not sent at all
/// ```
#[derive(Clone, Copy, Debug)]
pub struct PushPlanner;

impl PushPlanner {
    /// Compares `previous` with `current` and returns the minimal pushes.
    ///
    /// Records are matched on the canonical form of their `EvseID`, so a CPO that changes how it
    /// writes its identifiers does not delete and re-insert its whole fleet.
    #[must_use]
    pub fn plan(previous: &[EvseDataRecord], current: &[EvseDataRecord]) -> PushPlan {
        let before: BTreeMap<String, &EvseDataRecord> =
            previous.iter().map(|r| (r.evse_id.canonical(), r)).collect();
        let after: BTreeMap<String, &EvseDataRecord> =
            current.iter().map(|r| (r.evse_id.canonical(), r)).collect();

        let mut plan = PushPlan { inserts: vec![], updates: vec![], deletes: vec![], unchanged: 0 };

        for (key, record) in &after {
            match before.get(key) {
                None => plan.inserts.push((*record).clone()),
                // `lastUpdate` and `deltaType` are Hubject's to write, so a difference in them is
                // not a change to the charging point and must not cause a pointless push.
                Some(existing) if !records_equal(existing, record) => plan.updates.push((*record).clone()),
                Some(_) => plan.unchanged += 1,
            }
        }
        for (key, record) in &before {
            if !after.contains_key(key) {
                plan.deletes.push((*record).clone());
            }
        }
        plan
    }

    /// Builds the destructive request that replaces everything Hubject holds for the operator.
    ///
    /// # This deletes what it does not contain
    ///
    /// Every charging point of `operator_id` that is not in `records` is withdrawn from the roaming
    /// network. Use [`plan`](Self::plan) for routine synchronisation; use this only to re-baseline
    /// deliberately, with a fleet list you are sure is complete.
    #[must_use]
    pub fn full_load(
        operator_id: OperatorId,
        operator_name: Text<100>,
        records: Vec<EvseDataRecord>,
    ) -> PushEvseDataRequest {
        PushEvseDataRequest {
            action_type: ActionType::FullLoad,
            operator_evse_data: OperatorEvseData {
                operator_id,
                operator_name,
                evse_data_record: records,
                extensions: Extensions::new(),
            },
        }
    }
}

/// Whether two records describe the same charging point in the same state.
///
/// Ignores the two fields Hubject owns: `deltaType` and `lastUpdate` change without the charging
/// point changing, and pushing on that would send the whole fleet every night.
fn records_equal(a: &EvseDataRecord, b: &EvseDataRecord) -> bool {
    let normalise = |r: &EvseDataRecord| {
        let mut copy = r.clone();
        copy.delta_type = None;
        copy.last_update = None;
        copy
    };
    normalise(a) == normalise(b)
}

// The tests here build fixtures with `testkit::samples`, so they compile when that feature
// is on. Without the gate `cargo test --features sync` fails to build.
#[cfg(all(test, feature = "testkit"))]
mod tests {
    use super::*;
    use crate::testkit::samples;
    use crate::types::DateTime;

    #[test]
    fn a_plan_reports_emptiness_and_size_honestly() {
        // `is_empty` is how a caller decides whether to send anything at all. One that wrongly says
        // "nothing to do" skips a fleet update in silence, and the next thing anyone notices is a
        // charging point missing from the roaming network.
        let one = samples::evse_data_record("DE*ABC*E1");
        let two = samples::evse_data_record("DE*ABC*E2");
        let three = samples::evse_data_record("DE*ABC*E3");

        let nothing = PushPlanner::plan(std::slice::from_ref(&one), std::slice::from_ref(&one));
        assert!(nothing.is_empty());
        assert_eq!(nothing.record_count(), 0);
        assert_eq!(nothing.unchanged, 1);

        // One of each, so a `+` written as a `*` cannot give the same answer.
        let mut changed = two.clone();
        changed.charging_station_id = Some(crate::types::Text::new_unchecked("moved"));
        let plan = PushPlanner::plan(&[one.clone(), two], &[one, changed, three.clone()]);
        assert_eq!((plan.inserts.len(), plan.updates.len(), plan.deletes.len()), (1, 1, 0));
        assert!(!plan.is_empty());
        assert_eq!(plan.record_count(), 2);

        // Emptiness needs *all three* lists empty, not any one of them.
        for plan in [
            PushPlan { inserts: vec![three.clone()], updates: vec![], deletes: vec![], unchanged: 0 },
            PushPlan { inserts: vec![], updates: vec![three.clone()], deletes: vec![], unchanged: 0 },
            PushPlan { inserts: vec![], updates: vec![], deletes: vec![three.clone()], unchanged: 0 },
        ] {
            assert!(!plan.is_empty(), "a plan with work to do is not empty");
            assert_eq!(plan.record_count(), 1);
        }
    }

    #[test]
    fn an_unchanged_fleet_produces_no_requests() {
        let fleet = vec![samples::evse_data_record("DE*ABC*E1"), samples::evse_data_record("DE*ABC*E2")];
        let plan = PushPlanner::plan(&fleet, &fleet);
        assert!(plan.is_empty());
        assert_eq!(plan.unchanged, 2);
        assert!(plan.into_requests(&"DE*ABC".parse().unwrap(), &Text::new("ABC").unwrap()).is_empty());
    }

    #[test]
    fn hubjects_own_fields_do_not_count_as_a_change() {
        let previous = vec![samples::evse_data_record("DE*ABC*E1")];
        let mut current = previous.clone();
        // Hubject stamps these; a CPO that pushed on them would send its whole fleet nightly.
        current[0].last_update = Some(DateTime::now());
        current[0].delta_type = Some(crate::cpo::DeltaType::Update);

        let plan = PushPlanner::plan(&previous, &current);
        assert!(plan.is_empty(), "only Hubject's own fields differ");
        assert_eq!(plan.unchanged, 1);
    }

    #[test]
    fn a_real_change_produces_exactly_one_update() {
        let previous = vec![samples::evse_data_record("DE*ABC*E1")];
        let mut current = previous.clone();
        current[0].is_hubject_compatible = false;

        let plan = PushPlanner::plan(&previous, &current);
        assert_eq!(plan.updates.len(), 1);
        assert_eq!(plan.inserts.len(), 0);
        assert_eq!(plan.deletes.len(), 0);
    }

    #[test]
    fn requests_send_deletions_last() {
        let previous = vec![samples::evse_data_record("DE*ABC*E1"), samples::evse_data_record("DE*ABC*E2")];
        let current = vec![samples::evse_data_record("DE*ABC*E3")];

        let plan = PushPlanner::plan(&previous, &current);
        let requests = plan.into_requests(&"DE*ABC".parse().unwrap(), &Text::new("ABC").unwrap());
        let actions: Vec<_> = requests.iter().map(|r| r.action_type).collect();
        assert_eq!(actions, vec![ActionType::Insert, ActionType::Delete]);
        // …and never the destructive one.
        assert!(!actions.iter().any(|a| a.is_destructive_replace()));
    }

    #[test]
    fn a_change_of_identifier_spelling_is_not_a_fleet_replacement() {
        let previous = vec![samples::evse_data_record("DE*ABC*E1")];
        let current = vec![samples::evse_data_record("DEABCE1")];
        let plan = PushPlanner::plan(&previous, &current);
        // The same charging point, written two ways.
        assert!(plan.deletes.is_empty(), "the old spelling must not be deleted");
        assert_eq!(plan.unchanged + plan.updates.len() as u64, 1);
    }

    #[test]
    fn full_load_is_reachable_only_by_its_own_name() {
        let request = PushPlanner::full_load(
            "DE*ABC".parse().unwrap(),
            Text::new("ABC").unwrap(),
            vec![samples::evse_data_record("DE*ABC*E1")],
        );
        assert!(request.action_type.is_destructive_replace());
    }
}
