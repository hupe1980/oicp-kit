//! Answering "is this charging point open right now?" from what OICP actually carries.

use time::{OffsetDateTime, UtcOffset, Weekday};

use super::common::{DaySelection, OpeningTimes};
use super::datetime::DateTime;

/// Whether a charging point is open, and why it is or is not.
///
/// `Unknown` is a real answer rather than a failure: it means the record does not carry enough to
/// decide, and an EMP routing a driver should treat that differently from a definite "closed".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opening {
    /// Open at the instant asked about.
    Open,
    /// Closed at the instant asked about.
    Closed,
    /// The record does not say — no opening times, or a time zone that could not be read.
    Unknown(UnknownReason),
}

/// Why an opening question could not be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnknownReason {
    /// The record is not open around the clock but carries no opening times.
    NoOpeningTimes,
    /// The address carries no time zone, so a local time cannot be derived.
    ///
    /// Every window in `OpeningTimes` is a **local** time. Without the offset, `08:00` could be any
    /// of twenty-six instants, and guessing UTC would tell a driver in Lisbon that a Berlin
    /// charging point opens an hour late.
    NoTimeZone,
    /// The time zone is present but not of the form `UTC+HH:MM`.
    MalformedTimeZone,
    /// A period in the opening times does not parse.
    MalformedPeriod,
    /// The instant asked about is not a readable timestamp.
    MalformedInstant,
}

impl Opening {
    /// Whether the answer is a definite yes.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }

    /// Whether the answer is a definite no.
    #[must_use]
    pub const fn is_closed(self) -> bool {
        matches!(self, Self::Closed)
    }

    /// Whether the record did not say.
    #[must_use]
    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown(_))
    }
}

/// Parses OICP's `UTC+HH:MM` / `UTC-HH:MM` time-zone field.
///
/// OICP carries a **fixed offset**, not an IANA zone name — so this is exact for the offset given,
/// and takes no view on daylight saving. A CPO that publishes `UTC+01:00` all year is telling
/// every EMP that its Berlin charging points open an hour late in summer; that is a property of
/// the protocol, not something a library can fix, and it is worth knowing about.
fn parse_offset(time_zone: Option<&str>) -> Result<UtcOffset, UnknownReason> {
    let tz = time_zone.ok_or(UnknownReason::NoTimeZone)?;
    let bytes = tz.as_bytes();
    if bytes.len() != 9 || &bytes[..3] != b"UTC" || bytes[6] != b':' {
        return Err(UnknownReason::MalformedTimeZone);
    }
    let sign: i8 = match bytes[3] {
        b'+' => 1,
        b'-' => -1,
        _ => return Err(UnknownReason::MalformedTimeZone),
    };
    let hours: i8 = tz[4..6].parse().map_err(|_| UnknownReason::MalformedTimeZone)?;
    let minutes: i8 = tz[7..9].parse().map_err(|_| UnknownReason::MalformedTimeZone)?;
    if hours > 14 || minutes > 59 {
        return Err(UnknownReason::MalformedTimeZone);
    }
    UtcOffset::from_hms(sign * hours, sign * minutes, 0).map_err(|_| UnknownReason::MalformedTimeZone)
}

/// Whether `selection` covers `weekday`.
const fn covers(selection_matches: (bool, bool, bool), weekday: Weekday) -> bool {
    let (everyday, workdays, weekend) = selection_matches;
    if everyday {
        return true;
    }
    let is_weekend = matches!(weekday, Weekday::Saturday | Weekday::Sunday);
    (workdays && !is_weekend) || (weekend && is_weekend)
}

fn day_matches(selection: &DaySelection, weekday: Weekday) -> bool {
    let named = match selection {
        DaySelection::Monday => Some(Weekday::Monday),
        DaySelection::Tuesday => Some(Weekday::Tuesday),
        DaySelection::Wednesday => Some(Weekday::Wednesday),
        DaySelection::Thursday => Some(Weekday::Thursday),
        DaySelection::Friday => Some(Weekday::Friday),
        DaySelection::Saturday => Some(Weekday::Saturday),
        DaySelection::Sunday => Some(Weekday::Sunday),
        _ => None,
    };
    if let Some(day) = named {
        return day == weekday;
    }
    covers(
        (
            matches!(selection, DaySelection::Everyday),
            matches!(selection, DaySelection::Workdays),
            matches!(selection, DaySelection::Weekend),
        ),
        weekday,
    )
}

