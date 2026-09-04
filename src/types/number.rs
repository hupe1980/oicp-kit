//! `Number` — the OICP decimal number type. Never `f64` in a field, never `f64` in arithmetic.

use core::fmt;
use core::str::FromStr;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::validate::{Validate, Validator, ViolationCode};

/// An OICP JSON `number`: an exact decimal.
///
/// # Why not `f64`
///
/// Every `number` in OICP is either energy or money: `ConsumedEnergy`, `MeterValueStart`,
/// `MeterValueEnd`, `PricePerReferenceUnit`, `PricingDefaultPrice`. All of them end up on an
/// invoice between two companies, and OICP's own CDR rule — `ConsumedEnergy` is *the difference
/// between* `MeterValueEnd` *and* `MeterValueStart` — is an exact decimal identity that binary
/// floating point cannot honour: `10.1 - 0.1 != 10.0` in `f64`.
///
/// So every OICP number in this crate is a [`rust_decimal::Decimal`]. No public field of any wire
/// type is an `f32` or `f64`, the [`sync`](crate::sync) engine has none, and
/// `cargo run -p xtask -- no-floats` enforces that in CI.
///
/// # The JSON boundary
///
/// OICP sends these as JSON *numbers*, not strings. `serde_json` represents a fractional JSON
/// number as an `f64` unless its `arbitrary_precision` feature is enabled — a feature that changes
/// `serde_json::Value` globally for every crate in the build, so `oicp-kit` does not impose it.
/// The boundary therefore behaves as follows:
///
/// * Integral values pass through exactly, as JSON integers.
/// * Fractional values with at most 15 significant decimal digits — which covers every price and
///   energy OICP carries, with room to spare — pass through exactly, because the shortest decimal
///   that round-trips an `f64` *is* the original decimal.
/// * Beyond that, a round-trip rounds to the nearest `f64`. [`Number::json_round_trips`] says
///   whether a given value is affected and [`Validate::validate`] reports it as
///   [`ViolationCode::Imprecise`], so this can never happen silently.
///
/// A peer that sends a number as a JSON *string* (`"0.25"`) is tolerated on input and parsed
/// exactly; output is always a JSON number.
///
/// ```
/// use oicp_kit::types::Number;
///
/// let energy: Number = "10.1".parse().unwrap();
/// let start: Number = "0.1".parse().unwrap();
/// // The identity OICP requires of every CDR, exactly.
/// assert_eq!((energy - start).to_string(), "10.0");
/// assert_eq!(serde_json::to_string(&energy).unwrap(), "10.1");
/// ```
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Number(Decimal);

impl Number {
    /// Zero.
    pub const ZERO: Self = Self(Decimal::ZERO);
    /// One.
    pub const ONE: Self = Self(Decimal::ONE);

    /// Wraps a [`Decimal`].
    #[must_use]
    pub const fn new(value: Decimal) -> Self {
        Self(value)
    }

    /// The underlying [`Decimal`].
    #[must_use]
    pub const fn get(self) -> Decimal {
        self.0
    }

    /// The number of digits after the decimal point.
    #[must_use]
    pub fn scale(self) -> u32 {
        self.0.scale()
    }

    /// Rounds to `dp` decimal places, half away from zero.
    #[must_use]
    pub fn round_dp(self, dp: u32) -> Self {
        Self(self.0.round_dp_with_strategy(dp, rust_decimal::RoundingStrategy::MidpointAwayFromZero))
    }

    /// Whether this value survives a JSON round-trip unchanged.
    ///
    /// See [the type documentation](Self#the-json-boundary). `false` only for values with more
    /// significant digits than an `f64` can carry.
    #[must_use]
    pub fn json_round_trips(self) -> bool {
        if self.0.is_integer() && self.0.to_i64().is_some() {
            return true;
        }
        self.0.to_f64().and_then(decimal_from_f64).is_some_and(|d| d == self.0.normalize())
    }

    /// Whether the value is zero.
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    /// Whether the value is strictly negative.
    #[must_use]
    pub fn is_negative(self) -> bool {
        self.0.is_sign_negative() && !self.0.is_zero()
    }

    /// Whether the value is a whole number.
    ///
    /// OICP's property tables type several fields as `Integer` that the OpenAPI sources widened
    /// to `number`; this is how the wire types check them without refusing the value.
    #[must_use]
    pub fn is_integer(self) -> bool {
        self.0.is_integer()
    }

    /// Reports a violation unless the value lies within `min..=max` inclusive.
    pub(crate) fn validate_range(self, v: &mut Validator, min: i64, max: i64) {
        let (lo, hi) = (Decimal::from(min), Decimal::from(max));
        if self.0 < lo || self.0 > hi {
            v.report(
                ViolationCode::OutOfRange,
                format!("{} is outside the allowed range {min}..={max}", self.0),
            );
        }
    }

