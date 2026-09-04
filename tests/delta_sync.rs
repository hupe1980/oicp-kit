//! The delta engine against a broker: the whole loop, including the parts that go wrong.

use core::time::Duration;

use oicp_kit::cpo::DeltaType;
use oicp_kit::sync::{
    self, EvseRepository, FullPullReason, InMemoryEvseRepository, Plan, Planner, PlannerConfig,
};
use oicp_kit::testkit::{MockEmp, MockHubject, samples};
use oicp_kit::transport::PageQuery;
use oicp_kit::types::{DateTime, GeoCoordinatesFormat, ProviderId, Validate};

fn provider() -> ProviderId {
    "DE-DCB".parse().expect("valid")
}

fn planner() -> Planner {
    Planner::new(PlannerConfig::new(provider(), GeoCoordinatesFormat::Google))
}

fn broker_with(fleet: u32) -> MockHubject {
    let mut hubject = MockHubject::new();
    hubject.register_emp(MockEmp::permissive(provider()));
    for i in 0..fleet {
        hubject.push_evse_data(&samples::evse_data_record(&format!("DE*ABC*E{i}")).into()).expect("push");
    }
    hubject
}

/// One full crawl, applied. Returns how many records were seen.
fn crawl(
    hubject: &MockHubject,
    repository: &mut InMemoryEvseRepository,
    planner: &Planner,
    size: u32,
) -> u64 {
    let (plan, watermark) = planner.plan(repository).expect("planning");
    if plan.replaces_everything() {
        repository.clear().expect("clear");
    }
    let mut seen = 0;
    let mut query = Some(PageQuery::with_size(size));
    while let Some(current) = query {
        let page = hubject.pull_evse_data(plan.request(), current);
        page.validate().expect("the mock's pages are conformant");
        seen += page.content.len() as u64;
        query = page.next_page().map(|n| PageQuery::at(n, size));
        sync::apply(repository, page.content).expect("apply");
    }
    planner.commit(repository, watermark).expect("commit");
    seen
}

#[test]
fn a_first_crawl_pulls_everything_and_a_second_pulls_nothing() {
    let hubject = broker_with(25);
    let planner = planner();
    let mut repository = InMemoryEvseRepository::new();

    // The first plan must be a full pull: there is nothing to build a delta on.
    let (plan, _) = planner.plan(&repository).unwrap();
    assert!(matches!(plan, Plan::Full { reason: FullPullReason::NoWatermark, .. }));

    assert_eq!(crawl(&hubject, &mut repository, &planner, 10), 25);
    assert_eq!(repository.len().unwrap(), 25);

    // The second is a delta, and the mock has nothing new — the copy is unchanged.
    let (plan, _) = planner.plan(&repository).unwrap();
    assert!(!plan.replaces_everything());
    assert!(plan.request().is_delta());
    assert_eq!(repository.len().unwrap(), 25);
}

#[test]
fn a_crawl_over_many_pages_sees_every_record_exactly_once() {
    let hubject = broker_with(97);
    let planner = planner();
    let mut repository = InMemoryEvseRepository::new();

    // A page size that does not divide the fleet, so the last page is partial.
    assert_eq!(crawl(&hubject, &mut repository, &planner, 10), 97);
    assert_eq!(repository.len().unwrap(), 97);

    let keys = repository.keys();
    let mut unique = keys.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(keys.len(), unique.len(), "a record was seen twice");
}

#[test]
fn a_delta_that_deletes_removes_the_record_rather_than_corrupting_it() {
    let mut repository = InMemoryEvseRepository::new();
    sync::apply(&mut repository, samples::fleet("DE*ABC", 3)).unwrap();
    assert_eq!(repository.len().unwrap(), 3);

    // A deletion tombstone carries the EvseID and little else. Applying it as an upsert would
    // leave a half-empty record behind that looks like a real charging point.
    let mut tombstone = samples::pull_evse_data_record("DE*ABC*E1");
    tombstone.delta_type = Some(DeltaType::Delete);
    tombstone.charging_station_names.clear();

    let outcome = sync::apply(&mut repository, [tombstone]).unwrap();
    assert_eq!(outcome.deleted, 1);
    assert_eq!(repository.len().unwrap(), 2);
    assert!(repository.get(&"DE*ABC*E1".parse().unwrap()).unwrap().is_none());
}

