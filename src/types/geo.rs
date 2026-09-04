//! `GeoCoordinates` — one position, three notations.

use core::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::number::Number;
use super::validate::{Validate, Validator, ViolationCode};
use crate::oicp_enum;

oicp_enum! {
    /// Which notation `PullEvseData` should answer in.
    ///
    /// A required field on `eRoamingPullEVSEData`: the EMP chooses the notation, and every record
    /// in the response comes back in it.
    pub enum GeoCoordinatesFormat {
        /// `"47.662249 9.360922"` — latitude and longitude in one string, space separated.
        Google = "Google",
        /// Degrees, minutes and seconds: `9°21'39.32''`.
        DegreeMinuteSeconds = "DegreeMinuteSeconds",
        /// Decimal degrees as separate strings: `"9.360922"`.
        DecimalDegree = "DecimalDegree",
    }
}

/// A position on the earth, in whichever of OICP's three notations it arrived in.
///
/// # Why the notation is part of the type
///
/// The spec is explicit that *one of the following three options MUST be provided*, and
/// `PullEvseData` lets the EMP pick which one comes back. All three encode a WGS84 position, but
/// they are not interchangeable on the wire: a record that arrived as `Google` and goes back out
/// as `DecimalDegree` is a changed record, and for a hub that has to forward it, a corrupted one.
///
/// So the variant is preserved, and [`latitude`](Self::latitude) / [`longitude`](Self::longitude)
/// give you the numbers whichever notation they came in.
///
/// ```
/// use oicp_kit::types::GeoCoordinates;
///
/// let json = r#"{"Google":{"Coordinates":"47.662249 9.360922"}}"#;
/// let geo: GeoCoordinates = serde_json::from_str(json)?;
///
/// assert_eq!(geo.latitude().unwrap().to_string(), "47.662249");
/// assert_eq!(geo.longitude().unwrap().to_string(), "9.360922");
/// assert_eq!(serde_json::to_string(&geo)?, json);   // notation preserved
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeoCoordinates {
    /// Latitude and longitude in one string, in that order, separated by a space or comma.
    Google {
        /// The raw coordinate string, e.g. `"47.662249 9.360922"`.
        coordinates: String,
    },
    /// Decimal degrees, as two strings.
    DecimalDegree {
        /// Longitude, e.g. `"9.360922"`.
        longitude: String,
        /// Latitude, e.g. `"47.662249"`.
        latitude: String,
    },
    /// Degrees, minutes and seconds, as two strings.
    DegreeMinuteSeconds {
        /// Longitude, e.g. `"9°21'39.32''"`.
        longitude: String,
        /// Latitude, e.g. `"47°39'44.09''"`.
        latitude: String,
    },
}

impl GeoCoordinates {
    /// Builds a position in decimal-degree notation from exact decimals.
    #[must_use]
    pub fn from_decimal_degrees(latitude: Decimal, longitude: Decimal) -> Self {
        // Written the way the grammar requires — six places at most, and never a bare integer.
        // "Construct strictly" applies to the crate's own constructors first.
        Self::DecimalDegree {
            longitude: conformant(Number::new(longitude)),
            latitude: conformant(Number::new(latitude)),
        }
    }

    /// Which notation this position is written in.
    #[must_use]
    pub const fn format(&self) -> GeoCoordinatesFormat {
        match self {
            Self::Google { .. } => GeoCoordinatesFormat::Google,
            Self::DecimalDegree { .. } => GeoCoordinatesFormat::DecimalDegree,
            Self::DegreeMinuteSeconds { .. } => GeoCoordinatesFormat::DegreeMinuteSeconds,
        }
    }

    /// The latitude, as an exact decimal, whatever notation the value arrived in.
    ///
    /// `None` when the value does not parse — which [`Validate`] also reports.
    #[must_use]
    pub fn latitude(&self) -> Option<Number> {
        match self {
            Self::Google { coordinates } => split_google(coordinates).map(|(lat, _)| lat)?,
            Self::DecimalDegree { latitude, .. } => latitude.parse().ok(),
            Self::DegreeMinuteSeconds { latitude, .. } => parse_dms(latitude),
        }
    }

