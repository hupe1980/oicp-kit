//! Deciding what kind of pull to make, and when the watermark may move.

use core::fmt;
use core::time::Duration;

use super::repository::EvseRepository;
use crate::emp::PullEvseDataRequest;
use crate::types::{DateTime, GeoCoordinatesFormat, ProviderId};

/// What the planner decided to do next.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// Pull everything, because there is nothing to build a delta on.
    Full {
        /// The request to send.
        request: Box<PullEvseDataRequest>,
        /// Why a full pull rather than a delta.
        reason: FullPullReason,
    },
    /// Pull the changes since the watermark.
    Delta {
        /// The request to send.
        request: Box<PullEvseDataRequest>,
        /// The watermark the delta starts from.
        since: DateTime,
    },
}

impl Plan {
    /// The request to send.
    #[must_use]
    pub fn request(&self) -> &PullEvseDataRequest {
        match self {
            Self::Full { request, .. } | Self::Delta { request, .. } => request,
        }
    }

    /// Whether the local copy must be emptied before applying the answer.
    ///
    /// True for a full pull: the answer *is* the whole world, so anything not in it is gone. A
    /// caller that skips this keeps every charge point that was withdrawn while it was away.
    #[must_use]
    pub const fn replaces_everything(&self) -> bool {
        matches!(self, Self::Full { .. })
    }
}

/// Why the planner asked for everything rather than a delta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullPullReason {
    /// Nothing has ever been pulled.
    NoWatermark,
    /// The local copy is empty, so there is nothing for a delta to build on.
    EmptyRepository,
    /// The watermark is older than the configured maximum delta age.
    WatermarkTooOld,
    /// The watermark is in the future — a clock went backwards, and a delta from it would
    /// silently return nothing.
    WatermarkInFuture,
    /// A re-baseline was asked for.
    Requested,
}

impl fmt::Display for FullPullReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoWatermark => "nothing has been pulled yet",
            Self::EmptyRepository => "the local copy is empty",
            Self::WatermarkTooOld => "the watermark is older than the maximum delta age",
            Self::WatermarkInFuture => "the watermark is in the future",
            Self::Requested => "a re-baseline was requested",
        })
    }
}

/// How the planner should behave.
#[derive(Clone, Debug)]
pub struct PlannerConfig {
    /// The EMP doing the pulling.
    pub provider_id: ProviderId,
    /// Which notation coordinates should come back in.
    ///
    /// Keep this stable: changing it rewrites the `GeoCoordinates` of every record on the next
    /// full pull, and a delta will not reconcile the two notations for you.
    pub geo_format: GeoCoordinatesFormat,
    /// How old a watermark may be before a delta is no longer trusted.
    ///
    /// OICP documents no retention period for delta history, so this is a judgement rather than a
    /// spec rule. The default is 24 hours: long enough to ride out an overnight outage, short
    /// enough that a week-old watermark forces the re-baseline it needs.
    pub max_delta_age: Duration,
    /// Force a full pull on the next plan.
    pub force_full: bool,
    /// How far back to place a committed watermark, to absorb clock skew.
    ///
    /// `LastCall` is read by **Hubject's** clock and written from this machine's. If this machine
    /// runs a minute fast, every change Hubject records in that minute falls before the watermark
    /// and is never sent again: the loss is silent, permanent, and grows with the skew. Committing
    /// the watermark slightly in the past costs a small overlap — records that arrive twice, which
    /// the engine applies idempotently — and buys the guarantee that nothing falls through.
    ///
    /// The default is five minutes: more than any clock a partner is likely to run against NTP,
    /// and small enough that the overlap is a handful of records.
    pub clock_skew_guard: Duration,
}

impl PlannerConfig {
    /// The default configuration for `provider_id`.
    #[must_use]
    pub fn new(provider_id: ProviderId, geo_format: GeoCoordinatesFormat) -> Self {
        Self {
            provider_id,
            geo_format,
            max_delta_age: Duration::from_secs(24 * 60 * 60),
            force_full: false,
            clock_skew_guard: Duration::from_secs(5 * 60),
        }
    }