#[test]
fn the_stored_record_carries_no_delta_type() {
    // Regression: `deltaType` describes the pull, not the charging point. Storing it would make a
    // copy built from deltas differ forever from one built by a full pull.
    let mut repository = InMemoryEvseRepository::new();
    let mut record = samples::pull_evse_data_record("DE*ABC*E1");
    record.delta_type = Some(DeltaType::Insert);
    record.last_update = Some(samples::fixed_time());

    sync::apply(&mut repository, [record]).unwrap();
    let stored = repository.get(&"DE*ABC*E1".parse().unwrap()).unwrap().unwrap();

    assert_eq!(stored.delta_type, None, "deltaType must not be stored");
    assert_eq!(stored.last_update, Some(samples::fixed_time()), "lastUpdate is a fact about the record");
}

#[test]
fn the_watermark_only_moves_when_the_whole_crawl_succeeded() {
    let hubject = broker_with(5);
    let planner = planner();
    let mut repository = InMemoryEvseRepository::new();

    let (plan, watermark) = planner.plan(&repository).unwrap();
    let page = hubject.pull_evse_data(plan.request(), PageQuery::with_size(2));
    sync::apply(&mut repository, page.content).unwrap();

    // The crawl is not finished; the watermark must not have moved, or the pages we have not
    // applied yet are lost permanently.
    assert_eq!(repository.last_call().unwrap(), None);

    planner.commit(&mut repository, watermark.clone()).unwrap();
    assert_eq!(repository.last_call().unwrap(), Some(watermark));
}

#[test]
fn drift_is_visible_in_the_outcome() {
    let mut repository = InMemoryEvseRepository::new();

    // A deletion for a record we never had: the copy had drifted, or the delta was replayed.
    let mut tombstone = samples::pull_evse_data_record("DE*ABC*E1");
    tombstone.delta_type = Some(DeltaType::Delete);
    let outcome = sync::apply(&mut repository, [tombstone]).unwrap();
    assert_eq!(outcome.deleted_unknown, 1);
    assert!(outcome.suggests_drift());

    // An update for a record we never had: kept, because a charging point we cannot route to is
    // worse than a surprising one — but counted separately.
    let mut update = samples::pull_evse_data_record("DE*ABC*E2");
    update.delta_type = Some(DeltaType::Update);
    let outcome = sync::apply(&mut repository, [update]).unwrap();
    assert_eq!(outcome.updated_unknown, 1);
    assert_eq!(repository.len().unwrap(), 1, "the record was kept");
}

