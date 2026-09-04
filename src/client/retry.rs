//! `RetryPolicy` — retries that cannot start a charging session twice.

use core::time::Duration;

use crate::transport::{OicpError, Operation};

/// When to try again, and how long to wait.
///
/// # The rule that matters
///
/// Retrying an OICP call is not always safe, and the difference is not the HTTP status:
///
/// * A **transport failure** may mean the request never arrived, or that it arrived and the answer
///   was lost. For an `AuthorizeRemoteStart` the second case means retrying could start a second
///   charging session. So retries are allowed only where the operation is idempotent, or where
///   Hubject's own session handling makes a duplicate harmless.
/// * A **rejection** (`HTTP 200`, `Result: false`) is a decision, and repeating an identical
///   request gets an identical decision — except for the handful of transient codes that
///   [`Code::is_retryable`](crate::types::Code::is_retryable) names.
///
/// [`RetryPolicy::should_retry`] takes the operation as well as the error, so it can apply both.
///
/// ```
/// # use oicp_kit::client::RetryPolicy;
/// # use oicp_kit::transport::{OicpError, Operation};
/// let policy = RetryPolicy::default();
/// let lost = OicpError::transport("connection reset");
///
/// // A push is idempotent: send it again.
/// assert!(policy.should_retry(Operation::PushEvseData, &lost, 0).is_some());
///
/// // A remote start is not: a second one could start a second session.
/// assert!(policy.should_retry(Operation::AuthorizeRemoteStart, &lost, 0).is_none());
/// ```
#[derive(Clone, Debug)]
pub struct RetryPolicy {
    /// How many times to try again after the first attempt.
    pub max_retries: u32,
    /// The wait before the first retry.
    pub initial_backoff: Duration,
    /// The cap on the wait.
    pub max_backoff: Duration,
    /// How much of the backoff is random, as a percentage of it — 0 for none.
    ///
    /// Without jitter, every client that lost the same Hubject instance retries in lockstep and
    /// arrives together, which is how a recovering service is knocked over again.
    pub jitter_percent: u32,
    /// Retry operations that are not idempotent.
    ///
    /// Off by default. Turn it on only if your own system deduplicates on the partner session id.
    pub retry_non_idempotent: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(250),
            max_backoff: Duration::from_secs(10),
            jitter_percent: 25,
            retry_non_idempotent: false,
        }
    }
}

impl RetryPolicy {
    /// A policy that never retries.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            max_retries: 0,
            initial_backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
            jitter_percent: 0,
            retry_non_idempotent: false,
        }
    }

    /// Sets how many retries to allow.
    #[must_use]
    pub const fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Allows retrying operations that are not idempotent. See the field documentation.
    #[must_use]
    pub const fn allowing_non_idempotent(mut self) -> Self {
        self.retry_non_idempotent = true;
        self
    }

    /// How long to wait before attempt `attempt + 1`, or `None` to give up.
    ///
    /// `attempt` counts from zero for the first failure.
    #[must_use]
    pub fn should_retry(&self, operation: Operation, error: &OicpError, attempt: u32) -> Option<Duration> {
        if attempt >= self.max_retries || !error.is_retryable() {
            return None;
        }
        if !self.retry_non_idempotent && !is_idempotent(operation) {
            return None;
        }
        Some(self.backoff(attempt))
    }

    /// The backoff for `attempt`, doubling and capped, before jitter.
    #[must_use]
    pub fn backoff(&self, attempt: u32) -> Duration {
        let doubled = self.initial_backoff.saturating_mul(1u32 << attempt.min(16));
        doubled.min(self.max_backoff)
    }

    /// The backoff for `attempt` with jitter applied, given a position in `0..=1000`.
    ///
    /// `500` is the un-jittered backoff, `0` the earliest and `1000` the latest the spread
    /// allows. Split out so the jitter is testable without a random source — and computed in
    /// integer milliseconds, so the crate needs neither a floating-point multiply nor a
    /// random-number dependency to spread its retries.
    #[must_use]
    pub fn jittered_backoff(&self, attempt: u32, position_permille: u32) -> Duration {
        let base = self.backoff(attempt);
        if self.jitter_percent == 0 {
            return base;
        }
        let base_ms = base.as_millis();
        let span = base_ms * u128::from(self.jitter_percent.min(100)) / 100;
        // `position` walks the whole spread: 0 → base - span, 500 → base, 1000 → base + span.
        let offset = span * u128::from(position_permille.min(1000)) / 500;
        let millis = (base_ms + offset).saturating_sub(span);
        let jittered = u64::try_from(millis).map_or(self.max_backoff, Duration::from_millis);
        jittered.min(self.max_backoff)
    }
}

/// A position in `0..=1000` for the next retry's jitter.
///
/// `RandomState` seeds itself from the operating system, so this needs no random-number crate:
/// jitter exists to stop a fleet of clients retrying in lockstep, and any well-spread value does
/// that. Nothing here is a secret.
pub(crate) fn random_permille() -> u32 {
    use std::hash::{BuildHasher as _, RandomState};
    u32::try_from(RandomState::new().hash_one(0_u8) % 1001).unwrap_or(500)
}

