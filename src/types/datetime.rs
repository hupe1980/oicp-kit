//! `DateTime` — an OICP timestamp, and `HourMinute` for opening times.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::validate::{Validate, Validator, ViolationCode};

/// An OICP timestamp: an instant, in UTC, written as RFC 3339.
///
/// The OpenAPI sources type every timestamp as `string`/`format: date-time`, and every example
/// Hubject gives is UTC with milliseconds — `2020-09-23T14:17:53.038Z`. This type parses the full
/// RFC 3339 grammar (so an offset like `+02:00` is accepted and normalised to the same instant)
/// and writes back **the exact text that arrived**, so a CDR that a CPO signed is forwarded
/// byte-identically.
///
/// ```
/// use oicp_kit::types::DateTime;
///
/// let t: DateTime = "2020-09-23T14:17:53.038Z".parse()?;
/// assert_eq!(t.to_string(), "2020-09-23T14:17:53.038Z"); // not re-formatted
///
/// // An offset is understood, and compares as the same instant.
/// let same: DateTime = "2020-09-23T16:17:53.038+02:00".parse()?;
/// assert_eq!(t, same);
/// # Ok::<(), oicp_kit::types::DateTimeError>(())
/// ```
#[derive(Clone, Debug)]
pub struct DateTime {
    /// The instant, when the text parsed. `None` for a value that arrived malformed — which is
    /// preserved rather than replaced by a plausible-looking default, because an unreadable
    /// timestamp that silently reads as 1970 is worse than one that says it cannot be read.
    at: Option<OffsetDateTime>,
    /// The text as it arrived, so a re-serialised object is byte-identical.
    raw: String,
}

/// Why a string is not an OICP timestamp.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{value:?} is not an RFC 3339 timestamp: {reason}")]
pub struct DateTimeError {
    value: String,
    reason: String,
}

impl DateTime {
    /// Parses an RFC 3339 timestamp, keeping its text verbatim.
    ///
    /// # Errors
    ///
    /// Returns [`DateTimeError`] when the value is not RFC 3339.
    pub fn new(value: impl Into<String>) -> Result<Self, DateTimeError> {
        let raw = value.into();
        match OffsetDateTime::parse(&raw, &Rfc3339) {
            Ok(at) => Ok(Self { at: Some(at), raw }),
            Err(e) => Err(DateTimeError { value: raw, reason: e.to_string() }),
        }
    }

    /// Accepts `value` without parsing it; [`Validate`] reports it.
    ///
    /// A malformed timestamp on one record of a page must not fail the page.
    #[must_use]
    pub fn new_unchecked(value: impl Into<String>) -> Self {
        let raw = value.into();
        let at = OffsetDateTime::parse(&raw, &Rfc3339).ok();
        Self { at, raw }
    }

    /// Wraps an [`OffsetDateTime`], writing it as RFC 3339 in UTC.
    #[must_use]
    pub fn from_offset(at: OffsetDateTime) -> Self {
        let at = at.to_offset(time::UtcOffset::UTC);
        let raw = at.format(&Rfc3339).unwrap_or_default();
        Self { at: Some(at), raw }
    }

    /// The instant this timestamp denotes, or `None` when the text is not RFC 3339.
    ///
    /// Every arithmetic on a timestamp — a duration, an ordering, a window — is only meaningful
    /// when the text parsed, so the question is asked here rather than answered with a default.
    #[must_use]
    pub const fn as_offset(&self) -> Option<OffsetDateTime> {
        self.at
    }

    /// The timestamp exactly as it appears on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Whether the value parsed as RFC 3339.
    #[must_use]
    pub const fn is_well_formed(&self) -> bool {
        self.at.is_some()
    }

    /// The current instant.
    #[must_use]
    pub fn now() -> Self {
        Self::from_offset(OffsetDateTime::now_utc())
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

/// Two timestamps are equal when they denote the same instant, whatever offset they were written
/// in. A malformed value compares by its text, so two different malformed values stay distinct.
impl PartialEq for DateTime {
    fn eq(&self, other: &Self) -> bool {
        match (self.at, other.at) {
            (Some(a), Some(b)) => a == b,
            _ => self.raw == other.raw,
        }
    }
}
impl Eq for DateTime {}

impl core::hash::Hash for DateTime {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        match self.at {
            Some(at) => at.hash(state),
            None => self.raw.hash(state),
        }
    }
}

impl PartialOrd for DateTime {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Ordering agrees with equality, which `Ord` requires and a `BTreeMap` of timestamps depends on.
///
/// Two readable timestamps order by instant. A readable one sorts before an unreadable one, and
/// two unreadable ones order by their text — so `cmp(a, b) == Equal` holds exactly when `a == b`.
/// Ordering every unparseable value as the epoch, which is the obvious shortcut, makes two
/// different malformed timestamps compare `Equal` while `==` says they differ; a `BTreeMap` then
/// loses one of them.
impl Ord for DateTime {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        match (self.at, other.at) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => core::cmp::Ordering::Less,
            (None, Some(_)) => core::cmp::Ordering::Greater,
            (None, None) => self.raw.cmp(&other.raw),
        }
    }
}

