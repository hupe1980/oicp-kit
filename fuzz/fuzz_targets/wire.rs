//! Decoding arbitrary JSON as an OICP message must never panic, and what decodes must round-trip.
//!
//! The crate's contract is that a peer's payload cannot crash the decoder — a page of two thousand
//! records from dozens of operators is exactly where a panic would take down a roaming platform.
//! `Validate` reports what is wrong; it never unwinds.

#![no_main]

use libfuzzer_sys::fuzz_target;
use oicp_kit::cpo::{ChargeDetailRecord, ChargingNotification, EvseDataRecord, PushEvseDataRequest};
use oicp_kit::emp::{EvseDataResponse, PullEvseDataRecord, PullEvseDataRequest};
use oicp_kit::types::{Acknowledgement, Validate};

/// Decodes as `T`; if that works, validating and re-encoding must also work, and the JSON must be
/// stable across a second round trip.
fn exercise<T>(text: &str)
where
    T: serde::de::DeserializeOwned + serde::Serialize + Validate,
{
    let Ok(value) = serde_json::from_str::<T>(text) else { return };

    // Validation walks the whole object graph; it must report rather than panic.
    let _ = value.validate();

    let encoded = serde_json::to_string(&value).expect("a decoded value always re-encodes");
    let again: T = serde_json::from_str(&encoded).expect("what this crate emits, it can read");
    let twice = serde_json::to_string(&again).expect("re-encodes");
    assert_eq!(encoded, twice, "encoding is not idempotent");
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };

    exercise::<EvseDataRecord>(text);
    exercise::<PullEvseDataRecord>(text);
    exercise::<PushEvseDataRequest>(text);
    exercise::<PullEvseDataRequest>(text);
    exercise::<ChargeDetailRecord>(text);
    exercise::<ChargingNotification>(text);
    exercise::<Acknowledgement>(text);
    exercise::<EvseDataResponse>(text);
});
