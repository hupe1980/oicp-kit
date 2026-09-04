//! The OICP identifier types: parsed, structured, and byte-exact on the wire.
//!
//! # Two grammars, one field
//!
//! Every identifier in OICP accepts **two** encodings, and the specification gives one regular
//! expression that unions them:
//!
//! * **ISO 15118-1** — `DE*AB7*E840*6487`, `DEAB7E8406487`. Alpha-2 country code, `*` optional.
//! * **DIN SPEC 91286:2011-11** — `+49*810*000*438`. ITU-T E.164 country code, `*` mandatory.
//!
//! A library that models these as `String` cannot tell an operator's ID from a provider's, cannot
//! tell you which country an EVSE is in, and — worse — invites the caller to normalise. A library
//! that *normalises* breaks production: Hubject compares the `OperatorID`/`ProviderID` in the URL
//! path against the partner's TLS client certificate, and answers a mismatch with status code
//! `017 Unauthorized Access`. `DE*ABC` and `DEABC` are the same operator to a human and to this
//! crate's [`Eq`], but they are different strings to a certificate check.
//!
//! So the types here are **parse-preserving**:
//!
//! * [`FromStr`] parses the grammar and records which standard matched.
//! * The components — country, operator, instance, check digit — are available as accessors.
//! * [`Display`](core::fmt::Display) and `Serialize` return **the exact text that arrived**.
//! * [`PartialEq`], [`Hash`] and [`Ord`] compare *semantically*: case-insensitively, ignoring the
//!   optional separators and the optional DIN `+`. `DE*ABC*E123 == deabce123` and
//!   `+49*810*000*438 == 49*810*000*438` — the spec writes each pair as one charging spot.
//!
//!   The standard a value follows is **not** part of its identity, and does not need to be: the
//!   two grammars are disjoint where they say different things (an ISO id begins with two letters,
//!   a DIN one with digits) and identical where they do not ([`ProviderId`] — the spec lists
//!   `DE8EO` under both headings). Folding the standard into equality instead buys nothing and
//!   costs transitivity, because a value that satisfies both grammars would have to compare equal
//!   to two values that differ from each other.
//!
//! ```
//! use oicp_kit::types::EvseId;
//!
//! let a: EvseId = "DE*AB7*E840*6487".parse()?;
//! let b: EvseId = "DEAB7E8406487".parse()?;
//!
//! assert_eq!(a, b);                                 // the same charging spot
//! assert_eq!(a.to_string(), "DE*AB7*E840*6487");    // …written back exactly as it arrived
//! assert_eq!(b.to_string(), "DEAB7E8406487");
//! assert_eq!(a.country(), "DE");
//! assert_eq!(a.operator_id().to_string(), "DE*AB7");
//! # Ok::<(), oicp_kit::types::IdError>(())
//! ```
//!
//! # Validation without a regex engine
//!
//! The specification states each grammar as a regular expression. This module implements those
//! grammars as hand-written character checks rather than pulling in a regex engine: the crate
//! stays dependency-light, parsing a page of 2000 EVSE records stays allocation-free per field,
//! and each rule sits next to the sentence of the spec it comes from. The
//! [`tests`](#) in this module check the hand-written parsers against the spec's own examples,
//! and `tests/properties.rs` fuzzes them for round-trip exactness.

use core::fmt;
use core::hash::{Hash, Hasher};
use core::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::validate::{Validate, Validator, ViolationCode};

/// Which of the two identifier standards a value follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IdStandard {
    /// ISO 15118-1: alpha-2 country code, separators optional.
    Iso,
    /// DIN SPEC 91286:2011-11: ITU-T E.164 numeric country code, separators mandatory.
    Din,
    /// The two grammars coincide for this identifier type, so the value does not say which.
    ///
    /// [`ProviderId`] is the case: the specification lists `DE8EO` and `DE-8EO` as examples under
    /// **both** headings, and only permits `*` as an additional separator under DIN. A value like
    /// `DE-8EO` is therefore simultaneously a valid ISO and a valid DIN provider id, and this is
    /// the honest answer to "which standard is it?" — not a guess at one of the two.
    Either,
}

impl IdStandard {
    /// A short, stable slug.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Iso => "ISO",
            Self::Din => "DIN",
            Self::Either => "ISO/DIN",
        }
    }
}

impl fmt::Display for IdStandard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a string is not a valid OICP identifier.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{value:?} is not a valid {type_name}: {reason}")]
pub struct IdError {
    type_name: &'static str,
    value: String,
    reason: &'static str,
}

impl IdError {
    fn new(type_name: &'static str, value: &str, reason: &'static str) -> Self {
        Self { type_name, value: value.to_owned(), reason }
    }

    /// The identifier type that rejected the value.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        self.type_name
    }

    /// The text that was rejected.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// The bytes that carry an identifier's meaning: upper case, without the optional separators and
/// without the optional DIN `+`.
///
/// The `+` matters. `EvseID`'s DIN grammar is `\+?[0-9]{1,3}\*…`, so `+49*810*000*438` and
/// `49*810*000*438` are the same charging spot — the specification prints both. Keeping the `+` in
/// the comparison makes them two, and an EMP that stores one and looks up the other finds nothing.
///
/// Allocation-free on purpose: this runs inside `Eq`, `Hash` and `Ord`, which a `BTreeMap` of a
/// few hundred thousand charging points calls on every operation.
fn significant_bytes(s: &str) -> impl Iterator<Item = u8> + '_ {
    s.strip_prefix('+').unwrap_or(s).bytes().filter(|c| !is_separator(*c)).map(|c| c.to_ascii_uppercase())
}

/// Compares two identifiers the way the roaming network does: ignoring ASCII case, the optional
/// `*` / `-` separators, and the optional DIN `+`.
fn semantic_eq(a: &str, b: &str) -> bool {
    significant_bytes(a).eq(significant_bytes(b))
}

/// A total order consistent with [`semantic_eq`], so `Ord` and `Eq` agree — which `BTreeMap`
/// requires and which comparing the canonical *strings* would also give, at the cost of two
/// allocations per comparison.
fn semantic_cmp(a: &str, b: &str) -> core::cmp::Ordering {
    significant_bytes(a).cmp(significant_bytes(b))
}

fn semantic_hash<H: Hasher>(s: &str, state: &mut H) {
    for c in significant_bytes(s) {
        state.write_u8(c);
    }
    state.write_u8(0xff);
}

/// Compares two strings ignoring ASCII case, without allocating.
fn case_insensitive_cmp(a: &str, b: &str) -> core::cmp::Ordering {
    a.bytes().map(|c| c.to_ascii_uppercase()).cmp(b.bytes().map(|c| c.to_ascii_uppercase()))
}

const fn is_separator(c: u8) -> bool {
    matches!(c, b'*' | b'-')
}

// Each grammar below walks the bytes with a cursor, guarding every step with
// `if b.len() < i + n || !b[i..i + n]…`. The `||` is what keeps the slice in bounds, and the guard
// only fires on an input short enough to reach it — which is why the tests feed every prefix of a
// valid value rather than a hand-picked set of lengths.

