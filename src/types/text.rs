//! `Text` — a length-limited OICP string, and the shapes built on it.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::builder::strict_builder;
use super::validate::{Validate, Validator, ViolationCode, validate_fields};

/// A string with the maximum length OICP's property table gives for its field.
///
/// `N` is that maximum. Over-long values **arrive** — a peer that sends a 60-character city name
/// must not make the surrounding page undecodable — and [`Validate::validate`] reports them;
/// [`Text::new`] refuses to build one.
///
/// ```
/// use oicp_kit::types::{Text, Validate};
///
/// let city = Text::<50>::new("Berlin")?;
/// assert_eq!(city.as_str(), "Berlin");
///
/// // Too long to construct…
/// assert!(Text::<3>::new("Berlin").is_err());
/// // …but it still decodes, and is reported.
/// let arrived: Text<3> = serde_json::from_str(r#""Berlin""#)?;
/// assert!(arrived.validate().is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Text<const N: usize>(String);

/// Why a string could not be built.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    /// The value was longer than the field's maximum.
    #[error("the value is {actual} characters, but the maximum is {max}")]
    TooLong {
        /// The maximum the field allows.
        max: usize,
        /// The length of the offered value.
        actual: usize,
    },
}

impl<const N: usize> Text<N> {
    /// The maximum length of this field, per the specification's property table.
    pub const MAX: usize = N;

    /// Builds a value, refusing one that is already out of spec.
    ///
    /// # Errors
    ///
    /// Returns [`TextError::TooLong`] when the value exceeds `N` characters.
    pub fn new(value: impl Into<String>) -> Result<Self, TextError> {
        let s = value.into();
        let actual = s.chars().count();
        if actual > N {
            return Err(TextError::TooLong { max: N, actual });
        }
        Ok(Self(s))
    }

    /// Accepts `value` without checking its length; [`Validate`] reports it.
    #[must_use]
    pub fn new_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The text, consuming the wrapper.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }

    /// The length in characters.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.chars().count()
    }

    /// Whether the text is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<const N: usize> fmt::Display for Text<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<const N: usize> FromStr for Text<N> {
    type Err = TextError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl<const N: usize> Validate for Text<N> {
    fn validate_in(&self, v: &mut Validator) {
        let actual = self.len();
        if actual > N {
            v.report(
                ViolationCode::TooLong,
                format!("the value is {actual} characters, but the maximum is {N}"),
            );
        }
    }
}

impl<const N: usize> Serialize for Text<N> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de, const N: usize> Deserialize<'de> for Text<N> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self(String::deserialize(d)?))
    }
}

impl<const N: usize> AsRef<str> for Text<N> {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// These conversions are deliberately **permissive**, so a builder field can take a `&str`
// directly. The strictness promised by "construct strictly" lives one level up: every wire type's
// `build()` validates the finished object and returns `Err(Violations)`, so an over-long value
// entered here is caught before the object exists. `build_unchecked()` is the escape hatch, and
// says so in its name.
impl<const N: usize> From<&str> for Text<N> {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl<const N: usize> From<String> for Text<N> {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[cfg(feature = "schema")]
impl<const N: usize> schemars::JsonSchema for Text<N> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("Text{N}").into()
    }
    fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "maxLength": N })
    }
}

/// A partner-assigned session identifier, opaque to Hubject.
///
/// Both `CPOPartnerSessionID` and `EMPPartnerSessionID` are free-text, max 250 characters:
/// *"Partner systems can use this field to link their own session handling to HBS processes."*
pub type PartnerSessionId = Text<250>;

/// Free text in a stated language, used for station names and additional information.
///
/// Spec: `InfoTextType`. `lang` is an ISO-639-1 or ISO-639-2/T code with optional region and
/// script subtags; `value` is at most 150 characters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
#[builder(finish_fn = build_unchecked)]
pub struct InfoText {
    /// The language the text is in, e.g. `en`, `de`, `de-AT`.
    #[builder(into)]
    pub lang: String,
    /// The text itself.
    #[builder(into)]
    pub value: Text<150>,
}

impl InfoText {
    /// Builds a text in `lang`.
    ///
    /// # Errors
    ///
    /// Returns [`TextError::TooLong`] when `value` exceeds 150 characters.
    pub fn new(lang: impl Into<String>, value: impl Into<String>) -> Result<Self, TextError> {
        Ok(Self { lang: lang.into(), value: Text::new(value)? })
    }

    /// Whether `lang` matches the specification's language-code pattern.
    ///
    /// `^[a-z]{2,3}(?:-[A-Z]{2,3}(?:-[a-zA-Z]{4})?)?(?:-x-[a-zA-Z0-9]{1,8})?$`
    #[must_use]
    pub fn lang_is_well_formed(&self) -> bool {
        let mut parts = self.lang.split('-');
        let Some(primary) = parts.next() else { return false };
        if !matches!(primary.len(), 2 | 3) || !primary.bytes().all(|c| c.is_ascii_lowercase()) {
            return false;
        }
        let mut rest = parts.peekable();
        if let Some(region) = rest.peek()
            && *region != "x"
        {
            let region = rest.next().unwrap_or_default();
            if !matches!(region.len(), 2 | 3) || !region.bytes().all(|c| c.is_ascii_uppercase()) {
                return false;
            }
            if let Some(script) = rest.peek()
                && *script != "x"
            {
                let script = rest.next().unwrap_or_default();
                if script.len() != 4 || !script.bytes().all(|c| c.is_ascii_alphabetic()) {
                    return false;
                }
            }
        }
        match (rest.next(), rest.next(), rest.next()) {
            (None, _, _) => true,
            (Some("x"), Some(private), None) => {
                (1..=8).contains(&private.len()) && private.bytes().all(|c| c.is_ascii_alphanumeric())
            }
            _ => false,
        }
    }
}

impl Validate for InfoText {
    fn validate_in(&self, v: &mut Validator) {
        if !self.lang_is_well_formed() {
            v.report_at(
                "lang",
                ViolationCode::PatternMismatch,
                format!("{:?} is not an ISO-639-1 or ISO-639-2/T language code", self.lang),
            );
        }
        validate_fields!(self, v, value as "value");
    }
}

// Every builder's `build()` validates; `build_unchecked()` is the escape hatch.
strict_builder!(InfoText, InfoTextBuilder, info_text_builder);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_long_text_decodes_and_is_reported_but_cannot_be_constructed() {
        assert!(Text::<3>::new("Berlin").is_err());
        let arrived: Text<3> = serde_json::from_str(r#""Berlin""#).unwrap();
        assert_eq!(arrived.validate().unwrap_err().as_slice()[0].code, ViolationCode::TooLong);
        assert_eq!(serde_json::to_string(&arrived).unwrap(), r#""Berlin""#);
    }

    #[test]
    fn length_is_counted_in_characters_not_bytes() {
        // Five characters, ten bytes: a byte-length check would reject this wrongly.
        let text = Text::<5>::new("äöüßé").unwrap();
        assert_eq!(text.len(), 5);
        assert!(text.validate().is_ok());
    }

    #[test]
    fn language_codes_follow_the_spec_pattern() {
        for good in ["en", "de", "deu", "de-AT", "zh-CN-Hans", "en-x-custom"] {
            let t = InfoText::new(good, "x").unwrap();
            assert!(t.lang_is_well_formed(), "{good} should be valid");
            assert!(t.validate().is_ok());
        }
        for bad in ["", "E", "EN", "english", "de_AT", "de-at"] {
            let t = InfoText::new(bad, "x").unwrap();
            assert!(!t.lang_is_well_formed(), "{bad} should be invalid");
        }
    }
}