    /// Sets how far back a committed watermark is placed. See the field documentation.
    #[must_use]
    pub const fn with_clock_skew_guard(mut self, guard: Duration) -> Self {
        self.clock_skew_guard = guard;
        self
    }

    /// Sets the maximum age of a watermark that may still be used for a delta.
    #[must_use]
    pub const fn with_max_delta_age(mut self, age: Duration) -> Self {
        self.max_delta_age = age;
        self
    }

    /// Forces the next plan to be a full pull.
    #[must_use]
    pub const fn rebaseline(mut self) -> Self {
        self.force_full = true;
        self
    }
}

/// Decides whether the next EVSE-data pull is a full one or a delta, and when the watermark moves.
///
/// # The watermark rule
///
/// The instant a delta starts from is captured **before** the crawl, and written back **after**
/// it — and only if every page was applied. That ordering is the whole game:
///
/// * Capture it after the crawl, and every change made *during* the crawl is lost.
/// * Write it back before the crawl finishes, and a failed page's changes are lost.
///
/// Both losses are permanent and silent, because the next delta starts after them.
/// [`Planner::plan`] hands you the instant to record, and
/// [`Planner::commit`] is the only thing that writes it.
///
/// ```no_run
/// # use oicp_kit::sync::{Planner, PlannerConfig, InMemoryEvseRepository};
/// # use oicp_kit::types::GeoCoordinatesFormat;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut repository = InMemoryEvseRepository::new();
/// let planner = Planner::new(PlannerConfig::new("DE-DCB".parse()?, GeoCoordinatesFormat::Google));
///
/// // `begin` decides the pull *and* empties the copy when the answer is a full one.
/// let (plan, watermark) = planner.begin(&mut repository)?;
/// // …crawl every page of `plan.request()` and apply it…
/// planner.commit(&mut repository, watermark)?;   // only now
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct Planner {
    config: PlannerConfig,
}

impl Planner {
    /// A planner with `config`.
    #[must_use]
    pub const fn new(config: PlannerConfig) -> Self {
        Self { config }
    }

    /// The configuration.
    #[must_use]
    pub const fn config(&self) -> &PlannerConfig {
        &self.config
    }

    /// Decides what to pull next, and hands back the watermark to commit afterwards.
    ///
    /// Read-only. Prefer [`begin`](Self::begin) unless you are handling the re-baseline yourself:
    /// a full pull's answer *is* the whole world, so a copy that is not emptied first keeps every
    /// charging point that was withdrawn while this EMP was away, forever.
    ///
    /// # Errors
    ///
    /// Whatever the repository returns.
    pub fn plan<R: EvseRepository>(&self, repository: &R) -> Result<(Plan, DateTime), R::Error> {
        self.plan_at(repository, DateTime::now())
    }

    /// [`plan`](Self::plan), and empties the copy when the plan replaces everything.
    ///
    /// The rule that a full pull must start from an empty copy is one sentence of documentation
    /// and one line of caller code, and the failure when it is skipped is invisible: the crawl
    /// succeeds, the counts look right, and withdrawn charging points stay on the map until
    /// somebody routes a driver to one. So the rule lives here instead.
    ///
    /// The copy is unusable between this call and the end of the crawl — a full pull is a
    /// re-baseline, and there is no half of one that is safe to route on.
    ///
    /// # Errors
    ///
    /// Whatever the repository returns.
    pub fn begin<R: EvseRepository>(&self, repository: &mut R) -> Result<(Plan, DateTime), R::Error> {
        self.begin_at(repository, DateTime::now())
    }

    /// [`begin`](Self::begin), with the current instant supplied — for tests.
    ///
    /// # Errors
    ///
    /// Whatever the repository returns.
    pub fn begin_at<R: EvseRepository>(
        &self,
        repository: &mut R,
        now: DateTime,
    ) -> Result<(Plan, DateTime), R::Error> {
        let (plan, watermark) = self.plan_at(repository, now)?;
        if plan.replaces_everything() {
            repository.clear()?;
        }
        Ok((plan, watermark))
    }