/// Decides whether a charging point is open at `at`.
///
/// # What this needs
///
/// * `is_open_24_hours` — if true, the answer is always [`Opening::Open`].
/// * `opening_times` — the windows, which are **local** times.
/// * `time_zone` — the address's `UTC±HH:MM`, without which a local time cannot be derived.
///
/// # Overnight windows
///
/// A window whose end is before its begin (`22:00`–`02:00`) runs past midnight. That is common for
/// a station inside a parking garage, and reading it as an empty window would report the station
/// closed exactly when it is open.
///
/// The half after midnight belongs to **the day the window opened on**, not to the day it lands
/// in. `Monday 22:00–02:00` is open at 01:00 on *Tuesday* and closed at 01:00 on Monday, so each
/// day is asked two questions: does a window that starts today cover this minute, and does a
/// window that started yesterday still run into it. Checking only the first — which is what
/// reading the day selection once does — reports a garage open on the wrong night and closed on
/// the right one, and no test over an `Everyday` window can tell the difference.
pub(crate) fn is_open_at(
    is_open_24_hours: bool,
    opening_times: Option<&Vec<OpeningTimes>>,
    time_zone: Option<&str>,
    at: &DateTime,
) -> Opening {
    if is_open_24_hours {
        return Opening::Open;
    }
    let Some(times) = opening_times.filter(|t| !t.is_empty()) else {
        return Opening::Unknown(UnknownReason::NoOpeningTimes);
    };
    let offset = match parse_offset(time_zone) {
        Ok(offset) => offset,
        Err(reason) => return Opening::Unknown(reason),
    };

    let Some(instant) = at.as_offset() else {
        return Opening::Unknown(UnknownReason::MalformedInstant);
    };
    let local: OffsetDateTime = instant.to_offset(offset);
    let weekday = local.weekday();
    let yesterday = weekday.previous();
    let minutes = u16::from(local.hour()) * 60 + u16::from(local.minute());

    let mut malformed = false;
    for entry in times {
        let opens_today = day_matches(&entry.on, weekday);
        let opened_yesterday = day_matches(&entry.on, yesterday);
        if !opens_today && !opened_yesterday {
            continue;
        }
        for period in &entry.period {
            let (Some(begin), Some(end)) = (period.begin.minutes_of_day(), period.end.minutes_of_day())
            else {
                malformed = true;
                continue;
            };
            let open = if begin <= end {
                opens_today && (begin..end).contains(&minutes)
            } else {
                // Overnight. The evening half belongs to the day the window names; the morning
                // half belongs to the day after it.
                (opens_today && minutes >= begin) || (opened_yesterday && minutes < end)
            };
            if open {
                return Opening::Open;
            }
        }
    }

    if malformed { Opening::Unknown(UnknownReason::MalformedPeriod) } else { Opening::Closed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Extensions, HourMinute, Period};

    fn window(on: DaySelection, begin: &str, end: &str) -> OpeningTimes {
        OpeningTimes {
            period: vec![Period {
                begin: HourMinute::new(begin).expect("valid"),
                end: HourMinute::new(end).expect("valid"),
            }],
            on,
            extensions: Extensions::new(),
        }
    }

    /// 2026-08-31 is a Monday.
    fn monday_at(utc: &str) -> DateTime {
        format!("2026-08-31T{utc}:00.000Z").parse().expect("valid")
    }

    #[test]
    fn around_the_clock_is_always_open() {
        assert_eq!(is_open_at(true, None, None, &monday_at("03:00")), Opening::Open);
    }

    #[test]
    fn a_local_window_is_evaluated_in_the_stations_own_time_zone() {
        let times = vec![window(DaySelection::Workdays, "08:00", "18:00")];

        // 07:00 UTC is 09:00 in Berlin: open. Reading it as UTC would say closed.
        assert_eq!(is_open_at(false, Some(&times), Some("UTC+02:00"), &monday_at("07:00")), Opening::Open);
        // 05:00 UTC is 07:00 in Berlin: not yet.
        assert_eq!(is_open_at(false, Some(&times), Some("UTC+02:00"), &monday_at("05:00")), Opening::Closed);
        // The same instant in a station that publishes UTC+00:00 is a different answer.
        assert_eq!(is_open_at(false, Some(&times), Some("UTC+00:00"), &monday_at("07:00")), Opening::Closed);
    }

    #[test]
    fn a_negative_offset_works_too() {
        let times = vec![window(DaySelection::Everyday, "08:00", "18:00")];
        // 15:00 UTC is 10:00 in UTC-05:00.
        assert_eq!(is_open_at(false, Some(&times), Some("UTC-05:00"), &monday_at("15:00")), Opening::Open);
        assert_eq!(is_open_at(false, Some(&times), Some("UTC-05:00"), &monday_at("06:00")), Opening::Closed);
    }

    #[test]
    fn an_overnight_window_stays_open_past_midnight() {
        let times = vec![window(DaySelection::Everyday, "22:00", "02:00")];
        let at = |utc: &str| is_open_at(false, Some(&times), Some("UTC+00:00"), &monday_at(utc));

        assert_eq!(at("23:00"), Opening::Open, "before midnight");
        assert_eq!(at("01:00"), Opening::Open, "after midnight");
        assert_eq!(at("12:00"), Opening::Closed);
        assert_eq!(at("02:00"), Opening::Closed, "the end is exclusive");
    }

    #[test]
    fn an_overnight_window_belongs_to_the_day_it_opened_on() {
        // A garage that opens Monday at 22:00 and closes at 02:00 is open in the small hours of
        // *Tuesday*. An `Everyday` window cannot tell the two readings apart, which is why the
        // test above does not catch this and this one does.
        let times = vec![window(DaySelection::Monday, "22:00", "02:00")];
        let at = |day: &str, utc: &str| {
            let instant: DateTime = format!("2026-08-{day}T{utc}:00.000Z").parse().expect("valid");
            is_open_at(false, Some(&times), Some("UTC+00:00"), &instant)
        };

        assert_eq!(at("31", "23:00"), Opening::Open, "Monday evening");
        assert_eq!(at("31", "01:00"), Opening::Closed, "Monday small hours belong to Sunday's window");
        // 2026-09-01 is the Tuesday after.
        let tuesday: DateTime = "2026-09-01T01:00:00.000Z".parse().unwrap();
        assert_eq!(
            is_open_at(false, Some(&times), Some("UTC+00:00"), &tuesday),
            Opening::Open,
            "Tuesday small hours are the tail of Monday's window"
        );
        let tuesday_evening: DateTime = "2026-09-01T23:00:00.000Z".parse().unwrap();
        assert_eq!(
            is_open_at(false, Some(&times), Some("UTC+00:00"), &tuesday_evening),
            Opening::Closed,
            "Tuesday evening is not Monday's window"
        );
    }

    #[test]
    fn a_workday_window_does_not_leak_into_monday_morning_from_sunday() {
        // Workdays = Monday–Friday. A Friday 22:00–02:00 window runs into Saturday; a Monday
        // 01:00 is *not* covered, because Sunday is not a workday.
        let times = vec![window(DaySelection::Workdays, "22:00", "02:00")];
        assert_eq!(is_open_at(false, Some(&times), Some("UTC+00:00"), &monday_at("01:00")), Opening::Closed);
        // Saturday 01:00 is the tail of Friday's window.
        let saturday: DateTime = "2026-09-05T01:00:00.000Z".parse().unwrap();
        assert_eq!(is_open_at(false, Some(&times), Some("UTC+00:00"), &saturday), Opening::Open);
    }

    #[test]
    fn an_unreadable_instant_is_unknown() {
        let times = vec![window(DaySelection::Everyday, "08:00", "18:00")];
        let broken = DateTime::new_unchecked("31.08.2026 12:00");
        assert_eq!(
            is_open_at(false, Some(&times), Some("UTC+00:00"), &broken),
            Opening::Unknown(UnknownReason::MalformedInstant)
        );
    }

    #[test]
    fn day_selections_are_honoured() {
        let workdays = vec![window(DaySelection::Workdays, "08:00", "18:00")];
        let weekend = vec![window(DaySelection::Weekend, "08:00", "18:00")];
        let monday = vec![window(DaySelection::Monday, "08:00", "18:00")];
        let noon = monday_at("12:00"); // a Monday
        let saturday: DateTime = "2026-09-05T12:00:00.000Z".parse().unwrap();

        assert_eq!(is_open_at(false, Some(&workdays), Some("UTC+00:00"), &noon), Opening::Open);
        assert_eq!(is_open_at(false, Some(&workdays), Some("UTC+00:00"), &saturday), Opening::Closed);
        assert_eq!(is_open_at(false, Some(&weekend), Some("UTC+00:00"), &saturday), Opening::Open);
        assert_eq!(is_open_at(false, Some(&weekend), Some("UTC+00:00"), &noon), Opening::Closed);
        assert_eq!(is_open_at(false, Some(&monday), Some("UTC+00:00"), &noon), Opening::Open);
        assert_eq!(is_open_at(false, Some(&monday), Some("UTC+00:00"), &saturday), Opening::Closed);
    }

    #[test]
    fn several_windows_in_a_day_are_all_considered() {
        let times = vec![
            window(DaySelection::Everyday, "08:00", "12:00"),
            window(DaySelection::Everyday, "14:00", "18:00"),
        ];
        let at = |utc: &str| is_open_at(false, Some(&times), Some("UTC+00:00"), &monday_at(utc));
        assert_eq!(at("09:00"), Opening::Open);
        assert_eq!(at("13:00"), Opening::Closed, "the lunch gap");
        assert_eq!(at("15:00"), Opening::Open);
    }

    #[test]
    fn what_cannot_be_answered_is_unknown_rather_than_closed() {
        let times = vec![window(DaySelection::Everyday, "08:00", "18:00")];
        let noon = monday_at("12:00");

        // No time zone: the local time cannot be derived, and guessing UTC would misinform.
        assert_eq!(is_open_at(false, Some(&times), None, &noon), Opening::Unknown(UnknownReason::NoTimeZone));
        assert_eq!(
            is_open_at(false, Some(&times), Some("Europe/Berlin"), &noon),
            Opening::Unknown(UnknownReason::MalformedTimeZone)
        );
        // Not open around the clock, and no times given.
        assert_eq!(
            is_open_at(false, None, Some("UTC+00:00"), &noon),
            Opening::Unknown(UnknownReason::NoOpeningTimes)
        );
        assert_eq!(
            is_open_at(false, Some(&vec![]), Some("UTC+00:00"), &noon),
            Opening::Unknown(UnknownReason::NoOpeningTimes)
        );
    }

    #[test]
    fn a_malformed_period_is_unknown_not_closed() {
        let times = vec![OpeningTimes {
            period: vec![Period {
                begin: HourMinute::new_unchecked("not a time"),
                end: HourMinute::new_unchecked("18:00"),
            }],
            on: DaySelection::Everyday,
            extensions: Extensions::new(),
        }];
        assert_eq!(
            is_open_at(false, Some(&times), Some("UTC+00:00"), &monday_at("12:00")),
            Opening::Unknown(UnknownReason::MalformedPeriod)
        );
    }

    #[test]
    fn offsets_are_parsed_exactly() {
        assert_eq!(parse_offset(Some("UTC+01:00")).unwrap(), UtcOffset::from_hms(1, 0, 0).unwrap());
        assert_eq!(parse_offset(Some("UTC-05:30")).unwrap(), UtcOffset::from_hms(-5, -30, 0).unwrap());
        assert_eq!(parse_offset(Some("UTC+00:00")).unwrap(), UtcOffset::UTC);
        // Both ends of both bounds. +14:00 is Kiribati and a real offset; a `>` written as `>=`
        // would refuse it, and no test that only probes +99:00 can tell.
        assert_eq!(parse_offset(Some("UTC+14:00")).unwrap(), UtcOffset::from_hms(14, 0, 0).unwrap());
        assert_eq!(parse_offset(Some("UTC-14:00")).unwrap(), UtcOffset::from_hms(-14, 0, 0).unwrap());
        assert_eq!(parse_offset(Some("UTC+05:59")).unwrap(), UtcOffset::from_hms(5, 59, 0).unwrap());
        for bad in ["UTC+1:00", "GMT+01:00", "UTC+01-00", "UTC+99:00", "+01:00", "", "UTC+15:00", "UTC+05:60"]
        {
            assert!(parse_offset(Some(bad)).is_err(), "{bad} should not parse");
        }
        assert_eq!(parse_offset(None), Err(UnknownReason::NoTimeZone));
    }

    #[test]
    fn every_named_day_selects_its_own_day() {
        // Seven arms, seven days. Deleting any one of them silently falls through to the group
        // selections, which are all false for a named day — so that charging point is closed all
        // week and nothing says why.
        let days = [
            (DaySelection::Monday, "2026-08-31"),
            (DaySelection::Tuesday, "2026-09-01"),
            (DaySelection::Wednesday, "2026-09-02"),
            (DaySelection::Thursday, "2026-09-03"),
            (DaySelection::Friday, "2026-09-04"),
            (DaySelection::Saturday, "2026-09-05"),
            (DaySelection::Sunday, "2026-09-06"),
        ];
        for (selection, its_day) in &days {
            let times = vec![window(selection.clone(), "08:00", "18:00")];
            for (_, other_day) in &days {
                let at: DateTime = format!("{other_day}T12:00:00.000Z").parse().expect("valid");
                let answer = is_open_at(false, Some(&times), Some("UTC+00:00"), &at);
                let expected = if other_day == its_day { Opening::Open } else { Opening::Closed };
                assert_eq!(answer, expected, "{selection:?} on {other_day}");
            }
        }
    }

    #[test]
    fn the_three_answers_are_distinguishable() {
        let times = vec![window(DaySelection::Everyday, "08:00", "18:00")];
        let open = is_open_at(false, Some(&times), Some("UTC+00:00"), &monday_at("12:00"));
        let closed = is_open_at(false, Some(&times), Some("UTC+00:00"), &monday_at("03:00"));
        let unknown = is_open_at(false, Some(&times), None, &monday_at("12:00"));

        assert!(open.is_open() && !open.is_closed() && !open.is_unknown());
        assert!(closed.is_closed() && !closed.is_open() && !closed.is_unknown());
        assert!(unknown.is_unknown() && !unknown.is_open() && !unknown.is_closed());
    }
}