    /// The longitude, as an exact decimal, whatever notation the value arrived in.
    #[must_use]
    pub fn longitude(&self) -> Option<Number> {
        match self {
            Self::Google { coordinates } => split_google(coordinates).map(|(_, lon)| lon)?,
            Self::DecimalDegree { longitude, .. } => longitude.parse().ok(),
            Self::DegreeMinuteSeconds { longitude, .. } => parse_dms(longitude),
        }
    }

    /// This position rewritten in `format`.
    ///
    /// Explicit, because rewriting a peer's record silently is exactly what this type exists to
    /// prevent. Returns `None` when the coordinates do not parse.
    ///
    /// # Six decimal places, not twenty-eight
    ///
    /// The specification's decimal-degree grammar is `^-?1?\d{1,2}\.\d{1,6}$` — **at most six**
    /// fractional digits, and a decimal point that is not optional. Converting a
    /// degrees-minutes-seconds value divides by 3600 and produces twenty-eight, so the honest
    /// conversion rounds. Six decimal places is about 11 cm at the equator, which is finer than any
    /// charging point is surveyed; the digits beyond it were arithmetic, not measurement.
    #[must_use]
    pub fn to_format(&self, format: GeoCoordinatesFormat) -> Option<Self> {
        let (lat, lon) = (self.latitude()?, self.longitude()?);
        Some(match format {
            GeoCoordinatesFormat::Google => {
                Self::Google { coordinates: format!("{} {}", conformant(lat), conformant(lon)) }
            }
            GeoCoordinatesFormat::DecimalDegree => {
                Self::DecimalDegree { longitude: conformant(lon), latitude: conformant(lat) }
            }
            GeoCoordinatesFormat::DegreeMinuteSeconds => {
                Self::DegreeMinuteSeconds { longitude: to_dms(lon), latitude: to_dms(lat) }
            }
        })
    }

    /// Whether both coordinates parse, lie in range, **and** are written the way the
    /// specification's grammar for this notation requires.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        let (Some(lat), Some(lon)) = (self.latitude(), self.longitude()) else { return false };
        in_range(lat, 90) && in_range(lon, 180) && self.matches_its_grammar().is_empty()
    }

    /// The members whose text does not match the grammar the specification gives for this notation.
    fn matches_its_grammar(&self) -> Vec<&'static str> {
        let mut wrong = vec![];
        match self {
            Self::Google { coordinates } => {
                if !is_google(coordinates) {
                    wrong.push("Coordinates");
                }
            }
            Self::DecimalDegree { longitude, latitude } => {
                if !is_decimal_degree(latitude) {
                    wrong.push("Latitude");
                }
                if !is_decimal_degree(longitude) {
                    wrong.push("Longitude");
                }
            }
            Self::DegreeMinuteSeconds { longitude, latitude } => {
                if !is_dms(latitude) {
                    wrong.push("Latitude");
                }
                if !is_dms(longitude) {
                    wrong.push("Longitude");
                }
            }
        }
        wrong
    }
}

/// A decimal degree written the way `^-?1?\d{1,2}\.\d{1,6}$` requires: rounded to six places,
/// and never bare — `52` is not a decimal degree, `52.0` is.
fn conformant(value: Number) -> String {
    let rounded = value.round_dp(6).get().normalize();
    let text = rounded.to_string();
    if text.contains('.') { text } else { format!("{text}.0") }
}

fn in_range(value: Number, limit: i64) -> bool {
    let d = value.get();
    d >= Decimal::from(-limit) && d <= Decimal::from(limit)
}

/// `^-?1?\d{1,2}\.\d{1,6}$`
///
/// The mandatory decimal point is the part that surprises: OICP has no notation for a whole
/// number of degrees, so `"52"` is not a coordinate and `"52.0"` is.
fn is_decimal_degree(s: &str) -> bool {
    let body = s.strip_prefix('-').unwrap_or(s);
    // `-?1?\d{1,2}` — an optional leading 1, then one or two digits.
    let body = body.strip_prefix('1').unwrap_or(body);
    let Some((whole, fraction)) = body.split_once('.') else { return false };
    (1..=2).contains(&whole.len())
        && whole.bytes().all(|c| c.is_ascii_digit())
        && (1..=6).contains(&fraction.len())
        && fraction.bytes().all(|c| c.is_ascii_digit())
}