    /// [`plan`](Self::plan), with the current instant supplied — for tests.
    ///
    /// # Errors
    ///
    /// Whatever the repository returns.
    pub fn plan_at<R: EvseRepository>(
        &self,
        repository: &R,
        now: DateTime,
    ) -> Result<(Plan, DateTime), R::Error> {
        let plan = if let Some(reason) = self.full_pull_reason(repository, &now)? {
            Plan::Full {
                request: Box::new(PullEvseDataRequest::full(
                    self.config.provider_id.clone(),
                    self.config.geo_format,
                )),
                reason,
            }
        } else {
            // `full_pull_reason` returns `None` only when there is a usable watermark.
            let since = repository.last_call()?.expect("a delta plan requires a watermark");
            Plan::Delta {
                request: Box::new(PullEvseDataRequest::delta(
                    self.config.provider_id.clone(),
                    self.config.geo_format,
                    since.clone(),
                )),
                since,
            }
        };
        Ok((plan, self.guarded(&now)))
    }

    /// `now`, moved back by [`clock_skew_guard`](PlannerConfig::clock_skew_guard).
    fn guarded(&self, now: &DateTime) -> DateTime {
        let (Some(instant), Ok(guard)) =
            (now.as_offset(), time::Duration::try_from(self.config.clock_skew_guard))
        else {
            return now.clone();
        };
        DateTime::from_offset(instant - guard)
    }

    /// Whether this pull must be a full one, and why.
    fn full_pull_reason<R: EvseRepository>(
        &self,
        repository: &R,
        now: &DateTime,
    ) -> Result<Option<FullPullReason>, R::Error> {
        if self.config.force_full {
            return Ok(Some(FullPullReason::Requested));
        }
        let Some(watermark) = repository.last_call()? else {
            return Ok(Some(FullPullReason::NoWatermark));
        };
        if repository.is_empty()? {
            // A delta onto nothing yields only what changed, which is not a usable copy.
            return Ok(Some(FullPullReason::EmptyRepository));
        }
        let (Some(now), Some(mark)) = (now.as_offset(), watermark.as_offset()) else {
            // An unreadable watermark cannot be sent as `LastCall`, and an unreadable `now`
            // cannot be committed as the next one.
            return Ok(Some(FullPullReason::NoWatermark));
        };
        let elapsed = now - mark;
        if elapsed.is_negative() {
            // The watermark is in the future: a delta from it returns nothing at all, and the
            // copy would freeze silently until the clocks agree again.
            return Ok(Some(FullPullReason::WatermarkInFuture));
        }
        if elapsed.unsigned_abs() > self.config.max_delta_age {
            return Ok(Some(FullPullReason::WatermarkTooOld));
        }
        Ok(None)
    }

    /// Records that a crawl finished successfully, advancing the watermark to `at`.
    ///
    /// Call this **only** once every page has been applied. `at` is the instant
    /// [`plan`](Self::plan) handed back — captured before the crawl, so changes made during it are
    /// picked up next time rather than lost.
    ///
    /// # Errors
    ///
    /// Whatever the repository returns.
    pub fn commit<R: EvseRepository>(&self, repository: &mut R, at: DateTime) -> Result<(), R::Error> {
        repository.set_last_call(at)
    }
}

// The tests here build fixtures with `testkit::samples`, so they compile when that feature
// is on. Without the gate `cargo test --features sync` fails to build.
#[cfg(all(test, feature = "testkit"))]
mod tests {
    use super::*;
    use crate::sync::{EvseRepository, InMemoryEvseRepository};
    use crate::testkit::samples;
    use crate::types::Validate;

    fn planner() -> Planner {
        Planner::new(PlannerConfig::new("DE-DCB".parse().unwrap(), GeoCoordinatesFormat::Google))
    }