/// Generates the shared body of an identifier newtype: wire-exact storage, semantic equality,
/// serde, validation and schema.
macro_rules! oicp_id {
    (
        $(#[$meta:meta])*
        $name:ident, $type_name:literal
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug)]
        pub struct $name {
            /// Exactly the text that arrived on the wire.
            raw: String,
            standard: IdStandard,
        }

        impl $name {
            /// Parses `value`, keeping its text verbatim.
            ///
            /// # Errors
            ///
            /// Returns [`IdError`] when the value matches neither the ISO nor the DIN grammar.
            pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
                let raw = value.into();
                let standard = Self::classify(&raw)?;
                Ok(Self { raw, standard })
            }

            /// Accepts `value` **without** checking the grammar.
            ///
            /// For decoding a peer's payload, where a malformed id must arrive and be reported by
            /// [`Validate`] rather than break the whole page. `standard` is a best guess; the
            /// value is reported by [`Validate::validate`] either way.
            #[must_use]
            pub fn new_unchecked(value: impl Into<String>) -> Self {
                let raw = value.into();
                let standard = Self::classify(&raw).unwrap_or(if raw.starts_with('+') || raw.starts_with(|c: char| c.is_ascii_digit()) {
                    IdStandard::Din
                } else {
                    IdStandard::Iso
                });
                Self { raw, standard }
            }

            /// The identifier exactly as it appears on the wire.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.raw
            }

            /// Which standard this identifier follows.
            #[must_use]
            pub const fn standard(&self) -> IdStandard {
                self.standard
            }

            /// The identifier with every optional separator removed, upper-cased.
            ///
            /// Use this as a database key. Never put it on the wire: Hubject matches identifiers
            /// against the TLS client certificate as text.
            #[must_use]
            pub fn canonical(&self) -> String {
                significant_bytes(&self.raw).map(char::from).collect()
            }

            /// Whether this value satisfies the specification's grammar.
            #[must_use]
            pub fn is_well_formed(&self) -> bool {
                Self::classify(&self.raw).is_ok()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.raw)
            }
        }

        // `Eq`, `Hash` and `Ord` all read the same significant bytes, so they cannot disagree:
        // equal values hash alike and compare `Equal`, which is what a `HashMap` and a `BTreeMap`
        // of charging points each rely on. `standard` is metadata about the spelling and takes no
        // part — see the module documentation.
        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                semantic_eq(&self.raw, &other.raw)
            }
        }
        impl Eq for $name {}

        impl Hash for $name {
            fn hash<H: Hasher>(&self, state: &mut H) {
                semantic_hash(&self.raw, state);
            }
        }

        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $name {
            fn cmp(&self, other: &Self) -> core::cmp::Ordering {
                semantic_cmp(&self.raw, &other.raw)
            }
        }

        impl FromStr for $name {
            type Err = IdError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::new(s)
            }
        }

        impl Validate for $name {
            fn validate_in(&self, v: &mut Validator) {
                if let Err(e) = Self::classify(&self.raw) {
                    v.report(ViolationCode::PatternMismatch, e.to_string());
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.raw)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                // Permissive by design: a malformed id on one record of a 2000-record page must
                // not fail the page. `Validate` reports it.
                Ok(Self::new_unchecked(String::deserialize(d)?))
            }
        }

        #[cfg(feature = "schema")]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> { $type_name.into() }
            fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({ "type": "string" })
            }
        }
    };
}

// --- EvseID -------------------------------------------------------------------------------

oicp_id! {
    /// The ID that identifies a charging spot.
    ///
    /// Spec pattern:
    /// `^(([A-Za-z]{2}\*?[A-Za-z0-9]{3}\*?E[A-Za-z0-9\*]{1,30})|(\+?[0-9]{1,3}\*[0-9]{3}\*[0-9\*]{1,32}))$`
    ///
    /// ISO: `DE*AB7*E840*6487`, `DEAB7E8406487` — the `E` after the operator part is what marks an
    /// id as ISO beyond doubt. DIN: `+49*810*000*438`.
    EvseId, "EvseID"
}

impl EvseId {
    fn classify(s: &str) -> Result<IdStandard, IdError> {
        if let Ok(()) = Self::check_iso(s) {
            return Ok(IdStandard::Iso);
        }
        Self::check_din(s).map(|()| IdStandard::Din)
    }

    /// `[A-Za-z]{2} \*? [A-Za-z0-9]{3} \*? E [A-Za-z0-9\*]{1,30}`
    fn check_iso(s: &str) -> Result<(), IdError> {
        let b = s.as_bytes();
        let mut i = 0;
        let err = |reason| Err(IdError::new("EvseID", s, reason));
        if b.len() < 2 || !b[0].is_ascii_alphabetic() || !b[1].is_ascii_alphabetic() {
            return err("an ISO EvseID starts with a two-letter country code");
        }
        i += 2;
        if b.get(i) == Some(&b'*') {
            i += 1;
        }
        if b.len() < i + 3 || !b[i..i + 3].iter().all(u8::is_ascii_alphanumeric) {
            return err("the operator part of an ISO EvseID is three alphanumeric characters");
        }
        i += 3;
        if b.get(i) == Some(&b'*') {
            i += 1;
        }
        if b.get(i).is_none_or(|c| !c.eq_ignore_ascii_case(&b'E')) {
            return err("an ISO EvseID has an 'E' after the operator part");
        }
        i += 1;
        let rest = &b[i..];
        if rest.is_empty() || rest.len() > 30 {
            return err("the instance part of an ISO EvseID is 1 to 30 characters");
        }
        if !rest.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'*') {
            return err("the instance part of an ISO EvseID is alphanumeric with optional '*' separators");
        }
        Ok(())
    }

    /// `\+? [0-9]{1,3} \* [0-9]{3} \* [0-9\*]{1,32}`
    fn check_din(s: &str) -> Result<(), IdError> {
        let err = |reason| Err(IdError::new("EvseID", s, reason));
        let body = s.strip_prefix('+').unwrap_or(s);
        let mut parts = body.splitn(3, '*');
        let (Some(country), Some(operator), Some(instance)) = (parts.next(), parts.next(), parts.next())
        else {
            return err("a DIN EvseID is country*operator*instance, separated by mandatory '*'");
        };
        if country.is_empty() || country.len() > 3 || !country.bytes().all(|c| c.is_ascii_digit()) {
            return err("the country code of a DIN EvseID is 1 to 3 digits (ITU-T E.164)");
        }
        if operator.len() != 3 || !operator.bytes().all(|c| c.is_ascii_digit()) {
            return err("the operator part of a DIN EvseID is exactly three digits");
        }
        if instance.is_empty()
            || instance.len() > 32
            || !instance.bytes().all(|c| c.is_ascii_digit() || c == b'*')
        {
            return err("the instance part of a DIN EvseID is 1 to 32 digits with optional '*' separators");
        }
        Ok(())
    }

    /// The two- or three-character country code this EVSE belongs to.
    ///
    /// Alpha-2 (ISO 3166-1) for an ISO id, numeric (ITU-T E.164) for a DIN one.
    #[must_use]
    pub fn country(&self) -> &str {
        match self.standard() {
            IdStandard::Din => {
                let body = self.as_str().strip_prefix('+').unwrap_or(self.as_str());
                body.split('*').next().unwrap_or("")
            }
            // An EvseID always distinguishes the two grammars, so `Either` cannot occur here.
            IdStandard::Iso | IdStandard::Either => &self.as_str()[..2],
        }
    }

    /// The operator that runs this charging spot.
    ///
    /// OICP has no `OperatorID` field on most messages — Hubject *derives* the operator from the
    /// `EvseID`, and so can you. Used by [`MockHubject`](crate::testkit::MockHubject) for routing.
    #[must_use]
    pub fn operator_id(&self) -> OperatorId {
        let raw = match self.standard() {
            IdStandard::Iso | IdStandard::Either => {
                // country [sep] operator, stopping before the 'E' marker.
                let b = self.as_str().as_bytes();
                let mut end = 2;
                if b.get(end) == Some(&b'*') {
                    end += 1;
                }
                end += 3;
                &self.as_str()[..end]
            }
            IdStandard::Din => {
                let s = self.as_str();
                let plus = usize::from(s.starts_with('+'));
                let body = &s[plus..];
                match body.match_indices('*').nth(1) {
                    Some((idx, _)) => &s[..plus + idx],
                    None => s,
                }
            }
        };
        OperatorId::new_unchecked(raw)
    }
}