#[test]
fn the_outcome_counts_the_ordinary_work_too() {
    // `ApplyOutcome` is what a caller logs — "412 inserted, 88 updated, 3 deleted". A counter that
    // never moves reads as a quiet night rather than as a broken counter, so each one is asserted
    // at a value it could not reach by accident.
    let mut repository = InMemoryEvseRepository::new();

    let first = sync::apply(
        &mut repository,
        [samples::pull_evse_data_record("DE*ABC*E1"), samples::pull_evse_data_record("DE*ABC*E2")],
    )
    .unwrap();
    assert_eq!(first.inserted, 2, "both records were new");
    assert_eq!(first.updated, 0);
    assert_eq!(first.total(), 2);
    assert!(!first.suggests_drift());

    // The same two again, plus a third: two replacements and one new record.
    let mut third = samples::pull_evse_data_record("DE*ABC*E3");
    third.delta_type = Some(DeltaType::Insert);
    let second = sync::apply(
        &mut repository,
        [samples::pull_evse_data_record("DE*ABC*E1"), samples::pull_evse_data_record("DE*ABC*E2"), third],
    )
    .unwrap();
    assert_eq!(second.updated, 2, "the two we had were replaced");
    assert_eq!(second.inserted, 1);
    assert_eq!(second.total(), 3);
    assert!(!second.suggests_drift(), "a re-send of a record we hold is not drift");
    assert_eq!(repository.len().unwrap(), 3);

    // An `update` for a record we hold is the ordinary delta case, and lands in `updated` rather
    // than in the drift counter beside it.
    let mut known = samples::pull_evse_data_record("DE*ABC*E1");
    known.delta_type = Some(DeltaType::Update);
    let ordinary = sync::apply(&mut repository, [known]).unwrap();
    assert_eq!(ordinary.updated, 1);
    assert_eq!(ordinary.updated_unknown, 0, "we had this one, so it is not drift");
    assert!(!ordinary.suggests_drift());

    // And a deletion we can act on.
    let mut gone = samples::pull_evse_data_record("DE*ABC*E2");
    gone.delta_type = Some(DeltaType::Delete);
    let third_outcome = sync::apply(&mut repository, [gone]).unwrap();
    assert_eq!(third_outcome.deleted, 1);
    assert_eq!(third_outcome.deleted_unknown, 0);
    assert_eq!(repository.len().unwrap(), 2);
}

#[test]
fn a_stale_watermark_forces_a_rebaseline_and_the_rebaseline_clears_first() {
    let hubject = broker_with(3);
    let planner = Planner::new(
        PlannerConfig::new(provider(), GeoCoordinatesFormat::Google)
            .with_max_delta_age(Duration::from_secs(60)),
    );
    let mut repository = InMemoryEvseRepository::new();
    crawl(&hubject, &mut repository, &planner, 10);
    assert_eq!(repository.len().unwrap(), 3);

    // Rewind the watermark past the maximum delta age.
    let stale: DateTime = "2020-01-01T00:00:00.000Z".parse().unwrap();
    repository.set_last_call(stale).unwrap();

    let (plan, _) = planner.plan(&repository).unwrap();
    assert!(matches!(plan, Plan::Full { reason: FullPullReason::WatermarkTooOld, .. }));
    assert!(plan.replaces_everything(), "a full pull is the whole world; the copy must be cleared first");
}

#[test]
fn a_delta_plan_never_carries_the_filters_the_spec_forbids() {
    let hubject = broker_with(2);
    let planner = planner();
    let mut repository = InMemoryEvseRepository::new();
    crawl(&hubject, &mut repository, &planner, 10);

    let (plan, _) = planner.plan(&repository).unwrap();
    let request = plan.request();
    assert!(request.is_delta());
    assert!(
        request.conflicting_filters().is_empty(),
        "a delta scoped to a region omits charge points that moved out of it"
    );
    request.validate().expect("the planner never builds an invalid request");
}

#[test]
fn a_full_pull_after_a_withdrawal_removes_what_is_gone() {
    // The reason a full pull must clear first: a charging point withdrawn while the EMP was away
    // appears in no delta and in no full pull, so only the clear removes it.
    let hubject = broker_with(3);
    let planner = planner();
    let mut repository = InMemoryEvseRepository::new();
    crawl(&hubject, &mut repository, &planner, 10);
    assert_eq!(repository.len().unwrap(), 3);

    // Something that is no longer in the broker's data.
    sync::apply(&mut repository, [samples::pull_evse_data_record("DE*ABC*E99")]).unwrap();
    assert_eq!(repository.len().unwrap(), 4);

    let rebaseline = Planner::new(PlannerConfig::new(provider(), GeoCoordinatesFormat::Google).rebaseline());
    crawl(&hubject, &mut repository, &rebaseline, 10);
    assert_eq!(repository.len().unwrap(), 3, "the stale record is gone");
}