    fn populated() -> InMemoryEvseRepository {
        let mut repo = InMemoryEvseRepository::new();
        repo.upsert(samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
        repo
    }

    #[test]
    fn the_first_pull_is_always_a_full_one() {
        let (plan, _) = planner().plan(&InMemoryEvseRepository::new()).unwrap();
        assert!(matches!(plan, Plan::Full { reason: FullPullReason::NoWatermark, .. }));
        assert!(plan.replaces_everything());
        assert!(!plan.request().is_delta());
    }

    #[test]
    fn a_recent_watermark_and_a_populated_copy_gives_a_delta() {
        let mut repo = populated();
        let now: DateTime = "2026-08-31T12:00:00.000Z".parse().unwrap();
        repo.set_last_call("2026-08-31T11:00:00.000Z".parse().unwrap()).unwrap();

        let (plan, watermark) = planner().plan_at(&repo, now.clone()).unwrap();
        let Plan::Delta { request, since } = plan else { panic!("expected a delta") };
        assert_eq!(since.as_str(), "2026-08-31T11:00:00.000Z");
        assert!(request.is_delta());
        // The delta carries no filters, so it cannot go stale — and it validates.
        assert!(request.conflicting_filters().is_empty());
        assert!(request.validate().is_ok());
        // The watermark to commit is *now*, less the skew guard — not the old one.
        assert_eq!(watermark.as_str(), "2026-08-31T11:55:00Z");
        assert!(watermark < now);
    }

    #[test]
    fn the_committed_watermark_is_held_back_by_the_skew_guard() {
        // `LastCall` is read by Hubject's clock. A machine running a minute fast would otherwise
        // ask for changes since an instant Hubject has not reached, and every change in that
        // minute would never be sent again.
        let mut repo = populated();
        let now: DateTime = "2026-08-31T12:00:00.000Z".parse().unwrap();
        repo.set_last_call("2026-08-31T11:00:00.000Z".parse().unwrap()).unwrap();

        let (_, watermark) = planner().plan_at(&repo, now.clone()).unwrap();
        assert_eq!(watermark.as_str(), "2026-08-31T11:55:00Z", "five minutes by default");

        // The overlap is configurable, and can be switched off by a partner who has measured it.
        let strict = Planner::new(
            PlannerConfig::new("DE-DCB".parse().unwrap(), GeoCoordinatesFormat::Google)
                .with_clock_skew_guard(Duration::ZERO),
        );
        let (_, watermark) = strict.plan_at(&repo, now.clone()).unwrap();
        assert_eq!(watermark, now);
    }

    #[test]
    fn begin_empties_the_copy_for_a_full_pull_and_leaves_it_alone_for_a_delta() {
        // A full pull is the whole world, so what is not in it has been withdrawn: a copy that is
        // not emptied first keeps withdrawn charging points forever.
        let mut repo = populated();
        let now: DateTime = "2026-08-31T12:00:00.000Z".parse().unwrap();

        let (plan, _) = planner().begin_at(&mut repo, now.clone()).unwrap();
        assert!(plan.replaces_everything());
        assert_eq!(repo.len().unwrap(), 0, "a full pull starts from an empty copy");

        // A delta builds on what is there, so nothing is thrown away.
        repo.upsert(samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
        repo.set_last_call("2026-08-31T11:00:00.000Z".parse().unwrap()).unwrap();
        let (plan, _) = planner().begin_at(&mut repo, now).unwrap();
        assert!(!plan.replaces_everything());
        assert_eq!(repo.len().unwrap(), 1);
    }

    #[test]
    fn a_stale_watermark_forces_a_rebaseline() {
        let mut repo = populated();
        repo.set_last_call("2026-08-01T11:00:00.000Z".parse().unwrap()).unwrap();
        let now: DateTime = "2026-08-31T12:00:00.000Z".parse().unwrap();

        let (plan, _) = planner().plan_at(&repo, now).unwrap();
        assert!(matches!(plan, Plan::Full { reason: FullPullReason::WatermarkTooOld, .. }));
    }

    #[test]
    fn every_full_pull_reason_says_something_different() {
        // A full pull of Europe is expensive and the log line is the only explanation an operator
        // gets. Five reasons that all render the same are one reason.
        let reasons = [
            FullPullReason::NoWatermark,
            FullPullReason::EmptyRepository,
            FullPullReason::WatermarkTooOld,
            FullPullReason::WatermarkInFuture,
            FullPullReason::Requested,
        ];
        let rendered: Vec<String> = reasons.iter().map(ToString::to_string).collect();
        for text in &rendered {
            assert!(!text.is_empty(), "a reason rendered as nothing");
        }
        let mut unique = rendered.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), reasons.len(), "two reasons render the same: {rendered:?}");
    }