// --- EvcoID -------------------------------------------------------------------------------

oicp_id! {
    /// The contract ID that identifies an EV driver's contract with an EMP.
    ///
    /// Spec pattern:
    /// `^(([A-Za-z]{2}\-?[A-Za-z0-9]{3}\-?C[A-Za-z0-9]{8}\-?[\d|A-Za-z])|([A-Za-z]{2}[\*|\-]?[A-Za-z0-9]{3}[\*|\-]?[A-Za-z0-9]{6}[\*|\-]?[\d|X]))$`
    ///
    /// ISO: eight-character instance prefixed with `C`, separator `-`, then a check character —
    /// `DE-8EO-CAet5e4XY-3`. DIN: six-character instance, separator `*` or `-` —
    /// `DE*8EO*Aet5e4*3`.
    EvcoId, "EvcoID"
}

impl EvcoId {
    fn classify(s: &str) -> Result<IdStandard, IdError> {
        if Self::check_iso(s).is_ok() {
            return Ok(IdStandard::Iso);
        }
        Self::check_din(s).map(|()| IdStandard::Din)
    }

    /// `[A-Za-z]{2} \-? [A-Za-z0-9]{3} \-? C [A-Za-z0-9]{8} \-? [\d|A-Za-z]`
    fn check_iso(s: &str) -> Result<(), IdError> {
        let b = s.as_bytes();
        let err = |reason| Err(IdError::new("EvcoID", s, reason));
        let mut i = 0;
        if b.len() < 2 || !b[0].is_ascii_alphabetic() || !b[1].is_ascii_alphabetic() {
            return err("an ISO EvcoID starts with a two-letter country code");
        }
        i += 2;
        if b.get(i) == Some(&b'-') {
            i += 1;
        }
        if b.len() < i + 3 || !b[i..i + 3].iter().all(u8::is_ascii_alphanumeric) {
            return err("the provider part of an ISO EvcoID is three alphanumeric characters");
        }
        i += 3;
        if b.get(i) == Some(&b'-') {
            i += 1;
        }
        if b.get(i).is_none_or(|c| !c.eq_ignore_ascii_case(&b'C')) {
            return err("an ISO EvcoID prefixes its instance part with 'C'");
        }
        i += 1;
        if b.len() < i + 8 || !b[i..i + 8].iter().all(u8::is_ascii_alphanumeric) {
            return err("the instance part of an ISO EvcoID is exactly eight alphanumeric characters");
        }
        i += 8;
        if b.get(i) == Some(&b'-') {
            i += 1;
        }
        if b.len() != i + 1 || !b[i].is_ascii_alphanumeric() {
            return err("an ISO EvcoID ends with a single check character");
        }
        Ok(())
    }

    /// `[A-Za-z]{2} [\*\-]? [A-Za-z0-9]{3} [\*\-]? [A-Za-z0-9]{6} [\*\-]? [\dX]`
    fn check_din(s: &str) -> Result<(), IdError> {
        let b = s.as_bytes();
        let err = |reason| Err(IdError::new("EvcoID", s, reason));
        let mut i = 0;
        if b.len() < 2 || !b[0].is_ascii_alphabetic() || !b[1].is_ascii_alphabetic() {
            return err("a DIN EvcoID starts with a two-letter country code");
        }
        i += 2;
        if b.get(i).is_some_and(|c| is_separator(*c)) {
            i += 1;
        }
        if b.len() < i + 3 || !b[i..i + 3].iter().all(u8::is_ascii_alphanumeric) {
            return err("the provider part of a DIN EvcoID is three alphanumeric characters");
        }
        i += 3;
        if b.get(i).is_some_and(|c| is_separator(*c)) {
            i += 1;
        }
        if b.len() < i + 6 || !b[i..i + 6].iter().all(u8::is_ascii_alphanumeric) {
            return err("the instance part of a DIN EvcoID is exactly six alphanumeric characters");
        }
        i += 6;
        if b.get(i).is_some_and(|c| is_separator(*c)) {
            i += 1;
        }
        if b.len() != i + 1 || !(b[i].is_ascii_digit() || b[i].eq_ignore_ascii_case(&b'X')) {
            return err("a DIN EvcoID ends with a check digit or 'X'");
        }
        Ok(())
    }

    /// The EMP that issued this contract.
    ///
    /// Hubject routes an authorization to the EMP named by the EvcoID's provider part, so this is
    /// how a CPO — or [`MockHubject`](crate::testkit::MockHubject) — finds the counterparty.
    #[must_use]
    pub fn provider_id(&self) -> ProviderId {
        let b = self.as_str().as_bytes();
        let mut end = 2;
        if b.get(end).is_some_and(|c| is_separator(*c)) {
            end += 1;
        }
        end = (end + 3).min(self.as_str().len());
        ProviderId::new_unchecked(&self.as_str()[..end])
    }
}

// --- OperatorID ---------------------------------------------------------------------------

oicp_id! {
    /// Identifies a Charge Point Operator, including its country code.
    ///
    /// Spec pattern: `^(([A-Za-z]{2}\*?[A-Za-z0-9]{3})|(\+?[0-9]{1,3}\*[0-9]{3}))$`
    ///
    /// ISO: `DE*A36`, `DEA36`. DIN: `+49*536`.
    ///
    /// This is the value Hubject matches against the partner's TLS client certificate when it
    /// appears in a URL path, which is why it is never rewritten on the way out.
    OperatorId, "OperatorID"
}

impl OperatorId {
    fn classify(s: &str) -> Result<IdStandard, IdError> {
        let b = s.as_bytes();
        let err = |reason| Err(IdError::new("OperatorID", s, reason));
        // ISO: two letters, optional '*', three alphanumerics.
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1].is_ascii_alphabetic() {
            let rest = if b.get(2) == Some(&b'*') { &b[3..] } else { &b[2..] };
            if rest.len() == 3 && rest.iter().all(u8::is_ascii_alphanumeric) {
                return Ok(IdStandard::Iso);
            }
            return err("an ISO OperatorID is a two-letter country code and three alphanumeric characters");
        }
        // DIN: optional '+', 1-3 digits, '*', exactly three digits.
        let body = s.strip_prefix('+').unwrap_or(s);
        let mut parts = body.splitn(2, '*');
        let (Some(country), Some(operator)) = (parts.next(), parts.next()) else {
            return err("a DIN OperatorID is country*operator with a mandatory '*'");
        };
        if country.is_empty() || country.len() > 3 || !country.bytes().all(|c| c.is_ascii_digit()) {
            return err("the country code of a DIN OperatorID is 1 to 3 digits");
        }
        if operator.len() != 3 || !operator.bytes().all(|c| c.is_ascii_digit()) {
            return err("the operator part of a DIN OperatorID is exactly three digits");
        }
        Ok(IdStandard::Din)
    }

    /// The country code of this operator.
    #[must_use]
    pub fn country(&self) -> &str {
        match self.standard() {
            IdStandard::Din => {
                let body = self.as_str().strip_prefix('+').unwrap_or(self.as_str());
                body.split('*').next().unwrap_or("")
            }
            // An OperatorID always distinguishes the two grammars, so `Either` cannot occur here.
            IdStandard::Iso | IdStandard::Either => &self.as_str()[..2],
        }
    }
}

// --- ProviderID ---------------------------------------------------------------------------

oicp_id! {
    /// Identifies an e-Mobility Provider. Assigned by Hubject.
    ///
    /// Spec pattern: `^([A-Za-z]{2}\-?[A-Za-z0-9]{3}|[A-Za-z]{2}[\*|-]?[A-Za-z0-9]{3})$`
    ///
    /// `DE8EO`, `DE-8EO`, `DE*8EO` are the same provider.
    ///
    /// This is the value Hubject matches against the partner's TLS client certificate when it
    /// appears in a URL path, which is why it is never rewritten on the way out.
    ProviderId, "ProviderID"
}