impl From<OffsetDateTime> for DateTime {
    fn from(at: OffsetDateTime) -> Self {
        Self::from_offset(at)
    }
}

impl TryFrom<DateTime> for OffsetDateTime {
    type Error = DateTimeError;
    fn try_from(t: DateTime) -> Result<Self, Self::Error> {
        t.at.ok_or(DateTimeError { value: t.raw, reason: "not an RFC 3339 timestamp".to_owned() })
    }
}

impl FromStr for DateTime {
    type Err = DateTimeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Validate for DateTime {
    fn validate_in(&self, v: &mut Validator) {
        if self.at.is_none() {
            v.report(ViolationCode::PatternMismatch, format!("{:?} is not an RFC 3339 timestamp", self.raw));
        }
    }
}

impl Serialize for DateTime {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for DateTime {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::new_unchecked(String::deserialize(d)?))
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for DateTime {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DateTime".into()
    }
    fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "format": "date-time" })
    }
}

/// A time of day as `HH:MM`, used by opening times and pricing availability windows.
///
/// Spec pattern: `[0-9]{2}:[0-9]{2}`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HourMinute(String);

impl HourMinute {
    /// Parses `HH:MM`.
    ///
    /// # Errors
    ///
    /// Returns [`DateTimeError`] when the value is not two digits, a colon and two digits, or
    /// when the hour or minute is out of range.
    pub fn new(value: impl Into<String>) -> Result<Self, DateTimeError> {
        let raw = value.into();
        Self::check(&raw).map(|()| Self(raw))
    }

    /// Accepts `value` without checking it; [`Validate`] reports it.
    #[must_use]
    pub fn new_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Builds a time from its parts.
    ///
    /// # Errors
    ///
    /// Returns [`DateTimeError`] when `hour > 23` or `minute > 59`.
    pub fn from_hm(hour: u8, minute: u8) -> Result<Self, DateTimeError> {
        Self::new(format!("{hour:02}:{minute:02}"))
    }

    /// The value as it appears on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Minutes since midnight, for ordering and overlap checks.
    #[must_use]
    pub fn minutes_of_day(&self) -> Option<u16> {
        let (h, m) = self.0.split_once(':')?;
        Some(u16::from(h.parse::<u8>().ok()?) * 60 + u16::from(m.parse::<u8>().ok()?))
    }

    /// Whether the value satisfies the specification's pattern.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        Self::check(&self.0).is_ok()
    }

    fn check(s: &str) -> Result<(), DateTimeError> {
        let err = |reason: &str| DateTimeError { value: s.to_owned(), reason: reason.to_owned() };
        let (h, m) = s.split_once(':').ok_or_else(|| err("expected HH:MM"))?;
        if h.len() != 2 || m.len() != 2 || !h.bytes().chain(m.bytes()).all(|c| c.is_ascii_digit()) {
            return Err(err("expected two digits, a colon and two digits"));
        }
        let hour: u8 = h.parse().map_err(|_| err("the hour is not a number"))?;
        let minute: u8 = m.parse().map_err(|_| err("the minute is not a number"))?;
        // 24:00 is a common way to write "end of day" and Hubject accepts it in opening times.
        if hour > 24 || minute > 59 || (hour == 24 && minute != 0) {
            return Err(err("the time is out of range"));
        }
        Ok(())
    }
}

impl fmt::Display for HourMinute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for HourMinute {
    type Err = DateTimeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Validate for HourMinute {
    fn validate_in(&self, v: &mut Validator) {
        if let Err(e) = Self::check(&self.0) {
            v.report(ViolationCode::PatternMismatch, e.to_string());
        }
    }
}

impl Serialize for HourMinute {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for HourMinute {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::new_unchecked(String::deserialize(d)?))
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for HourMinute {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "HourMinute".into()
    }
    fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "pattern": "[0-9]{2}:[0-9]{2}" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_written_back_exactly_as_they_arrived() {
        for text in ["2020-09-23T14:17:53.038Z", "2018-01-23T14:04:29.377Z", "2021-01-23T14:21:36.954Z"] {
            let t: DateTime = text.parse().unwrap();
            assert_eq!(serde_json::to_string(&t).unwrap(), format!("\"{text}\""));
        }
    }

    #[test]
    fn an_offset_denotes_the_same_instant_as_its_utc_form() {
        let utc: DateTime = "2020-09-23T14:17:53.038Z".parse().unwrap();
        let offset: DateTime = "2020-09-23T16:17:53.038+02:00".parse().unwrap();
        assert_eq!(utc, offset);
        // …and neither is rewritten.
        assert_eq!(offset.to_string(), "2020-09-23T16:17:53.038+02:00");
    }