    /// As [`validate_range`](Self::validate_range), but for a bound the specification states and
    /// real equipment exceeds.
    ///
    /// The message names the [`SpecDefect`](super::SpecDefect), so a partner with a perfectly good
    /// 500 A charger reads "OICP 2.3 caps this at 99 A, here is the issue" rather than concluding
    /// that this crate cannot count.
    pub(crate) fn validate_range_with_defect(self, v: &mut Validator, min: i64, max: i64, defect: &str) {
        let (lo, hi) = (Decimal::from(min), Decimal::from(max));
        if self.0 < lo || self.0 > hi {
            let note = super::SpecDefect::get(defect).map_or_else(String::new, |d| format!("; {}", d.note()));
            v.report(
                ViolationCode::OutOfRange,
                format!("{} is outside the allowed range {min}..={max}{note}", self.0),
            );
        }
    }
}

/// Reconstructs a decimal from an `f64` the way `serde_json` would print it — via the shortest
/// representation that round-trips — so `json_round_trips` compares like with like.
fn decimal_from_f64(f: f64) -> Option<Decimal> {
    Decimal::from_str(&format!("{f}")).ok().map(|d| d.normalize())
}

impl Validate for Number {
    fn validate_in(&self, v: &mut Validator) {
        if !self.json_round_trips() {
            v.report(
                ViolationCode::Imprecise,
                format!("{} carries more significant digits than a JSON number round-trip preserves", self.0),
            );
        }
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Number({})", self.0)
    }
}

impl From<Decimal> for Number {
    fn from(value: Decimal) -> Self {
        Self(value)
    }
}

impl From<Number> for Decimal {
    fn from(value: Number) -> Self {
        value.0
    }
}

macro_rules! from_integer {
    ($($t:ty),*) => {
        $(impl From<$t> for Number {
            fn from(value: $t) -> Self { Self(Decimal::from(value)) }
        })*
    };
}
from_integer!(i8, i16, i32, i64, u8, u16, u32, u64);

impl FromStr for Number {
    type Err = rust_decimal::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Decimal::from_str(s).or_else(|_| Decimal::from_scientific(s)).map(Self)
    }
}

impl core::ops::Add for Number {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl core::ops::Sub for Number {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl core::ops::Mul for Number {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self(self.0 * rhs.0)
    }
}

impl core::iter::Sum for Number {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Self(iter.map(|n| n.0).sum())
    }
}

impl Serialize for Number {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // An integral value is written as a JSON integer, which is exact for anything an i64
        // holds. A fractional value goes out through f64, which `serde_json` prints with the
        // shortest round-tripping representation — the original decimal, for every value in
        // OICP's domain. `json_round_trips` reports the values where that is not true.
        if self.0.is_integer() {
            if let Some(i) = self.0.to_i64() {
                return s.serialize_i64(i);
            }
        }
        match self.0.to_f64() {
            Some(f) => s.serialize_f64(f),
            None => {
                Err(serde::ser::Error::custom(format!("{} cannot be represented as a JSON number", self.0)))
            }
        }
    }
}

impl<'de> Deserialize<'de> for Number {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = Number;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON number, or a string holding one")
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Number, E> {
                Ok(Number(Decimal::from(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Number, E> {
                Ok(Number(Decimal::from(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Number, E> {
                // Via the shortest round-tripping text rather than `Decimal::from_f64_retain`,
                // so `0.1` arrives as the decimal `0.1` and not as the binary approximation.
                decimal_from_f64(v)
                    .map(Number)
                    .ok_or_else(|| E::custom(format!("{v} is not a finite JSON number")))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Number, E> {
                v.parse().map_err(|_| E::custom(format!("{v:?} is not a number")))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Number {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Number".into()
    }
    fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "number" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_survive_the_json_boundary() {
        for text in ["0.1", "0.25", "10.1", "22", "0", "-3.5", "1234.5678", "99999.99"] {
            let n: Number = text.parse().unwrap();
            let json = serde_json::to_string(&n).unwrap();
            let back: Number = serde_json::from_str(&json).unwrap();
            assert_eq!(n.get().normalize(), back.get().normalize(), "{text} did not round-trip ({json})");
        }
    }

    #[test]
    fn the_cdr_energy_identity_is_exact() {
        // The float trap this type exists to avoid: 10.1_f64 - 0.1_f64 == 10.000000000000002.
        let end: Number = "10.1".parse().unwrap();
        let start: Number = "0.1".parse().unwrap();
        let consumed: Number = "10.0".parse().unwrap();
        assert_eq!(end - start, consumed);
    }

    #[test]
    fn integral_values_are_written_as_json_integers() {
        let n: Number = "22".parse().unwrap();
        assert_eq!(serde_json::to_string(&n).unwrap(), "22");
        let n: Number = "22.0".parse().unwrap();
        assert_eq!(serde_json::to_string(&n).unwrap(), "22");
    }

    #[test]
    fn a_number_sent_as_a_string_is_accepted() {
        let n: Number = serde_json::from_str(r#""0.25""#).unwrap();
        assert_eq!(n.to_string(), "0.25");
        assert_eq!(serde_json::to_string(&n).unwrap(), "0.25");
    }

    #[test]
    fn imprecise_values_are_reported_not_silently_rounded() {
        let n: Number = "1.2345678901234567890123".parse().unwrap();
        assert!(!n.json_round_trips());
        assert_eq!(n.validate().unwrap_err().as_slice()[0].code, ViolationCode::Imprecise);
    }
}