/// `^-?1?\d{1,2}\.\d{1,6}\s*\,?\s*-?1?\d{1,2}\.\d{1,6}$` — latitude, then longitude.
fn is_google(s: &str) -> bool {
    let cleaned = s.replace(',', " ");
    let mut parts = cleaned.split_whitespace();
    let (Some(lat), Some(lon)) = (parts.next(), parts.next()) else { return false };
    parts.next().is_none() && is_decimal_degree(lat) && is_decimal_degree(lon)
}

/// `^-?1?\d{1,2}°[ ]?\d{1,2}'[ ]?\d{1,2}\.\d+''$`
///
/// The seconds carry a mandatory fractional part, so `9°21'39''` is not a coordinate even though
/// it reads like one.
fn is_dms(s: &str) -> bool {
    let body = s.strip_prefix('-').unwrap_or(s);
    let Some((degrees, rest)) = body.split_once('°') else { return false };
    let degrees = degrees.strip_prefix('1').unwrap_or(degrees);
    if !(1..=2).contains(&degrees.len()) || !degrees.bytes().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    let Some((minutes, rest)) = rest.split_once('\'') else { return false };
    if !(1..=2).contains(&minutes.len()) || !minutes.bytes().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    let Some(seconds) = rest.strip_suffix("''") else { return false };
    let Some((whole, fraction)) = seconds.split_once('.') else { return false };
    (1..=2).contains(&whole.len())
        && whole.bytes().all(|c| c.is_ascii_digit())
        && !fraction.is_empty()
        && fraction.bytes().all(|c| c.is_ascii_digit())
}

/// `"47.662249 9.360922"` or `"47.662249,9.360922"` → (latitude, longitude).
fn split_google(s: &str) -> Option<(Option<Number>, Option<Number>)> {
    let cleaned = s.replace(',', " ");
    let mut parts = cleaned.split_whitespace();
    let lat = parts.next()?;
    let lon = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some((lat.parse().ok(), lon.parse().ok()))
}

/// `9°21'39.32''` → `9.360922…`, exactly, without ever touching an `f64`.
fn parse_dms(s: &str) -> Option<Number> {
    let negative = s.starts_with('-');
    let body = s.strip_prefix('-').unwrap_or(s).trim();
    let (deg, rest) = body.split_once('°')?;
    let (min, rest) = rest.trim_start().split_once('\'')?;
    let sec = rest.trim_start().trim_end_matches('\'');
    let deg: Decimal = deg.trim().parse().ok()?;
    let min: Decimal = min.trim().parse().ok()?;
    let sec: Decimal = sec.trim().parse().ok()?;
    let sixty = Decimal::from(60);
    let value = deg + min / sixty + sec / (sixty * sixty);
    Some(Number::new(if negative { -value } else { value }))
}

/// The inverse of [`parse_dms`], written the way `^-?1?\d{1,2}°[ ]?\d{1,2}'[ ]?\d{1,2}\.\d+''$`
/// requires — including the fractional seconds, which are not optional there.
fn to_dms(value: Number) -> String {
    let d = value.get();
    let sign = if d.is_sign_negative() { "-" } else { "" };
    let abs = d.abs();
    let degrees = abs.trunc();
    let minutes_full = (abs - degrees) * Decimal::from(60);
    let minutes = minutes_full.trunc();
    let seconds = ((minutes_full - minutes) * Decimal::from(60)).round_dp(2);
    // `\d{1,2}\.\d+`: a whole number of seconds is not a conformant value, so 39 is written 39.0.
    let seconds = seconds.normalize().to_string();
    let seconds = if seconds.contains('.') { seconds } else { format!("{seconds}.0") };
    format!("{sign}{degrees}°{minutes}'{seconds}''")
}

impl fmt::Display for GeoCoordinates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.latitude(), self.longitude()) {
            (Some(lat), Some(lon)) => write!(f, "{lat}, {lon}"),
            _ => f.write_str("<unparseable coordinates>"),
        }
    }
}

