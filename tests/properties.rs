//! Properties that must hold for every input, not just the ones anyone thought to write down.

use oicp_kit::cpo::DeltaType;
use oicp_kit::sync::{self, EvseRepository, InMemoryEvseRepository, PushPlanner};
use oicp_kit::testkit::samples;
use oicp_kit::types::{EvcoId, EvseId, Number, OperatorId, ProviderId, SessionId, Uid, Validate};
use proptest::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

// --- identifiers ----------------------------------------------------------------------------

/// An ISO EvseID: two letters, three alphanumerics, `E`, then an instance.
fn iso_evse_id() -> impl Strategy<Value = String> {
    ("[A-Z]{2}", "[A-Z0-9]{3}", "[A-Z0-9]{1,10}", any::<bool>(), any::<bool>()).prop_map(
        |(country, operator, instance, sep1, sep2)| {
            let s1 = if sep1 { "*" } else { "" };
            let s2 = if sep2 { "*" } else { "" };
            format!("{country}{s1}{operator}{s2}E{instance}")
        },
    )
}

/// A DIN EvseID: `+?` digits, `*`, three digits, `*`, then digits.
fn din_evse_id() -> impl Strategy<Value = String> {
    ("[0-9]{1,3}", "[0-9]{3}", "[0-9]{1,10}", any::<bool>()).prop_map(
        |(country, operator, instance, plus)| {
            let p = if plus { "+" } else { "" };
            format!("{p}{country}*{operator}*{instance}")
        },
    )
}

proptest! {
    /// An identifier is written back exactly as it arrived — the property Hubject's certificate
    /// check depends on.
    #[test]
    fn an_evse_id_is_never_rewritten(text in prop_oneof![iso_evse_id(), din_evse_id()]) {
        let id: EvseId = text.parse().expect("the generated id is well formed");
        prop_assert_eq!(id.to_string(), text.clone());
        prop_assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{text}\""));
        prop_assert!(id.is_well_formed());
        prop_assert!(id.validate().is_ok());
    }

    /// Separators and case do not change which charging spot an identifier names.
    #[test]
    fn separators_and_case_do_not_change_identity(
        country in "[A-Z]{2}", operator in "[A-Z0-9]{3}", instance in "[A-Z0-9]{1,10}"
    ) {
        let spaced: EvseId = format!("{country}*{operator}*E{instance}").parse().unwrap();
        let packed: EvseId = format!("{country}{operator}E{instance}").parse().unwrap();
        let lower: EvseId = format!("{}*{}*e{}", country.to_lowercase(), operator.to_lowercase(), instance.to_lowercase())
            .parse()
            .unwrap();

        prop_assert_eq!(&spaced, &packed);
        prop_assert_eq!(&spaced, &lower);
        // Equal values must hash equally, or a HashMap of charging points silently duplicates.
        let hash = |id: &EvseId| { let mut h = DefaultHasher::new(); id.hash(&mut h); h.finish() };
        prop_assert_eq!(hash(&spaced), hash(&packed));
        prop_assert_eq!(hash(&spaced), hash(&lower));
        // …and the operator is the same however the id was written.
        prop_assert_eq!(spaced.operator_id(), packed.operator_id());
    }

    /// Any string decodes — the page must not fail — and is reported if malformed.
    #[test]
    fn any_string_decodes_as_an_identifier_and_round_trips(text in ".{0,40}") {
        let json = serde_json::to_string(&text).unwrap();
        let id: EvseId = serde_json::from_str(&json).expect("decoding never fails");
        prop_assert_eq!(serde_json::to_string(&id).unwrap(), json);
        // Well-formed or reported: never silently wrong.
        prop_assert!(id.is_well_formed() != id.validate().is_err());
    }

    /// The same, for the other identifier types.
    #[test]
    fn every_identifier_type_decodes_anything_and_round_trips(text in ".{0,40}") {
        let json = serde_json::to_string(&text).unwrap();
        macro_rules! check {
            ($ty:ty) => {{
                let id: $ty = serde_json::from_str(&json).expect("decoding never fails");
                prop_assert_eq!(serde_json::to_string(&id).unwrap(), json.clone());
            }};
        }
        check!(EvcoId);
        check!(OperatorId);
        check!(ProviderId);
        check!(SessionId);
        check!(Uid);
    }
}