    #[test]
    fn the_maximum_delta_age_is_a_boundary_not_a_vibe() {
        // Exactly at the limit is still usable; one second past it is not. A `>` written as a `>=`
        // costs a full pull of Europe every time the job runs on the hour.
        let mut repo = populated();
        repo.set_last_call("2026-08-31T12:00:00.000Z".parse().unwrap()).unwrap();
        let planner = Planner::new(
            PlannerConfig::new("DE-DCB".parse().unwrap(), GeoCoordinatesFormat::Google)
                .with_max_delta_age(Duration::from_secs(3600)),
        );

        let (plan, _) = planner.plan_at(&repo, "2026-08-31T13:00:00.000Z".parse().unwrap()).unwrap();
        assert!(matches!(plan, Plan::Delta { .. }), "exactly one hour old is still a delta");

        let (plan, _) = planner.plan_at(&repo, "2026-08-31T13:00:01.000Z".parse().unwrap()).unwrap();
        assert!(matches!(plan, Plan::Full { reason: FullPullReason::WatermarkTooOld, .. }));
    }

    #[test]
    fn a_watermark_from_the_future_forces_a_rebaseline() {
        let mut repo = populated();
        // A clock that jumped forward and back: a delta from here returns nothing, forever.
        repo.set_last_call("2026-09-30T11:00:00.000Z".parse().unwrap()).unwrap();
        let now: DateTime = "2026-08-31T12:00:00.000Z".parse().unwrap();

        let (plan, _) = planner().plan_at(&repo, now).unwrap();
        assert!(matches!(plan, Plan::Full { reason: FullPullReason::WatermarkInFuture, .. }));
    }

    #[test]
    fn a_watermark_without_a_copy_forces_a_full_pull() {
        let mut repo = InMemoryEvseRepository::new();
        repo.set_last_call("2026-08-31T11:00:00.000Z".parse().unwrap()).unwrap();
        let now: DateTime = "2026-08-31T12:00:00.000Z".parse().unwrap();

        let (plan, _) = planner().plan_at(&repo, now).unwrap();
        assert!(matches!(plan, Plan::Full { reason: FullPullReason::EmptyRepository, .. }));
    }

    #[test]
    fn a_rebaseline_can_be_asked_for() {
        let mut repo = populated();
        repo.set_last_call("2026-08-31T11:00:00.000Z".parse().unwrap()).unwrap();
        let planner = Planner::new(
            PlannerConfig::new("DE-DCB".parse().unwrap(), GeoCoordinatesFormat::Google).rebaseline(),
        );
        let (plan, _) = planner.plan_at(&repo, "2026-08-31T12:00:00.000Z".parse().unwrap()).unwrap();
        assert!(matches!(plan, Plan::Full { reason: FullPullReason::Requested, .. }));
    }

    #[test]
    fn the_watermark_moves_only_on_commit() {
        let mut repo = populated();
        let now: DateTime = "2026-08-31T12:00:00.000Z".parse().unwrap();
        let planner = planner();

        let (_, watermark) = planner.plan_at(&repo, now).unwrap();
        assert_eq!(repo.last_call().unwrap(), None, "planning does not move the watermark");

        planner.commit(&mut repo, watermark.clone()).unwrap();
        assert_eq!(repo.last_call().unwrap(), Some(watermark));
    }
}