impl Validate for GeoCoordinates {
    fn validate_in(&self, v: &mut Validator) {
        let member = match self {
            Self::Google { .. } => "Google",
            Self::DecimalDegree { .. } => "DecimalDegree",
            Self::DegreeMinuteSeconds { .. } => "DegreeMinuteSeconds",
        };
        v.enter(member);
        for field in self.matches_its_grammar() {
            v.report_at(
                field,
                ViolationCode::PatternMismatch,
                match self {
                    Self::Google { .. } => "not two decimal degrees of at most six places, latitude first",
                    Self::DecimalDegree { .. } => {
                        "not -?1?\\d{1,2}.\\d{1,6}: a decimal degree needs a decimal point and at most six places"
                    }
                    Self::DegreeMinuteSeconds { .. } => {
                        "not -?1?\\d{1,2}°\\d{1,2}'\\d{1,2}.\\d+'': the seconds need a fractional part"
                    }
                },
            );
        }
        match self.latitude() {
            None => v.report(ViolationCode::PatternMismatch, "the latitude does not parse"),
            Some(lat) if !in_range(lat, 90) => {
                v.report(ViolationCode::OutOfRange, format!("latitude {lat} is outside -90..=90"));
            }
            Some(_) => {}
        }
        match self.longitude() {
            None => v.report(ViolationCode::PatternMismatch, "the longitude does not parse"),
            Some(lon) if !in_range(lon, 180) => {
                v.report(ViolationCode::OutOfRange, format!("longitude {lon} is outside -180..=180"));
            }
            Some(_) => {}
        }
        v.leave();
    }
}

/// The wire shape: an object with three optional members.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct GeoWire {
    #[serde(rename = "Google", default, skip_serializing_if = "Option::is_none")]
    google: Option<GoogleWire>,
    #[serde(rename = "DecimalDegree", default, skip_serializing_if = "Option::is_none")]
    decimal_degree: Option<PairWire>,
    #[serde(rename = "DegreeMinuteSeconds", default, skip_serializing_if = "Option::is_none")]
    degree_minute_seconds: Option<PairWire>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct GoogleWire {
    #[serde(rename = "Coordinates")]
    coordinates: String,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct PairWire {
    #[serde(rename = "Longitude")]
    longitude: String,
    #[serde(rename = "Latitude")]
    latitude: String,
}

impl Serialize for GeoCoordinates {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let wire = match self.clone() {
            Self::Google { coordinates } => GeoWire {
                google: Some(GoogleWire { coordinates }),
                decimal_degree: None,
                degree_minute_seconds: None,
            },
            Self::DecimalDegree { longitude, latitude } => GeoWire {
                google: None,
                decimal_degree: Some(PairWire { longitude, latitude }),
                degree_minute_seconds: None,
            },
            Self::DegreeMinuteSeconds { longitude, latitude } => GeoWire {
                google: None,
                decimal_degree: None,
                degree_minute_seconds: Some(PairWire { longitude, latitude }),
            },
        };
        wire.serialize(s)
    }
}

impl<'de> Deserialize<'de> for GeoCoordinates {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = GeoWire::deserialize(d)?;
        // Spec order. Hubject's own examples fill in all three members at once, which no real
        // payload does; taking the first present member is what its parser does too.
        if let Some(g) = wire.google {
            return Ok(Self::Google { coordinates: g.coordinates });
        }
        if let Some(p) = wire.decimal_degree {
            return Ok(Self::DecimalDegree { longitude: p.longitude, latitude: p.latitude });
        }
        if let Some(p) = wire.degree_minute_seconds {
            return Ok(Self::DegreeMinuteSeconds { longitude: p.longitude, latitude: p.latitude });
        }
        Err(serde::de::Error::custom(
            "GeoCoordinates must carry one of Google, DecimalDegree or DegreeMinuteSeconds",
        ))
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for GeoCoordinates {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "GeoCoordinates".into()
    }
    fn json_schema(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        GeoWire::json_schema(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_notation_round_trips_unchanged() {
        for json in [
            r#"{"Google":{"Coordinates":"47.662249 9.360922"}}"#,
            r#"{"DecimalDegree":{"Longitude":"9.360922","Latitude":"47.662249"}}"#,
            r#"{"DegreeMinuteSeconds":{"Longitude":"9°21'39.32''","Latitude":"47°39'44.09''"}}"#,
        ] {
            let geo: GeoCoordinates = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&geo).unwrap(), json, "{json} was rewritten");
        }
    }