// --- numbers --------------------------------------------------------------------------------

proptest! {
    /// Every number OICP can carry survives the JSON boundary exactly.
    #[test]
    fn energy_and_money_survive_json(units in 0i64..1_000_000, cents in 0u32..10_000) {
        let text = format!("{units}.{cents:04}");
        let value: Number = text.parse().expect("a decimal");
        let json = serde_json::to_string(&value).unwrap();
        let back: Number = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(value.get().normalize(), back.get().normalize(), "{} became {}", text, json);
        prop_assert!(value.json_round_trips());
        prop_assert!(value.validate().is_ok());
    }

    /// The identity OICP defines for every CDR holds exactly — the reason this is not an `f64`.
    #[test]
    fn the_cdr_energy_identity_is_exact(start_units in 0i64..100_000, delta_units in 0i64..100_000, frac in 0u32..10_000) {
        let start: Number = format!("{start_units}.{frac:04}").parse().unwrap();
        let delta: Number = format!("{delta_units}.0000").parse().unwrap();
        let end = start + delta;
        prop_assert_eq!(end - start, delta);
    }
}

// --- the delta engine -----------------------------------------------------------------------

/// One step a CPO might take, which Hubject turns into a delta record.
#[derive(Clone, Debug)]
enum Change {
    Add(u8),
    Modify(u8),
    Remove(u8),
}

fn changes() -> impl Strategy<Value = Vec<Change>> {
    prop::collection::vec(
        prop_oneof![
            (0u8..12).prop_map(Change::Add),
            (0u8..12).prop_map(Change::Modify),
            (0u8..12).prop_map(Change::Remove),
        ],
        0..40,
    )
}

/// The world as the CPO sees it, and the deltas Hubject would emit for each change.
#[derive(Default)]
struct World {
    live: std::collections::BTreeSet<u8>,
    modified: std::collections::BTreeSet<u8>,
}

impl World {
    fn apply(&mut self, change: &Change) -> Option<oicp_kit::emp::PullEvseDataRecord> {
        // Which record to emit, and whether it is the modified shape — decided before the record
        // is built, so the world's state is not borrowed while building it.
        let (n, delta) = match change {
            Change::Add(n) if self.live.insert(*n) => (*n, DeltaType::Insert),
            Change::Modify(n) if self.live.contains(n) => {
                self.modified.insert(*n);
                (*n, DeltaType::Update)
            }
            Change::Remove(n) if self.live.remove(n) => {
                self.modified.remove(n);
                (*n, DeltaType::Delete)
            }
            // Hubject emits nothing for a change that changes nothing.
            _ => return None,
        };
        let mut record = samples::pull_evse_data_record(&format!("DE*ABC*E{n}"));
        record.delta_type = Some(delta);
        // A modification has to be visible in the record, or "converges" is vacuous.
        if self.modified.contains(&n) {
            record.is_hubject_compatible = false;
        }
        Some(record)
    }