    #[test]
    fn a_malformed_timestamp_decodes_and_is_reported() {
        let t: DateTime = serde_json::from_str(r#""23.09.2020 14:17""#).unwrap();
        assert!(!t.is_well_formed());
        assert_eq!(t.validate().unwrap_err().as_slice()[0].code, ViolationCode::PatternMismatch);
        assert_eq!(serde_json::to_string(&t).unwrap(), r#""23.09.2020 14:17""#);
    }

    #[test]
    fn hour_minute_checks_the_pattern_and_the_range() {
        // Both ends of every bound, because a `>` that should be `>=` passes any test that only
        // probes the middle: 23:59 and 24:00 are legal and 24:01 is not.
        for good in ["08:30", "00:00", "23:59", "24:00"] {
            let value: HourMinute = good.parse().unwrap_or_else(|e| panic!("{good}: {e}"));
            assert_eq!(value.as_str(), good, "the text is kept");
            assert_eq!(value.to_string(), good);
            assert!(value.is_well_formed());
            assert!(value.validate().is_ok());
        }
        assert_eq!("08:30".parse::<HourMinute>().unwrap().minutes_of_day(), Some(510));
        assert_eq!("24:00".parse::<HourMinute>().unwrap().minutes_of_day(), Some(1440));

        for bad in ["8:30", "0830", "25:00", "08:60", "24:01", "", "0a:30"] {
            assert!(bad.parse::<HourMinute>().is_err(), "{bad} should be rejected");
            let arrived = HourMinute::new_unchecked(bad);
            assert!(!arrived.is_well_formed(), "{bad}");
            assert!(arrived.validate().is_err(), "{bad} decodes but must be reported");
            assert_eq!(arrived.as_str(), bad, "and its text survives");
        }
    }

    #[test]
    fn equality_hashing_and_ordering_agree_with_each_other() {
        use std::hash::{BuildHasher as _, RandomState};
        let hasher = RandomState::new();

        let utc: DateTime = "2020-09-23T14:17:53.038Z".parse().unwrap();
        let offset: DateTime = "2020-09-23T16:17:53.038+02:00".parse().unwrap();
        let later: DateTime = "2020-09-23T14:17:53.039Z".parse().unwrap();
        let junk_a = DateTime::new_unchecked("23.09.2020");
        let junk_b = DateTime::new_unchecked("nope");

        // The same instant written two ways is one instant, and hashes as one.
        assert_eq!(utc, offset);
        assert_eq!(hasher.hash_one(&utc), hasher.hash_one(&offset));
        assert_eq!(utc.cmp(&offset), core::cmp::Ordering::Equal);

        assert_ne!(utc, later);
        assert!(utc < later);
        // …and values that differ hash apart. Without this, a `Hash` that hashes *nothing*
        // satisfies every assertion above — every key collides, the map degrades to a list, and
        // no test says a word.
        assert_ne!(hasher.hash_one(&utc), hasher.hash_one(&later));

        // Two different unreadable values stay different — under `Eq` *and* under `Ord`. Ordering
        // them both as the epoch, which reading the instant alone would do, makes them compare
        // `Equal` while `==` says they differ, and a `BTreeMap` then loses one.
        assert_ne!(junk_a, junk_b);
        assert_ne!(junk_a.cmp(&junk_b), core::cmp::Ordering::Equal);
        assert_eq!(junk_a, DateTime::new_unchecked("23.09.2020"));
        assert_eq!(hasher.hash_one(&junk_a), hasher.hash_one(DateTime::new_unchecked("23.09.2020")));
        assert_ne!(hasher.hash_one(&junk_a), hasher.hash_one(&junk_b));

        // A readable value never collides with an unreadable one, in either relation.
        assert_ne!(utc, junk_a);
        assert!(utc < junk_a);

        // The whole contract, over every pair.
        let all = [&utc, &offset, &later, &junk_a, &junk_b];
        for a in all {
            for b in all {
                assert_eq!(
                    a.cmp(b) == core::cmp::Ordering::Equal,
                    a == b,
                    "Ord and Eq disagree on {a} vs {b}"
                );
            }
        }
    }

    #[cfg(feature = "schema")]
    #[test]
    fn the_published_schema_says_what_these_types_are_on_the_wire() {
        // A partner generating a client from `oicp schema` gets whatever this returns. An empty
        // schema validates everything, which is the one answer that helps nobody.
        let mut generator = schemars::SchemaGenerator::default();
        let timestamp = <DateTime as schemars::JsonSchema>::json_schema(&mut generator);
        let json = serde_json::to_value(&timestamp).unwrap();
        assert_eq!(json["type"], "string");
        assert_eq!(json["format"], "date-time");

        let hour_minute = <HourMinute as schemars::JsonSchema>::json_schema(&mut generator);
        let json = serde_json::to_value(&hour_minute).unwrap();
        assert_eq!(json["type"], "string");
        assert_eq!(json["pattern"], "[0-9]{2}:[0-9]{2}");
    }

    #[test]
    fn an_unreadable_timestamp_has_no_instant() {
        let broken = DateTime::new_unchecked("23.09.2020 14:17");
        assert_eq!(broken.as_offset(), None, "and does not silently read as the epoch");
        assert!(OffsetDateTime::try_from(broken).is_err());

        let good: DateTime = "2020-09-23T14:17:53.038Z".parse().unwrap();
        assert!(good.as_offset().is_some());
    }
}