    #[test]
    fn coordinates_read_the_same_whichever_notation_they_arrived_in() {
        let google: GeoCoordinates =
            serde_json::from_str(r#"{"Google":{"Coordinates":"52.480495 13.356465"}}"#).unwrap();
        let decimal: GeoCoordinates =
            serde_json::from_str(r#"{"DecimalDegree":{"Longitude":"13.356465","Latitude":"52.480495"}}"#)
                .unwrap();
        assert_eq!(google.latitude(), decimal.latitude());
        assert_eq!(google.longitude(), decimal.longitude());
        assert_eq!(google.latitude().unwrap().to_string(), "52.480495");
    }

    #[test]
    fn a_comma_separated_google_pair_is_understood() {
        let geo: GeoCoordinates =
            serde_json::from_str(r#"{"Google":{"Coordinates":"52.480495,13.356465"}}"#).unwrap();
        assert_eq!(geo.latitude().unwrap().to_string(), "52.480495");
        assert!(geo.is_well_formed());
    }

    #[test]
    fn degrees_minutes_seconds_convert_exactly() {
        let dms: GeoCoordinates = serde_json::from_str(
            r#"{"DegreeMinuteSeconds":{"Longitude":"9°21'39.32''","Latitude":"-21°34'23.16''"}}"#,
        )
        .unwrap();
        // 9 + 21/60 + 39.32/3600, computed in decimal, not binary.
        assert_eq!(dms.longitude().unwrap().round_dp(6).to_string(), "9.360922");
        assert!(dms.latitude().unwrap().is_negative());
        assert!(dms.is_well_formed());
    }

    #[test]
    fn conversion_between_notations_is_explicit() {
        let google: GeoCoordinates =
            serde_json::from_str(r#"{"Google":{"Coordinates":"52.480495 13.356465"}}"#).unwrap();
        let decimal = google.to_format(GeoCoordinatesFormat::DecimalDegree).unwrap();
        assert_eq!(decimal.format(), GeoCoordinatesFormat::DecimalDegree);
        assert_eq!(decimal.latitude(), google.latitude());
    }

    #[test]
    fn out_of_range_and_unparseable_coordinates_are_reported() {
        // Two problems each, and both are worth saying: `999` is out of range *and* is not written
        // the way a decimal degree has to be — there is no notation in OICP for a whole number of
        // degrees.
        let bad: GeoCoordinates =
            serde_json::from_str(r#"{"DecimalDegree":{"Longitude":"999","Latitude":"91"}}"#).unwrap();
        let err = bad.validate().unwrap_err();
        assert_eq!(err.len(), 4, "{err}");
        assert!(err.iter().all(|x| x.pointer.starts_with("/DecimalDegree")));
        assert_eq!(err.iter().filter(|x| x.code == ViolationCode::OutOfRange).count(), 2);
        assert_eq!(err.iter().filter(|x| x.code == ViolationCode::PatternMismatch).count(), 2);

        let junk: GeoCoordinates = serde_json::from_str(r#"{"Google":{"Coordinates":"north-ish"}}"#).unwrap();
        assert!(!junk.is_well_formed());
        assert!(junk.validate().is_err());
    }

    #[test]
    fn each_notation_is_checked_against_its_own_grammar() {
        // The specification gives a regular expression per notation, and they are stricter than
        // "it parses": a decimal point is mandatory, six fractional places is the maximum, and the
        // seconds of a DMS value carry a fractional part.
        let decimal = |lat: &str| GeoCoordinates::DecimalDegree {
            latitude: lat.to_owned(),
            longitude: "9.360922".to_owned(),
        };
        for good in ["9.360922", "52.480495", "-21.568201", "13.5", "0.0"] {
            assert!(decimal(good).validate().is_ok(), "{good} is conformant");
        }
        for bad in ["52", "52.", "9.3609222", "", "9,36"] {
            let value = decimal(bad);
            assert!(value.validate().is_err(), "{bad:?} should be reported");
            assert!(!value.is_well_formed(), "{bad:?}");
        }

        // The contract every type in this crate keeps: the cheap question and the detailed one
        // agree. A `is_well_formed` that says yes where `validate` says no is the worse of the two
        // to trust, and callers reach for it because it is cheaper.
        for text in ["9.360922", "52", "9.3609222", "", "-21.568201", "north-ish"] {
            let value = decimal(text);
            assert_eq!(value.is_well_formed(), value.validate().is_ok(), "disagreed on {text:?}");
        }

        let dms = |lat: &str| GeoCoordinates::DegreeMinuteSeconds {
            latitude: lat.to_owned(),
            longitude: "9°21'39.32''".to_owned(),
        };
        assert!(dms("47°39'44.09''").validate().is_ok());
        assert!(dms("47° 39' 44.09''").validate().is_ok(), "the optional spaces are allowed");
        assert!(dms("47°39'44''").validate().is_err(), "the seconds need a fractional part");

        let google = |c: &str| GeoCoordinates::Google { coordinates: c.to_owned() };
        assert!(google("47.662249 9.360922").validate().is_ok());
        assert!(google("47.662249,9.360922").validate().is_ok());
        assert!(google("47 9").validate().is_err());
    }

    #[test]
    fn a_coordinate_renders_readably_and_says_when_it_cannot() {
        let geo: GeoCoordinates =
            serde_json::from_str(r#"{"Google":{"Coordinates":"52.480495 13.356465"}}"#).unwrap();
        assert_eq!(geo.to_string(), "52.480495, 13.356465");

        let junk = GeoCoordinates::Google { coordinates: "north-ish".to_owned() };
        assert_eq!(junk.to_string(), "<unparseable coordinates>");
    }

    #[cfg(feature = "schema")]
    #[test]
    fn the_published_schema_describes_the_three_notations() {
        // `oicp schema` is generated from this. An empty schema accepts anything, including the
        // two-member object the type exists to make impossible.
        let mut generator = schemars::SchemaGenerator::default();
        let schema = <GeoCoordinates as schemars::JsonSchema>::json_schema(&mut generator);
        let json = serde_json::to_value(&schema).unwrap();
        let properties = json["properties"].as_object().expect("an object with members: {json}");
        for member in ["Google", "DecimalDegree", "DegreeMinuteSeconds"] {
            assert!(properties.contains_key(member), "{member} is missing from {json}");
        }
        assert_eq!(<GeoCoordinates as schemars::JsonSchema>::schema_name(), "GeoCoordinates");
    }

    #[test]
    fn the_dms_conversion_produces_the_value_it_should() {
        // Degrees, minutes and seconds each come out of a different step of the same arithmetic:
        // truncate, times sixty, truncate, times sixty. A `*` written as a `+` still produces a
        // string shaped like a coordinate, so only the exact text catches it.
        let dms = |d: &str| to_dms(d.parse::<Number>().unwrap());

        assert_eq!(dms("9.360922"), "9°21'39.32''");
        assert_eq!(dms("47.662249"), "47°39'44.1''");
        assert_eq!(dms("-21.573100"), "-21°34'23.16''");
        assert_eq!(dms("0.5"), "0°30'0.0''", "a whole number of seconds still needs its point");
        assert_eq!(dms("52.0"), "52°0'0.0''");

        // …and every one of them is a value the grammar accepts.
        for text in ["9.360922", "47.662249", "-21.573100", "0.5", "52.0", "13.999999"] {
            let rendered = dms(text);
            assert!(is_dms(&rendered), "{text} rendered as {rendered}, which is not conformant");
        }

        // Round-tripping through the notation lands within the rounding it documents.
        let back = parse_dms(&dms("9.360922")).unwrap();
        assert_eq!(back.round_dp(5), "9.36092".parse::<Number>().unwrap());
    }

    #[test]
    fn each_condition_of_the_dms_grammar_stands_on_its_own() {
        // Each clause of the check rejects on its own, so a case per clause — otherwise an `||`
        // written as an `&&` is caught by nothing.
        assert!(is_dms("9°21'39.32''"));
        assert!(is_dms("-9°21'39.32''"), "a negative latitude");
        assert!(is_dms("113°21'39.32''"), "the leading 1 of a three-digit longitude");
        assert!(is_dms("9° 21' 39.32''"), "the optional spaces");

        assert!(!is_dms("9°21'39.32'"), "one closing quote is not two");
        assert!(!is_dms("921'39.32''"), "no degree sign");
        assert!(!is_dms("9°2139.32''"), "no minute mark");
        assert!(!is_dms("999°21'39.32''"), "three digits without the leading 1");
        assert!(!is_dms("°21'39.32''"), "no degrees at all");
        assert!(!is_dms("9°211'39.32''"), "three minute digits");
        assert!(!is_dms("9°'39.32''"), "no minutes at all");
        assert!(!is_dms("9°2a'39.32''"), "minutes that are not digits");
        assert!(!is_dms("9°21'399.32''"), "three whole seconds digits");
        assert!(!is_dms("9°21'.32''"), "no whole seconds");
        assert!(!is_dms("9°21'3a.32''"), "whole seconds that are not digits");
        assert!(!is_dms("9°21'39.''"), "an empty fractional part");
        assert!(!is_dms("9°21'39.3a''"), "a fractional part that is not digits");
        assert!(!is_dms("9°21'39''"), "and no fractional part at all");
    }

    #[test]
    fn each_condition_of_the_google_grammar_stands_on_its_own() {
        assert!(is_google("47.662249 9.360922"));
        assert!(is_google("47.662249,9.360922"));
        assert!(is_google("47.662249 , 9.360922"));

        assert!(!is_google("47.662249"), "one coordinate is not a pair");
        assert!(!is_google("47.662249 9.360922 1.0"), "and three is not a pair either");
        assert!(!is_google("47 9.360922"), "the latitude needs its decimal point");
        assert!(!is_google("47.662249 9"), "so does the longitude");
        assert!(!is_google(""), "and an empty string is not a position");
    }

    #[test]
    fn a_converted_coordinate_is_conformant_by_construction() {
        // Degrees-minutes-seconds divided by 3600 gives twenty-eight decimal places, and the
        // grammar allows six. A conversion that emits what the arithmetic produced writes a value
        // the specification does not accept — from a value that was perfectly fine.
        let dms: GeoCoordinates = serde_json::from_str(
            r#"{"DegreeMinuteSeconds":{"Longitude":"9°21'39.32''","Latitude":"47°39'44.09''"}}"#,
        )
        .unwrap();

        for format in [
            GeoCoordinatesFormat::DecimalDegree,
            GeoCoordinatesFormat::Google,
            GeoCoordinatesFormat::DegreeMinuteSeconds,
        ] {
            let converted = dms.to_format(format).expect("the source parses");
            assert_eq!(converted.format(), format);
            assert!(converted.validate().is_ok(), "{format:?} produced {converted:?}");
            assert!(converted.is_well_formed());
        }

        let decimal = dms.to_format(GeoCoordinatesFormat::DecimalDegree).unwrap();
        let GeoCoordinates::DecimalDegree { latitude, .. } = &decimal else { panic!("decimal") };
        assert_eq!(latitude, "47.662247", "rounded to the six places the grammar allows");

        // A whole number of degrees still needs its decimal point — including straight out of the
        // constructor, which is the first place "construct strictly" applies.
        let whole = GeoCoordinates::from_decimal_degrees(52.into(), 13.into());
        assert!(whole.validate().is_ok(), "{whole:?}");
        assert_eq!(
            whole,
            GeoCoordinates::DecimalDegree { latitude: "52.0".to_owned(), longitude: "13.0".to_owned() }
        );
        assert!(whole.to_format(GeoCoordinatesFormat::DecimalDegree).unwrap().validate().is_ok());
    }

    #[test]
    fn an_empty_geo_object_is_a_decode_error() {
        assert!(serde_json::from_str::<GeoCoordinates>("{}").is_err());
    }
}