/// Whether sending this operation twice is harmless.
///
/// The four that are not: starting or stopping a session, and starting or stopping a reservation.
/// Each of them changes the state of a physical charging point, and a duplicate can leave a driver
/// with a session they did not ask for or a reservation they will be billed for.
///
/// Everything else is a push of state (idempotent by construction), a pull (a read), or a record
/// of something that already happened, which Hubject deduplicates on the session id.
const fn is_idempotent(operation: Operation) -> bool {
    !matches!(
        operation,
        Operation::AuthorizeRemoteStart
            | Operation::AuthorizeRemoteStop
            | Operation::AuthorizeRemoteReservationStart
            | Operation::AuthorizeRemoteReservationStop
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Acknowledgement, Code};

    #[test]
    fn a_lost_push_is_retried_and_a_lost_remote_start_is_not() {
        let policy = RetryPolicy::default();
        let lost = OicpError::transport("connection reset");

        assert!(policy.should_retry(Operation::PushEvseData, &lost, 0).is_some());
        assert!(policy.should_retry(Operation::ChargeDetailRecord, &lost, 0).is_some());
        assert!(policy.should_retry(Operation::PullEvseData, &lost, 0).is_some());

        // Four operations change the state of a charging point.
        for op in [
            Operation::AuthorizeRemoteStart,
            Operation::AuthorizeRemoteStop,
            Operation::AuthorizeRemoteReservationStart,
            Operation::AuthorizeRemoteReservationStop,
        ] {
            assert!(policy.should_retry(op, &lost, 0).is_none(), "{op:?} must not be retried by default");
        }
    }

    #[test]
    fn a_decision_is_never_retried_however_the_transport_went() {
        let policy = RetryPolicy::default();
        let refused = OicpError::rejected(Acknowledgement::failure(Code::NoValidContract));
        assert!(policy.should_retry(Operation::AuthorizeStart, &refused, 0).is_none());

        // …but a transient code is.
        let unavailable = OicpError::rejected(Acknowledgement::failure(Code::ServiceNotAvailable));
        assert!(policy.should_retry(Operation::AuthorizeStart, &unavailable, 0).is_some());
    }

    #[test]
    fn retries_stop_at_the_limit() {
        let policy = RetryPolicy::default().with_max_retries(2);
        let lost = OicpError::transport("reset");
        assert!(policy.should_retry(Operation::PushEvseData, &lost, 0).is_some());
        assert!(policy.should_retry(Operation::PushEvseData, &lost, 1).is_some());
        assert!(policy.should_retry(Operation::PushEvseData, &lost, 2).is_none());
    }

    #[test]
    fn backoff_doubles_and_is_capped() {
        let policy = RetryPolicy {
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_millis(500),
            ..RetryPolicy::default()
        };
        assert_eq!(policy.backoff(0), Duration::from_millis(100));
        assert_eq!(policy.backoff(1), Duration::from_millis(200));
        assert_eq!(policy.backoff(2), Duration::from_millis(400));
        assert_eq!(policy.backoff(3), Duration::from_millis(500), "capped");
        assert_eq!(policy.backoff(30), Duration::from_millis(500), "no overflow");
    }

    #[test]
    fn jitter_spreads_the_retries_around_the_backoff() {
        let policy = RetryPolicy { jitter_percent: 50, ..RetryPolicy::default() };
        let base = policy.backoff(0); // 250 ms
        let early = policy.jittered_backoff(0, 0);
        let late = policy.jittered_backoff(0, 1000);

        assert!(early < base && late > base, "jitter must move the wait in both directions");
        assert_eq!(policy.jittered_backoff(0, 500), base, "the midpoint is the base");
        assert_eq!(early, Duration::from_millis(125), "half of 250 ms early");
        assert_eq!(late, Duration::from_millis(375), "half of 250 ms late");

        // Off means off.
        let none = RetryPolicy { jitter_percent: 0, ..RetryPolicy::default() };
        assert_eq!(none.jittered_backoff(0, 0), none.backoff(0));
    }

    #[test]
    fn jitter_never_leaves_the_spread_or_the_cap() {
        let policy = RetryPolicy { jitter_percent: 100, ..RetryPolicy::default() };
        let base = policy.backoff(0);
        for position in 0..=1000 {
            let wait = policy.jittered_backoff(0, position);
            assert!(wait <= base * 2, "{position} produced {wait:?}");
            assert!(wait <= policy.max_backoff, "{position} exceeded the cap");
        }
    }

    #[test]
    fn the_random_position_stays_in_range() {
        for _ in 0..1000 {
            assert!(random_permille() <= 1000);
        }
    }

    #[test]
    fn non_idempotent_retries_can_be_opted_into() {
        let policy = RetryPolicy::default().allowing_non_idempotent();
        let lost = OicpError::transport("reset");
        assert!(policy.should_retry(Operation::AuthorizeRemoteStart, &lost, 0).is_some());
    }

    #[test]
    fn the_none_policy_never_retries() {
        let policy = RetryPolicy::none();
        assert!(policy.should_retry(Operation::PushEvseData, &OicpError::transport("x"), 0).is_none());
    }
}