impl ProviderId {
    fn classify(s: &str) -> Result<IdStandard, IdError> {
        let b = s.as_bytes();
        let err = |reason| Err(IdError::new("ProviderID", s, reason));
        if b.len() < 2 || !b[0].is_ascii_alphabetic() || !b[1].is_ascii_alphabetic() {
            return err("a ProviderID starts with a two-letter country code");
        }
        let (rest, separated) =
            if b.get(2).is_some_and(|c| is_separator(*c)) { (&b[3..], true) } else { (&b[2..], false) };
        if rest.len() != 3 || !rest.iter().all(u8::is_ascii_alphanumeric) {
            return err("a ProviderID is a two-letter country code and three alphanumeric characters");
        }
        // The spec lists `DE8EO` and `DE-8EO` under *both* the ISO and the DIN heading and adds
        // `DE*8EO` under DIN, so no provider id distinguishes the two grammars. Claiming one
        // would make `DE-8EO` and `DE*8EO` — the same EMP — compare unequal.
        let _ = separated;
        Ok(IdStandard::Either)
    }

    /// The country code of this provider.
    #[must_use]
    pub fn country(&self) -> &str {
        &self.as_str()[..2]
    }
}

// --- ChargingPoolID -----------------------------------------------------------------------

oicp_id! {
    /// Groups EVSEs into a charging pool, per the emi³ standard definition.
    ///
    /// Spec pattern: `([A-Za-z]{2}\*?[A-Za-z0-9]{3}\*?P[A-Za-z0-9\*]{1,30})` — the ISO `EvseID`
    /// grammar with `P` (pool) in place of `E`. There is no DIN form and no check digit.
    ///
    /// Example: `IT*123*P456*AB789`.
    ChargingPoolId, "ChargingPoolID"
}

impl ChargingPoolId {
    fn classify(s: &str) -> Result<IdStandard, IdError> {
        let b = s.as_bytes();
        let err = |reason| Err(IdError::new("ChargingPoolID", s, reason));
        let mut i = 0;
        if b.len() < 2 || !b[0].is_ascii_alphabetic() || !b[1].is_ascii_alphabetic() {
            return err("a ChargingPoolID starts with a two-letter country code");
        }
        i += 2;
        if b.get(i) == Some(&b'*') {
            i += 1;
        }
        if b.len() < i + 3 || !b[i..i + 3].iter().all(u8::is_ascii_alphanumeric) {
            return err("the operator part of a ChargingPoolID is three alphanumeric characters");
        }
        i += 3;
        if b.get(i) == Some(&b'*') {
            i += 1;
        }
        if b.get(i).is_none_or(|c| !c.eq_ignore_ascii_case(&b'P')) {
            return err("a ChargingPoolID has a 'P' after the operator part, marking it as a pool");
        }
        i += 1;
        let rest = &b[i..];
        if rest.is_empty() || rest.len() > 30 {
            return err("the pool part of a ChargingPoolID is 1 to 30 characters");
        }
        if !rest.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'*') {
            return err("the pool part of a ChargingPoolID is alphanumeric with optional '*' separators");
        }
        Ok(IdStandard::Iso)
    }
}

// --- SessionID ----------------------------------------------------------------------------

/// The Hubject session identifier that ties an authorization, its charging process and its CDR
/// together.
///
/// Spec pattern: `^[A-Za-z0-9]{8}(-[A-Za-z0-9]{4}){3}-[A-Za-z0-9]{12}$` — GUID-shaped, but note
/// that OICP allows *letters* in every group, so it is not necessarily a hex UUID.
///
/// Example: `b2688855-7f00-0002-6d8e-48d883f6abb6`.
#[derive(Clone, Debug)]
pub struct SessionId(String);

impl SessionId {
    /// Parses `value`.
    ///
    /// # Errors
    ///
    /// Returns [`IdError`] when the value does not have the GUID shape OICP requires.
    pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
        let raw = value.into();
        Self::check(&raw)?;
        Ok(Self(raw))
    }

    /// Accepts `value` without checking the grammar; [`Validate`] reports it.
    #[must_use]
    pub fn new_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The session id exactly as it appears on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this value satisfies the specification's grammar.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        Self::check(&self.0).is_ok()
    }

    fn check(s: &str) -> Result<(), IdError> {
        let err = |reason| Err(IdError::new("SessionID", s, reason));
        let groups: Vec<&str> = s.split('-').collect();
        if groups.len() != 5 {
            return err("a SessionID has five '-'-separated groups");
        }
        let expected = [8usize, 4, 4, 4, 12];
        for (group, want) in groups.iter().zip(expected) {
            if group.len() != want || !group.bytes().all(|c| c.is_ascii_alphanumeric()) {
                return err("a SessionID is 8-4-4-4-12 alphanumeric characters");
            }
        }
        Ok(())
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq for SessionId {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}
impl Eq for SessionId {}

impl Hash for SessionId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for c in self.0.bytes() {
            state.write_u8(c.to_ascii_uppercase());
        }
    }
}

impl PartialOrd for SessionId {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SessionId {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        case_insensitive_cmp(&self.0, &other.0)
    }
}

impl FromStr for SessionId {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Validate for SessionId {
    fn validate_in(&self, v: &mut Validator) {
        if let Err(e) = Self::check(&self.0) {
            v.report(ViolationCode::PatternMismatch, e.to_string());
        }
    }
}

impl Serialize for SessionId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SessionId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::new_unchecked(String::deserialize(d)?))
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for SessionId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SessionID".into()
    }
    fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string" })
    }
}

// --- UID ----------------------------------------------------------------------------------

/// An RFID card's unique identifier.
///
/// Spec pattern: `^([0-9A-F]{8,8}|[0-9A-F]{14,14}|[0-9A-F]{20,20})$` — uppercase hexadecimal, of
/// exactly 8, 14 or 20 characters (single, double and triple-length Mifare UIDs).
///
/// # Case
///
/// Equality is case-insensitive: a reader that reports `7568290fff765f` has read the same card as
/// one that reports `7568290FFF765F`, and an EMP whose blocklist misses that has a real problem.
/// The wire form is still preserved exactly.
///
/// But the specification writes `[0-9A-F]`, so a lowercase UID is a violation: it **decodes** via
/// [`new_unchecked`](Self::new_unchecked), compares equal to its uppercase twin, and
/// [`canonical`](Self::canonical) gives the key — but [`new`](Self::new) refuses it and
/// [`Validate`] reports it, because a case-sensitive peer will not match what this crate emits.
#[derive(Clone, Debug)]
pub struct Uid(String);

impl Uid {
    /// Parses `value`.
    ///
    /// # Errors
    ///
    /// Returns [`IdError`] unless the value is 8, 14 or 20 hexadecimal characters.
    pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
        let raw = value.into();
        Self::check(&raw)?;
        Ok(Self(raw))
    }

    /// Accepts `value` without checking the grammar; [`Validate`] reports it.
    #[must_use]
    pub fn new_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The UID exactly as it appears on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The UID upper-cased — the form to use as a lookup key.
    #[must_use]
    pub fn canonical(&self) -> String {
        self.0.to_ascii_uppercase()
    }

    /// Whether this value satisfies the specification's grammar.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        Self::check(&self.0).is_ok()
    }

    fn check(s: &str) -> Result<(), IdError> {
        if !matches!(s.len(), 8 | 14 | 20) {
            return Err(IdError::new("UID", s, "an RFID UID is 8, 14 or 20 characters"));
        }
        if !s.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(IdError::new("UID", s, "an RFID UID is hexadecimal"));
        }
        // The specification writes `[0-9A-F]`. Readers that emit lower case are common, so a
        // lowercase UID must *decode* — `new_unchecked` accepts it, `Eq` matches it against its
        // uppercase twin, and `canonical()` gives the key. But this crate does not *emit* one,
        // because a case-sensitive peer would not match it.
        if s.bytes().any(|c| c.is_ascii_lowercase()) {
            return Err(IdError::new(
                "UID",
                s,
                "the specification writes an RFID UID in uppercase hexadecimal",
            ));
        }
        Ok(())
    }
}

impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq for Uid {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}
impl Eq for Uid {}

impl Hash for Uid {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for c in self.0.bytes() {
            state.write_u8(c.to_ascii_uppercase());
        }
    }
}

impl PartialOrd for Uid {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Uid {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        case_insensitive_cmp(&self.0, &other.0)
    }
}

impl FromStr for Uid {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Validate for Uid {
    fn validate_in(&self, v: &mut Validator) {
        if let Err(e) = Self::check(&self.0) {
            v.report(ViolationCode::PatternMismatch, e.to_string());
        }
    }
}

impl Serialize for Uid {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Uid {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::new_unchecked(String::deserialize(d)?))
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Uid {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "UID".into()
    }
    fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string" })
    }
}

// --- ProviderIdOrAll ----------------------------------------------------------------------

/// A [`ProviderId`], or the literal `*` meaning "every subscribed EMP".
///
/// The pricing pushes (`PushPricingProductData`, `PushEVSEPricing`) accept an asterisk in place of
/// a provider id, for offer-to-all prices. Modelling that as a `String` invites a CPO to publish
/// a price to the entire roaming network by typo, so it is a distinct variant instead.
///
/// Spec: `ProviderIDAsterisk`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProviderIdOrAll {
    /// One specific EMP.
    One(ProviderId),
    /// Every EMP subscribed to the operator's service — the literal `*`.
    All,
}

impl ProviderIdOrAll {
    /// The value as it appears on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::One(id) => id.as_str(),
            Self::All => "*",
        }
    }

    /// Whether this addresses every subscribed EMP.
    #[must_use]
    pub const fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }
}

impl fmt::Display for ProviderIdOrAll {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<ProviderId> for ProviderIdOrAll {
    fn from(id: ProviderId) -> Self {
        Self::One(id)
    }
}

impl FromStr for ProviderIdOrAll {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "*" { Ok(Self::All) } else { ProviderId::new(s).map(Self::One) }
    }
}

impl Validate for ProviderIdOrAll {
    fn validate_in(&self, v: &mut Validator) {
        if let Self::One(id) = self {
            id.validate_in(v);
        }
    }
}

impl Serialize for ProviderIdOrAll {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProviderIdOrAll {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(if s == "*" { Self::All } else { Self::One(ProviderId::new_unchecked(s)) })
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for ProviderIdOrAll {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ProviderIDAsterisk".into()
    }
    fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evse_id_accepts_every_example_the_spec_gives() {
        for (text, standard) in [
            ("DE*AB7*E840*6487", IdStandard::Iso),
            ("DEAB7E8406487", IdStandard::Iso),
            ("DE*XYZ*ETEST1", IdStandard::Iso),
            ("+49*810*000*438", IdStandard::Din),
            ("49*810*000*438", IdStandard::Din),
        ] {
            let id: EvseId = text.parse().unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(id.standard(), standard, "{text}");
            assert_eq!(id.to_string(), text, "{text} was rewritten");
        }
    }

    #[test]
    fn evse_ids_that_differ_only_in_separators_and_case_are_equal() {
        let a: EvseId = "DE*AB7*E840*6487".parse().unwrap();
        let b: EvseId = "DEAB7E8406487".parse().unwrap();
        let c: EvseId = "de*ab7*e840*6487".parse().unwrap();
        assert_eq!(a, b);
        assert_eq!(a, c);
        // …and each keeps its own text.
        assert_eq!(a.to_string(), "DE*AB7*E840*6487");
        assert_eq!(b.to_string(), "DEAB7E8406487");
    }

    #[test]
    fn an_iso_id_never_equals_a_din_one() {
        // Not because the standard is compared, but because the grammars cannot collide: an ISO
        // id starts with two letters and a DIN one with digits.
        let iso: EvseId = "DE*AB7*E840*6487".parse().unwrap();
        let din: EvseId = "+49*810*000*438".parse().unwrap();
        assert_ne!(iso, din);
    }

    #[test]
    fn the_optional_din_plus_does_not_make_a_second_charging_spot() {
        // `\+?[0-9]{1,3}\*…`: the specification prints both spellings of the same EVSE.
        let with: EvseId = "+49*810*000*438".parse().unwrap();
        let without: EvseId = "49*810*000*438".parse().unwrap();

        assert_eq!(with, without);
        assert_eq!(with.cmp(&without), core::cmp::Ordering::Equal);
        assert_eq!(with.canonical(), without.canonical());
        assert_eq!(with.to_string(), "+49*810*000*438", "and each keeps its own text");
        assert_eq!(with.operator_id(), without.operator_id());
    }

    #[test]
    fn ordering_agrees_with_equality_for_every_identifier() {
        // `BTreeMap` needs `cmp(a, b) == Equal` to mean `a == b`; a `HashMap` needs equal values
        // to hash alike. One set of significant bytes feeds all three.
        use std::hash::{BuildHasher as _, RandomState};
        let hasher = RandomState::new();
        let spellings = ["DE*AB7*E840*6487", "DEAB7E8406487", "de-ab7-e840-6487", "DE*XYZ*ETEST1"];
        for a in spellings {
            for b in spellings {
                let (x, y) = (EvseId::new_unchecked(a), EvseId::new_unchecked(b));
                assert_eq!(
                    (x.cmp(&y) == core::cmp::Ordering::Equal),
                    x == y,
                    "Ord and Eq disagree on {a:?} vs {b:?}"
                );
                if x == y {
                    assert_eq!(hasher.hash_one(&x), hasher.hash_one(&y), "{a:?} and {b:?} hash apart");
                } else {
                    // Without this half, a `Hash` that hashes nothing passes: every key collides,
                    // the map degrades to a linear scan, and no test notices.
                    assert_ne!(hasher.hash_one(&x), hasher.hash_one(&y), "{a:?} and {b:?} hash alike");
                }
            }
        }
    }

    #[test]
    fn the_operator_is_derived_from_the_evse_id_the_way_hubject_does_it() {
        let iso: EvseId = "DE*AB7*E840*6487".parse().unwrap();
        assert_eq!(iso.operator_id().to_string(), "DE*AB7");
        assert_eq!(iso.country(), "DE");

        let packed: EvseId = "DEAB7E8406487".parse().unwrap();
        assert_eq!(packed.operator_id().to_string(), "DEAB7");
        // Same operator, whichever way the EvseID was written.
        assert_eq!(iso.operator_id(), packed.operator_id());

        let din: EvseId = "+49*810*000*438".parse().unwrap();
        assert_eq!(din.operator_id().to_string(), "+49*810");
        assert_eq!(din.country(), "49");
    }

    #[test]
    fn evco_id_accepts_every_example_the_spec_gives() {
        for (text, standard) in [
            ("DE-8EO-CAet5e4XY-3", IdStandard::Iso),
            ("DE8EOCAet5e43X1", IdStandard::Iso),
            ("DE*8EO*Aet5e4*3", IdStandard::Din),
            ("DE-8EO-Aet5e4-3", IdStandard::Din),
            ("DE8EOAet5e43", IdStandard::Din),
            ("DE-DCB-C12345678-X", IdStandard::Iso),
        ] {
            let id: EvcoId = text.parse().unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(id.standard(), standard, "{text}");
            assert_eq!(id.to_string(), text);
        }
    }

