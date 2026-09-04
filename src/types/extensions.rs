//! `Extensions` — the map that keeps JSON fields this crate has never heard of.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::validate::{Validate, Validator};

/// Undocumented JSON fields found on an OICP object, preserved verbatim.
///
/// # Why every object carries one
///
/// OICP 2.3 has no extensibility chapter and no version negotiation — but it *is* edited in place.
/// Hubject revises the 2.3 documents without bumping the version: `IsHubjectCompatible` and
/// `IsOpen24Hours` were added to `PullEvseData` this way, and the CDR schema gained a
/// `PartnerProductID` clarification in 2026. A partner's stack that was built against last year's
/// snapshot is still expected to forward this year's payloads intact.
///
/// So every wire object in this crate carries an `extensions` field marked `#[serde(flatten)]`.
/// A field that arrives, survives and is written back unchanged is the difference between a hub
/// that can sit between two parties who have agreed on something it knows nothing about, and one
/// that quietly destroys their data.
///
/// Keys are kept in a [`BTreeMap`], so serialisation order is deterministic.
///
/// ```
/// use oicp_kit::types::Extensions;
///
/// let json = r#"{"HubjectFutureField":"whatever","acme_note":3}"#;
/// let extensions: Extensions = serde_json::from_str(json).unwrap();
///
/// assert_eq!(extensions.get::<u32>("acme_note").unwrap(), Some(3));
/// assert_eq!(serde_json::to_string(&extensions).unwrap(), json);
/// ```
///
/// In place, on a real object:
///
#[cfg_attr(feature = "cpo", doc = "```rust")]
#[cfg_attr(not(feature = "cpo"), doc = "```rust,ignore")]
/// # use oicp_kit::cpo::EvseStatusRecord;
/// let json = r#"{"EvseID":"DE*XYZ*ETEST1","EvseStatus":"Available","HubjectAddedThis":42}"#;
/// let record: EvseStatusRecord = serde_json::from_str(json).unwrap();
/// assert_eq!(record.extensions.get::<u32>("HubjectAddedThis").unwrap(), Some(42));
/// assert_eq!(serde_json::to_string(&record).unwrap(), json);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Extensions(BTreeMap<String, serde_json::Value>);

impl Extensions {
    /// An empty set of extensions.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether no undocumented field was present.
    ///
    /// Objects skip serialising their `extensions` field when this is true, so an object that
    /// carried no extensions is written back byte-identically.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many undocumented fields are present.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The raw JSON value stored under `key`, if any.
    #[must_use]
    pub fn get_raw(&self, key: &str) -> Option<&serde_json::Value> {
        self.0.get(key)
    }

    /// Deserialises the value stored under `key` into `T`.
    ///
    /// Returns `Ok(None)` when the key is absent, and `Err` when it is present but does not
    /// deserialise into `T`.
    ///
    /// # Errors
    ///
    /// Returns the `serde_json` error when the stored value is not a `T`.
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>, serde_json::Error> {
        self.0.get(key).map(|v| serde_json::from_value(v.clone())).transpose()
    }

    /// Whether `key` is present.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// Stores `value` under `key`, returning what was there before.
    ///
    /// # Errors
    ///
    /// Returns the `serde_json` error when `value` cannot be serialised.
    pub fn insert<T: Serialize>(
        &mut self,
        key: impl Into<String>,
        value: T,
    ) -> Result<(), serde_json::Error> {
        self.0.insert(key.into(), serde_json::to_value(value)?);
        Ok(())
    }

    /// Removes `key`, returning the raw value that was stored.
    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        self.0.remove(key)
    }

    /// The keys present, in sorted order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// The entries, in sorted key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &serde_json::Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
}

impl Validate for Extensions {
    /// Extensions carry no spec constraints — that is the point of them.
    fn validate_in(&self, _v: &mut Validator) {}
}

impl FromIterator<(String, serde_json::Value)> for Extensions {
    fn from_iter<I: IntoIterator<Item = (String, serde_json::Value)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Extensions {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Extensions".into()
    }
    fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "object", "additionalProperties": true })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_survive_a_round_trip_in_sorted_order() {
        let json = r#"{"a_first":1,"z_last":"text"}"#;
        let ext: Extensions = serde_json::from_str(json).unwrap();
        assert_eq!(ext.len(), 2);
        assert_eq!(serde_json::to_string(&ext).unwrap(), json);
    }

    #[test]
    fn typed_access_reports_a_mismatch_rather_than_guessing() {
        let ext: Extensions = serde_json::from_str(r#"{"n":3,"s":"x"}"#).unwrap();
        assert_eq!(ext.get::<u32>("n").unwrap(), Some(3));
        assert_eq!(ext.get::<u32>("missing").unwrap(), None);
        assert!(ext.get::<u32>("s").is_err());
    }
}
