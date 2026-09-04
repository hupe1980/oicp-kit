//! Identifiers must accept anything on the wire, never panic, and never rewrite what they got.
//!
//! The wire-exactness property is the one Hubject's certificate check depends on: an identifier
//! this crate alters is `017 Unauthorized Access` on every request.

#![no_main]

use libfuzzer_sys::fuzz_target;
use oicp_kit::types::{
    ChargingPoolId, EvcoId, EvseId, OperatorId, ProviderId, SessionId, Uid, Validate,
};

macro_rules! exercise {
    ($ty:ty, $text:expr) => {{
        let json = serde_json::to_string($text).expect("a string encodes");
        // Decoding is permissive by design: a malformed id on one record of a page must arrive.
        let id: $ty = serde_json::from_str(&json).expect("decoding an identifier never fails");

        // Whatever arrived goes back out byte-identically.
        assert_eq!(serde_json::to_string(&id).expect("re-encodes"), json, "the identifier was rewritten");

        // Well-formed or reported, never silently wrong.
        assert_eq!(id.is_well_formed(), id.validate().is_ok());

        // The strict constructor agrees with the check.
        assert_eq!(<$ty>::new($text.to_owned()).is_ok(), id.is_well_formed());
    }};
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if text.len() > 512 {
        return;
    }

    exercise!(EvseId, text);
    exercise!(EvcoId, text);
    exercise!(OperatorId, text);
    exercise!(ProviderId, text);
    exercise!(ChargingPoolId, text);
    exercise!(SessionId, text);
    exercise!(Uid, text);

    // The accessors must not panic on a malformed value either — they are called on records that
    // arrived from a peer, before anyone has validated them.
    if let Ok(evse) = text.parse::<EvseId>() {
        let _ = evse.operator_id();
        let _ = evse.country();
        let _ = evse.canonical();
    }
    if let Ok(evco) = text.parse::<EvcoId>() {
        let _ = evco.provider_id();
    }
});