    #[test]
    fn the_provider_is_derived_from_the_evco_id() {
        let iso: EvcoId = "DE-8EO-CAet5e4XY-3".parse().unwrap();
        assert_eq!(iso.provider_id().to_string(), "DE-8EO");
        let din: EvcoId = "DE*8EO*Aet5e4*3".parse().unwrap();
        assert_eq!(din.provider_id().to_string(), "DE*8EO");
        assert_eq!(iso.provider_id(), din.provider_id());
    }

    #[test]
    fn operator_and_provider_ids_accept_the_spec_examples() {
        for text in ["DE*A36", "DEA36", "+49*536", "49*536"] {
            let id: OperatorId = text.parse().unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(id.to_string(), text);
        }
        for text in ["DE8EO", "DE-8EO", "DE*8EO"] {
            let id: ProviderId = text.parse().unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(id.to_string(), text);
            assert_eq!(id.country(), "DE");
        }
    }

    #[test]
    fn malformed_ids_are_rejected_by_new_but_survive_deserialisation() {
        assert!("not-an-evse-id".parse::<EvseId>().is_err());
        assert!("DE*AB7*840*6487".parse::<EvseId>().is_err()); // no 'E' marker

        // A bad id on one record must not fail a 2000-record page.
        let id: EvseId = serde_json::from_str(r#""garbage""#).unwrap();
        assert!(!id.is_well_formed());
        assert_eq!(serde_json::to_string(&id).unwrap(), r#""garbage""#);
        assert_eq!(id.validate().unwrap_err().as_slice()[0].code, ViolationCode::PatternMismatch);
    }

    #[test]
    fn every_prefix_of_a_valid_identifier_is_rejected_without_panicking() {
        // Each grammar walks the bytes with a cursor and guards every step with
        // `if b.len() < i + n || !b[i..i + n]…`. The `||` is what stops the slice from going out
        // of bounds, and the guard only *fires* on an input short enough to reach it — which no
        // test had. Feeding every prefix of a valid value walks the cursor to each guard in turn,
        // so all of them are exercised by construction rather than by enumeration.
        //
        // It matters because the crate's promise is that a malformed identifier decodes and is
        // reported, never panics: `fuzz/fuzz_targets/identifiers.rs` asserts exactly that against
        // arbitrary bytes, and this is the same property with the interesting inputs named.
        let valid = [
            "DE*AB7*E840*6487",
            "DEAB7E8406487",
            "+49*810*000*438",
            "DE-8EO-CAet5e4XY-3",
            "DE*8EO*Aet5e4*3",
            "DE*A36",
            "+49*536",
            "DE-8EO",
            "IT*123*P456*AB789",
            "b2688855-7f00-0002-6d8e-48d883f6abb6",
            "7568290FFF765F",
        ];

        for text in valid {
            // Every prefix, and one character past the end of every bound.
            let extended = format!("{text}9");
            for cut in 0..=extended.len() {
                let candidate = &extended[..cut];

                // Every type sees every candidate: a truncated EvcoID is a perfectly good thing to
                // hand an EvseID parser, and the point is that nothing anywhere unwinds. The
                // contract asserted is the one a caller relies on — the cheap question and the
                // detailed one agree — which is also what makes an unreached guard visible, since
                // a guard that cannot fire cannot make them disagree.
                assert_eq!(
                    EvseId::new_unchecked(candidate).is_well_formed(),
                    EvseId::new(candidate).is_ok(),
                    "EvseID disagreed with itself on {candidate:?}"
                );
                assert_eq!(
                    EvcoId::new_unchecked(candidate).is_well_formed(),
                    EvcoId::new(candidate).is_ok(),
                    "EvcoID disagreed with itself on {candidate:?}"
                );
                assert_eq!(
                    OperatorId::new_unchecked(candidate).is_well_formed(),
                    OperatorId::new(candidate).is_ok(),
                    "OperatorID disagreed with itself on {candidate:?}"
                );
                assert_eq!(
                    ProviderId::new_unchecked(candidate).is_well_formed(),
                    ProviderId::new(candidate).is_ok(),
                    "ProviderID disagreed with itself on {candidate:?}"
                );
                assert_eq!(
                    ChargingPoolId::new_unchecked(candidate).is_well_formed(),
                    ChargingPoolId::new(candidate).is_ok(),
                    "ChargingPoolID disagreed with itself on {candidate:?}"
                );
                assert_eq!(
                    SessionId::new_unchecked(candidate).is_well_formed(),
                    SessionId::new(candidate).is_ok(),
                    "SessionID disagreed with itself on {candidate:?}"
                );
                assert_eq!(
                    Uid::new_unchecked(candidate).is_well_formed(),
                    Uid::new(candidate).is_ok(),
                    "UID disagreed with itself on {candidate:?}"
                );
            }
        }

        // The prefixes that must specifically be refused, named so the reason is on the record.
        assert!("DE".parse::<EvseId>().is_err(), "no operator part at all");
        assert!("DE*A".parse::<EvseId>().is_err(), "one operator character");
        assert!("DE*AB7*E".parse::<EvseId>().is_err(), "no instance after the E marker");
        assert!("DE-8EO-C".parse::<EvcoId>().is_err(), "no instance after the C marker");
        assert!("IT*123*P".parse::<ChargingPoolId>().is_err(), "no pool part after the P marker");
        assert!("DE".parse::<OperatorId>().is_err(), "a country code is not an OperatorID");
        assert!("DE".parse::<ProviderId>().is_err(), "nor a ProviderID");
    }

    #[test]
    fn both_characters_of_a_country_code_must_be_letters() {
        // Every grammar opens with `!b[0].is_ascii_alphabetic() || !b[1].is_ascii_alphabetic()`,
        // and the two halves need separate cases: written as `&&`, the check demands that *both*
        // characters be wrong before it complains, so a country code with one digit in it sails
        // through and the whole identifier is accepted. `DE` is the only shape the specification
        // allows, and a value like `D3*AB7*E840` is one keystroke away from a real one.
        for bad in [
            "1EAB7E840",        // EvseID, first character a digit
            "E1AB7E840",        // EvseID, second character a digit
            "D3*AB7*E840*6487", // …and with the separators in place
        ] {
            assert!(bad.parse::<EvseId>().is_err(), "{bad} should be rejected");
        }
        for bad in ["1E-8EO-CAet5e4XY-3", "D3-8EO-CAet5e4XY-3", "1E*8EO*Aet5e4*3", "D3*8EO*Aet5e4*3"] {
            assert!(bad.parse::<EvcoId>().is_err(), "{bad} should be rejected");
        }
        for bad in ["1E8EO", "D38EO", "1E-8EO"] {
            assert!(bad.parse::<ProviderId>().is_err(), "{bad} should be rejected");
        }
        for bad in ["1T*123*P456", "I3*123*P456"] {
            assert!(bad.parse::<ChargingPoolId>().is_err(), "{bad} should be rejected");
        }
        for bad in ["1E*A36", "D3*A36"] {
            assert!(bad.parse::<OperatorId>().is_err(), "{bad} should be rejected");
        }

        // The valid neighbours, so the check is not simply refusing everything.
        assert!("DE*AB7*E840*6487".parse::<EvseId>().is_ok());
        assert!("DE-8EO-CAet5e4XY-3".parse::<EvcoId>().is_ok());
        assert!("DE-8EO".parse::<ProviderId>().is_ok());
        assert!("IT*123*P456".parse::<ChargingPoolId>().is_ok());
        assert!("DE*A36".parse::<OperatorId>().is_ok());
    }

    #[test]
    fn the_din_grammars_reject_each_part_on_its_own() {
        // The DIN branch splits on `*` and checks three parts. Each check needs its own case, or
        // an `||` written as an `&&` is caught by nothing.
        for bad in [
            "49810000438",  // no separators at all
            "49*810",       // only two parts
            "*810*000",     // empty country
            "4999*810*000", // four-digit country
            "4a*810*000",   // non-digit country
            "49*81*000",    // two-digit operator
            "49*8100*000",  // four-digit operator
            "49*8a0*000",   // non-digit operator
            "49*810*",      // empty instance
            "49*810*00a",   // non-digit instance
        ] {
            assert!(bad.parse::<EvseId>().is_err(), "{bad} should be rejected");
            assert!(!EvseId::new_unchecked(bad).is_well_formed(), "{bad}");
        }
        assert!(format!("49*810*{}", "0".repeat(32)).parse::<EvseId>().is_ok(), "thirty-two digits");
        assert!(format!("49*810*{}", "0".repeat(33)).parse::<EvseId>().is_err(), "thirty-three is too many");

        // OperatorID's DIN branch, the same way.
        for bad in ["49810", "49", "*536", "4999*536", "4a*536", "49*53", "49*5366", "49*5a6"] {
            assert!(bad.parse::<OperatorId>().is_err(), "{bad} should be rejected");
        }
        assert!("+999*536".parse::<OperatorId>().is_ok(), "a three-digit country code");
    }

    #[test]
    fn the_components_a_caller_reads_are_the_ones_in_the_identifier() {
        // `country()` feeds routing and `oicp id`; an empty answer is a plausible-looking lie.
        assert_eq!("DE*A36".parse::<OperatorId>().unwrap().country(), "DE");
        assert_eq!("DEA36".parse::<OperatorId>().unwrap().country(), "DE");
        assert_eq!("+49*536".parse::<OperatorId>().unwrap().country(), "49");
        assert_eq!("999*536".parse::<OperatorId>().unwrap().country(), "999");
        assert_eq!("DE-8EO".parse::<ProviderId>().unwrap().country(), "DE");

        let session: SessionId = "b2688855-7f00-0002-6d8e-48d883f6abb6".parse().unwrap();
        assert_eq!(session.as_str(), "b2688855-7f00-0002-6d8e-48d883f6abb6");
        assert!(session.is_well_formed());
        let broken = SessionId::new_unchecked("nope");
        assert_eq!(broken.as_str(), "nope");
        assert!(!broken.is_well_formed());
    }

    #[test]
    fn the_pricing_asterisk_survives_every_road_in_and_out() {
        let all: ProviderIdOrAll = "*".parse().unwrap();
        let one: ProviderIdOrAll = "DE-8EO".parse().unwrap();

        assert_eq!(all.to_string(), "*");
        assert_eq!(one.to_string(), "DE-8EO");
        assert_eq!(all.as_str(), "*");
        assert!(all.is_all() && !one.is_all());
        assert!(all.validate().is_ok(), "the asterisk is not an identifier to validate");
        assert!(one.validate().is_ok());
        assert!(ProviderIdOrAll::One(ProviderId::new_unchecked("nope")).validate().is_err());

        // The `*` is recognised on the way in, not just on the way out.
        assert_eq!(serde_json::from_str::<ProviderIdOrAll>(r#""*""#).unwrap(), ProviderIdOrAll::All);
        assert_eq!(serde_json::from_str::<ProviderIdOrAll>(r#""DE-8EO""#).unwrap(), one);
        assert_eq!(serde_json::to_string(&all).unwrap(), r#""*""#);
    }

    #[test]
    fn a_rejection_says_which_type_rejected_what() {
        // `IdError`'s accessors are what a conformance report is built from, and `oicp id` prints
        // the standard. All three are load-bearing text.
        let err = "not-an-evse-id".parse::<EvseId>().unwrap_err();
        assert_eq!(err.type_name(), "EvseID");
        assert_eq!(err.value(), "not-an-evse-id");
        assert!(err.to_string().contains("EvseID"), "{err}");
        assert!(err.to_string().contains("not-an-evse-id"), "{err}");

        assert_eq!("DE*A36".parse::<OperatorId>().unwrap().standard().as_str(), "ISO");
        assert_eq!("+49*536".parse::<OperatorId>().unwrap().standard().as_str(), "DIN");
        assert_eq!("DE8EO".parse::<ProviderId>().unwrap().standard().as_str(), "ISO/DIN");
        assert_eq!(IdStandard::Iso.to_string(), "ISO");
        assert_eq!(IdStandard::Din.to_string(), "DIN");
        assert_eq!(IdStandard::Either.to_string(), "ISO/DIN");
    }

    #[cfg(feature = "schema")]
    #[test]
    fn the_published_schema_describes_every_identifier_as_a_string() {
        // `oicp schema` is an artefact partners generate clients from. An empty schema validates
        // anything, which is the one answer that helps nobody.
        let mut generator = schemars::SchemaGenerator::default();
        for (name, schema) in [
            ("EvseID", <EvseId as schemars::JsonSchema>::json_schema(&mut generator)),
            ("EvcoID", <EvcoId as schemars::JsonSchema>::json_schema(&mut generator)),
            ("OperatorID", <OperatorId as schemars::JsonSchema>::json_schema(&mut generator)),
            ("ProviderID", <ProviderId as schemars::JsonSchema>::json_schema(&mut generator)),
            ("ChargingPoolID", <ChargingPoolId as schemars::JsonSchema>::json_schema(&mut generator)),
            ("SessionID", <SessionId as schemars::JsonSchema>::json_schema(&mut generator)),
            ("UID", <Uid as schemars::JsonSchema>::json_schema(&mut generator)),
            ("ProviderIDAsterisk", <ProviderIdOrAll as schemars::JsonSchema>::json_schema(&mut generator)),
        ] {
            let json = serde_json::to_value(&schema).unwrap();
            assert_eq!(json["type"], "string", "{name} published an empty schema: {json}");
        }
        assert_eq!(<EvseId as schemars::JsonSchema>::schema_name(), "EvseID");
        assert_eq!(<Uid as schemars::JsonSchema>::schema_name(), "UID");
        assert_eq!(<ProviderIdOrAll as schemars::JsonSchema>::schema_name(), "ProviderIDAsterisk");
    }

    #[test]
    fn session_ids_compare_hash_and_order_as_one_session() {
        // Every routing decision the broker makes turns on this: the session an authorization
        // opened is the session a CDR settles. An `eq` that says yes to everything routes every
        // CDR to the first session it finds.
        use std::hash::{BuildHasher as _, RandomState};
        let hasher = RandomState::new();

        let lower: SessionId = "b2688855-7f00-0002-6d8e-48d883f6abb6".parse().unwrap();
        let upper: SessionId = "B2688855-7F00-0002-6D8E-48D883F6ABB6".parse().unwrap();
        let other: SessionId = "c2688855-7f00-0002-6d8e-48d883f6abb6".parse().unwrap();

        assert_eq!(lower, upper, "one session, written two ways");
        assert_eq!(hasher.hash_one(&lower), hasher.hash_one(&upper));
        assert_eq!(lower.cmp(&upper), core::cmp::Ordering::Equal);
        assert_eq!(lower.to_string(), "b2688855-7f00-0002-6d8e-48d883f6abb6", "and each keeps its text");

        assert_ne!(lower, other, "two sessions are two sessions");
        assert_ne!(hasher.hash_one(&lower), hasher.hash_one(&other));
        assert!(lower < other);
        assert!(other > upper);
        assert_eq!(lower.partial_cmp(&other), Some(core::cmp::Ordering::Less));
    }

    #[test]
    fn rfid_uids_compare_hash_and_order_as_one_card() {
        // An EMP's blocklist is a set of these. A `hash` that hashes nothing turns it into a linear
        // scan; an `eq` that says yes to everything blocks every card.
        use std::hash::{BuildHasher as _, RandomState};
        let hasher = RandomState::new();

        let upper: Uid = "7568290FFF765F".parse().unwrap();
        let lower = Uid::new_unchecked("7568290fff765f");
        let other: Uid = "AABBCCDDEEFF11".parse().unwrap();

        assert_eq!(upper, lower);
        assert_eq!(hasher.hash_one(&upper), hasher.hash_one(&lower));
        assert_eq!(upper.cmp(&lower), core::cmp::Ordering::Equal);

        assert_ne!(upper, other);
        assert_ne!(hasher.hash_one(&upper), hasher.hash_one(&other));
        assert!(other > upper, "A sorts after 7");
        assert_eq!(upper.partial_cmp(&other), Some(core::cmp::Ordering::Less));
    }

    /// Every length bound in the grammars, at the exact value and one step past it.
    ///
    /// A `<` written as `<=`, or an `||` as an `&&`, changes only what happens *at* the boundary —
    /// so a test that probes the middle of a range agrees with the mutant and the specification
    /// alike, and says nothing.
    #[test]
    fn every_grammar_boundary_is_checked_at_the_boundary() {
        // EvseID ISO: three alphanumerics after the country, then `E`, then 1..=30 more.
        assert!("DE*AB7*E1".parse::<EvseId>().is_ok(), "one instance character is enough");
        assert!(format!("DE*AB7*E{}", "1".repeat(30)).parse::<EvseId>().is_ok(), "thirty is the cap");
        assert!(format!("DE*AB7*E{}", "1".repeat(31)).parse::<EvseId>().is_err(), "thirty-one is not");
        assert!("DE*AB7*E".parse::<EvseId>().is_err(), "an empty instance part is not an EvseID");
        assert!("DE*AB*E1".parse::<EvseId>().is_err(), "two operator characters is not three");
        assert!("DE*AB7".parse::<EvseId>().is_err(), "and the E marker is not optional");

        // EvcoID ISO: `C` then exactly eight, then exactly one check character.
        assert!("DE-8EO-CAet5e4XY-3".parse::<EvcoId>().is_ok());
        assert!("DE-8EO-CAet5e4X-3".parse::<EvcoId>().is_err(), "seven is not eight");
        assert!("DE-8EO-CAet5e4XYZ-3".parse::<EvcoId>().is_err(), "nor is nine");
        assert!("DE-8EO-CAet5e4XY-".parse::<EvcoId>().is_err(), "the check character is not optional");
        assert!("DE-8EO-CAet5e4XY-34".parse::<EvcoId>().is_err(), "and there is exactly one of it");
        assert!("DE-8E-CAet5e4XY-3".parse::<EvcoId>().is_err(), "two provider characters is not three");

        // EvcoID DIN: exactly six in the instance, then exactly one digit or X.
        assert!("DE*8EO*Aet5e4*3".parse::<EvcoId>().is_ok());
        assert!("DE*8EO*Aet5e*3".parse::<EvcoId>().is_err(), "five is not six");
        assert!("DE*8EO*Aet5e44*3".parse::<EvcoId>().is_err(), "nor is seven");
        assert!("DE*8EO*Aet5e4*".parse::<EvcoId>().is_err(), "the check digit is not optional");
        assert!("DE*8EO*Aet5e4*33".parse::<EvcoId>().is_err(), "and there is exactly one of it");
        assert!("DE*8EO*Aet5e4*X".parse::<EvcoId>().is_ok(), "X is a check digit");
        assert!("DE*8EO*Aet5e4*Y".parse::<EvcoId>().is_err(), "Y is not");

        // ChargingPoolID: the ISO EvseID shape with `P`, and the same 1..=30 pool part.
        assert!("IT*123*P4".parse::<ChargingPoolId>().is_ok());
        assert!(format!("IT*123*P{}", "4".repeat(30)).parse::<ChargingPoolId>().is_ok());
        assert!(format!("IT*123*P{}", "4".repeat(31)).parse::<ChargingPoolId>().is_err());
        assert!("IT*123*P".parse::<ChargingPoolId>().is_err(), "an empty pool part is not a pool");
        assert!("IT*12*P456".parse::<ChargingPoolId>().is_err(), "two operator characters is not three");
        assert!("IT*123*456".parse::<ChargingPoolId>().is_err(), "and the P marker is not optional");

        // SessionID: 8-4-4-4-12, each group exact.
        assert!("b2688855-7f00-0002-6d8e-48d883f6abb6".parse::<SessionId>().is_ok());
        for bad in [
            "b268885-7f00-0002-6d8e-48d883f6abb6",   // 7 in the first group
            "b26888556-7f00-0002-6d8e-48d883f6abb6", // 9
            "b2688855-7f0-0002-6d8e-48d883f6abb6",   // 3 in the second
            "b2688855-7f00-0002-6d8e-48d883f6abb",   // 11 in the last
            "b2688855-7f00-0002-6d8e-48d883f6abb6a", // 13
            "b2688855-7f00-0002-6d8e-48d883f6ab_6",  // not alphanumeric
        ] {
            assert!(bad.parse::<SessionId>().is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn session_ids_follow_the_guid_shape() {
        let id: SessionId = "b2688855-7f00-0002-6d8e-48d883f6abb6".parse().unwrap();
        assert_eq!(id.to_string(), "b2688855-7f00-0002-6d8e-48d883f6abb6");
        assert!("b2688855-7f00-0002-6d8e".parse::<SessionId>().is_err());
        // OICP allows letters everywhere, so this is not a hex-only UUID.
        assert!("zzzzzzzz-aaaa-bbbb-cccc-dddddddddddd".parse::<SessionId>().is_ok());
    }

    #[test]
    fn rfid_uids_match_case_insensitively_but_only_uppercase_is_emitted() {
        let upper: Uid = "7568290FFF765F".parse().unwrap();
        // A reader that reports lower case is common, so the value must decode…
        let lower = Uid::new_unchecked("7568290fff765f");

        assert_eq!(upper, lower, "the same card");
        assert_eq!(lower.to_string(), "7568290fff765f", "and its text survives");
        assert_eq!(lower.canonical(), "7568290FFF765F", "with an uppercase key for lookups");

        // …but the specification writes [0-9A-F], so this crate will not construct or emit one.
        assert!("7568290fff765f".parse::<Uid>().is_err());
        assert!(lower.validate().is_err());
        assert!(!lower.is_well_formed());
        assert!(upper.validate().is_ok());
        assert!("123".parse::<Uid>().is_err());
    }

    #[test]
    fn is_well_formed_agrees_with_validate_for_every_identifier() {
        // The contract a caller relies on — and one a fuzz target checks for arbitrary input,
        // which is how the lowercase-UID disagreement above was found.
        for text in ["7568290FFF765F", "7568290fff765f", "123", "", "DE*ABC*E1", "not an id"] {
            let uid = Uid::new_unchecked(text);
            assert_eq!(uid.is_well_formed(), uid.validate().is_ok(), "Uid disagreed on {text:?}");
            let evse = EvseId::new_unchecked(text);
            assert_eq!(evse.is_well_formed(), evse.validate().is_ok(), "EvseId disagreed on {text:?}");
            let session = SessionId::new_unchecked(text);
            assert_eq!(
                session.is_well_formed(),
                session.validate().is_ok(),
                "SessionId disagreed on {text:?}"
            );
        }
    }

    #[test]
    fn charging_pool_ids_use_the_p_marker() {
        let id: ChargingPoolId = "IT*123*P456*AB789".parse().unwrap();
        assert_eq!(id.to_string(), "IT*123*P456*AB789");
        assert!("IT*123*E456*AB789".parse::<ChargingPoolId>().is_err());
    }

    #[test]
    fn the_pricing_asterisk_is_its_own_variant() {
        let all: ProviderIdOrAll = "*".parse().unwrap();
        assert!(all.is_all());
        assert_eq!(serde_json::to_string(&all).unwrap(), r#""*""#);
        let one: ProviderIdOrAll = "DE-8EO".parse().unwrap();
        assert!(!one.is_all());
    }
}