    /// What a *full* pull would return right now.
    fn full_pull(&self) -> Vec<oicp_kit::emp::PullEvseDataRecord> {
        self.live
            .iter()
            .map(|n| {
                let mut r = samples::pull_evse_data_record(&format!("DE*ABC*E{n}"));
                if self.modified.contains(n) {
                    r.is_hubject_compatible = false;
                }
                r
            })
            .collect()
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// **The property the delta engine exists for.**
    ///
    /// Applying any sequence of deltas leaves the EMP's copy in the state a fresh full pull would
    /// return. If this ever fails, an EMP's map of Europe has drifted from Hubject's, and nobody
    /// finds out until a driver is sent to a charging point that no longer exists.
    #[test]
    fn a_delta_sequence_converges_on_the_full_pull(changes in changes()) {
        let mut world = World::default();
        let mut repository = InMemoryEvseRepository::new();

        for change in &changes {
            if let Some(record) = world.apply(change) {
                sync::apply(&mut repository, [record]).expect("in-memory apply cannot fail");
            }
        }

        let mut expected = InMemoryEvseRepository::new();
        sync::apply(&mut expected, world.full_pull()).expect("in-memory apply cannot fail");

        prop_assert_eq!(repository.keys(), expected.keys(), "the delta copy holds different charging points");
        for record in expected.iter() {
            let actual = repository.get(&record.evse_id).unwrap();
            prop_assert_eq!(actual.as_ref(), Some(record), "a record differs after the deltas");
        }
    }

    /// Applying the same delta twice leaves the same state — so a retried page is harmless.
    #[test]
    fn applying_a_delta_twice_is_the_same_as_applying_it_once(changes in changes()) {
        let mut world = World::default();
        let records: Vec<_> = changes.iter().filter_map(|c| world.apply(c)).collect();

        let mut once = InMemoryEvseRepository::new();
        sync::apply(&mut once, records.clone()).unwrap();

        let mut twice = InMemoryEvseRepository::new();
        sync::apply(&mut twice, records.clone()).unwrap();
        sync::apply(&mut twice, records).unwrap();

        prop_assert_eq!(once.keys(), twice.keys());
    }
}

// --- the push planner -----------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Applying the planner's pushes to the previous fleet reproduces the current one — and none
    /// of them is the destructive `fullLoad`.
    #[test]
    fn a_push_plan_reproduces_the_current_fleet(
        previous in prop::collection::vec(0u8..12, 0..12),
        current in prop::collection::vec(0u8..12, 0..12),
    ) {
        let to_records = |ns: &[u8]| {
            let mut seen = std::collections::BTreeSet::new();
            ns.iter()
                .filter(|n| seen.insert(**n))
                .map(|n| samples::evse_data_record(&format!("DE*ABC*E{n}")))
                .collect::<Vec<_>>()
        };
        let previous = to_records(&previous);
        let current = to_records(&current);

        let plan = PushPlanner::plan(&previous, &current);
        prop_assert!(
            plan.record_count() + usize::try_from(plan.unchanged).unwrap_or(usize::MAX) >= current.len(),
            "the plan does not account for every current record"
        );

        // Replay the plan onto Hubject's copy and check it matches the current fleet.
        let mut hubject: std::collections::BTreeMap<String, _> =
            previous.iter().map(|r| (r.evse_id.canonical(), r.clone())).collect();
        for record in plan.inserts.iter().chain(plan.updates.iter()) {
            hubject.insert(record.evse_id.canonical(), record.clone());
        }
        for record in &plan.deletes {
            hubject.remove(&record.evse_id.canonical());
        }

        let expected: std::collections::BTreeMap<String, _> =
            current.iter().map(|r| (r.evse_id.canonical(), r.clone())).collect();
        prop_assert_eq!(hubject.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());

        // …and routine synchronisation never reaches for fullLoad.
        let requests = plan.into_requests(&samples::operator_id(), &oicp_kit::types::Text::new_unchecked("ABC"));
        for request in &requests {
            prop_assert!(!request.action_type.is_destructive_replace());
            prop_assert!(request.validate().is_ok());
        }
    }
}

// --- extensions -----------------------------------------------------------------------------

proptest! {
    /// Whatever a peer adds to an object, it comes back out unchanged.
    #[test]
    fn arbitrary_unknown_fields_survive(
        key in "[a-zA-Z][a-zA-Z0-9_]{0,20}",
        value in prop_oneof![
            Just(serde_json::Value::Null),
            any::<bool>().prop_map(serde_json::Value::Bool),
            any::<i32>().prop_map(|n| serde_json::json!(n)),
            "[a-zA-Z0-9 ]{0,30}".prop_map(|s| serde_json::json!(s)),
        ],
    ) {
        // Skip keys the object itself defines.
        prop_assume!(!["EvseID", "EvseStatus"].contains(&key.as_str()));

        let mut json = serde_json::json!({"EvseID": "DE*ABC*E1", "EvseStatus": "Available"});
        json.as_object_mut().unwrap().insert(key.clone(), value.clone());

        let record: oicp_kit::cpo::EvseStatusRecord = serde_json::from_value(json.clone()).expect("decodes");
        prop_assert_eq!(record.extensions.get_raw(&key), Some(&value));
        prop_assert_eq!(serde_json::to_value(&record).unwrap(), json);
    }
}
