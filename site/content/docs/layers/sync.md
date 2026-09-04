+++
title = "Delta sync"
weight = 40
description = "The LastCall engine, the four rules that are easy to get wrong quietly, and the CPO's mirror problem."
+++

An EMP needs a local copy of every charging point it can route a driver to. Pulling all of them
takes minutes and hundreds of megabytes, so nobody does it often. The specification's answer is the
`LastCall` delta: ask for the changes since an instant, and Hubject tags every record `insert`,
`update` or `delete`.

That sounds simple and is not.

## The four rules

**1. `LastCall` is exclusive with the filters.**

> *Cannot be combined with "SearchCenter", "CountryCodes", and "OperatorIDs".*

The reason is data integrity: a delta restricted to a country silently omits charge points that
moved *out* of it, and the stale records live forever. `PullEvseDataRequest::validate` reports the
combination, and `Planner` never constructs it.

**2. A delete is a tombstone, not an update.** It carries an `EvseID` and little else; applying it
as an upsert leaves a half-empty record that looks like a real charging point.

**3. The watermark advances on success, not on request.** Capture it *before* the crawl and write
it back *after* — and only if every page was applied. Get either wrong and the loss is permanent
and silent, because the next delta starts after the changes you lost.

**4. Deltas expire.** After a long outage a delta cannot be trusted and the copy must be rebuilt.

**5. `LastCall` is read by Hubject's clock and written by yours.** A machine running a minute fast
asks for changes since an instant Hubject has not reached yet, and everything recorded in that
minute is never sent again — silently, permanently, and worse the further the clocks drift. The
committed watermark is therefore held back by `clock_skew_guard`, five minutes by default. The cost
is a small overlap, and applying a delta twice is the same as applying it once.

## The engine

```rust
let planner = Planner::new(PlannerConfig::new(provider_id, GeoCoordinatesFormat::Google));

// `begin` decides the pull *and* empties the copy when the answer is a full one. A caller who
// skips that keeps every withdrawn charging point, and the crawl still reports success.
let (plan, watermark) = planner.begin(&mut repository)?;

let mut query = Some(PageQuery::new());
while let Some(current) = query {
    let page = client.pull_evse_data_page(plan.request(), current).await?;
    query = page.next_page().map(|n| PageQuery::at(n, current.size));
    sync::apply(&mut repository, page.content)?;
}

planner.commit(&mut repository, watermark)?;   // only now
```

`Planner::plan` is still there for a caller doing the re-baseline themselves; it takes `&R` and
changes nothing.

`Plan::Full` carries a `FullPullReason`, so a log says *why*: `NoWatermark`, `EmptyRepository`,
`WatermarkTooOld`, `WatermarkInFuture` (a clock went backwards — a delta from the future returns
nothing, forever), or `Requested`.

`ApplyOutcome` counts what happened, and counts drift separately:

```rust
let outcome = sync::apply(&mut repository, page.content)?;
if outcome.suggests_drift() {
    tracing::warn!(?outcome, "the local copy had drifted from Hubject's");
}
```

## The property that matters

```
applying any sequence of deltas leaves the same state as a full pull
```

`tests/properties.rs` generates arbitrary sequences of adds, modifications and removals, applies
the deltas Hubject would emit, and compares with a fresh full pull.

That test found a real bug in this crate: `deltaType` was being *stored*. It describes the pull,
not the charging point, so a delta-built copy carried stale tags and could never equal a
full-pull-built one. `sync::apply` now strips it and keeps `lastUpdate`, which *is* a fact about
the record.

## The CPO's mirror problem

A CPO must keep Hubject's copy of *its* fleet in step, and its tool is `ActionType` — where
`fullLoad` **replaces everything**. The failure mode is not rare: a nightly job pushes with
`fullLoad`, a filter in the query is wrong one night, and every charge point the query missed
vanishes from the roaming network until someone notices the drop in sessions.

```rust
let plan = PushPlanner::plan(&previous_snapshot, &current_fleet);
assert_eq!(plan.inserts.len(), 1);
assert_eq!(plan.deletes.len(), 1);
assert_eq!(plan.unchanged, 200);      // not sent at all

client.push_evse_data_plan(plan, "ABC Technologies").await?;
```

Requests come out in order — **inserts and updates first, deletions last** — because a charging
point being moved between stations exists throughout, and the other order blinks it out of the
roaming network for as long as the two requests take.

The planner ignores `deltaType` and `lastUpdate` when comparing, since Hubject writes those; a CPO
that pushed on them would send its whole fleet every night.

`PushPlanner::full_load` exists for a deliberate re-baseline. It has its own name so nobody reaches
it by accident.

## Your own storage

`EvseRepository` is four operations and no transactions — a delta is idempotent, so a crash
mid-page costs a repeat rather than a corruption, as long as the watermark only moves on success.
`InMemoryEvseRepository` is provided for tests, the CLI, and small EMPs.
