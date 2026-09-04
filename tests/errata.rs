//! Every erratum in [`ERRATA`] is real, and this crate does what it says about it.
//!
//! Each disagreement between Hubject's own OICP 2.3 documents is recorded in
//! [`oicp_kit::types::ERRATA`]. These tests check the *behaviour* the registry promises: that both
//! spellings decode, and that the one this crate emits is the leading document's.
//!
//! `cargo run -p xtask -- errata` checks the other half — that the disagreements still exist in
//! the vendored specs — so an erratum Hubject fixes shows up as a failing job rather than as a
//! stale claim in the documentation.

use oicp_kit::cpo::ChargeDetailRecord;
use oicp_kit::emp::GetChargeDetailRecordsRequest;
use oicp_kit::testkit::samples;
use oicp_kit::types::{ERRATA, Erratum};

/// Decodes `json`, re-encodes it, and returns the object that came back out.
fn round_trip<T>(json: serde_json::Value) -> serde_json::Value
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let decoded: T = serde_json::from_value(json).expect("the payload decodes");
    serde_json::to_value(&decoded).expect("the payload re-encodes")
}

#[test]
fn oicp23_e001_hub_provider_id_reads_both_spellings_and_writes_the_leading_one() {
    let erratum = Erratum::get("OICP23-E001").expect("the erratum is registered");
    assert!(erratum.field.contains("HubProviderID"));

    for spelling in ["HubProviderID", "HubProviderId"] {
        let mut json =
            serde_json::to_value(samples::charge_detail_record("DE*ABC*E1", samples::session_id())).unwrap();
        json.as_object_mut().unwrap().insert(spelling.to_owned(), serde_json::json!("DE-DCB"));

        let out = round_trip::<ChargeDetailRecord>(json);
        assert_eq!(
            out.get("HubProviderID").and_then(|v| v.as_str()),
            Some("DE-DCB"),
            "{spelling} did not survive into the canonical field"
        );
        assert!(out.get("HubProviderId").is_none(), "the OpenAPI typo must not be emitted");
    }
}

#[test]
fn oicp23_e002_charging_station_id_reads_both_spellings_and_writes_the_leading_one() {
    assert!(Erratum::get("OICP23-E002").is_some());

    for spelling in ["ChargingStationId", "ChargingStationID"] {
        let mut json = serde_json::to_value(samples::evse_data_record("DE*ABC*E1")).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("ChargingStationId");
        object.insert(spelling.to_owned(), serde_json::json!("TEST 1"));

        let out = round_trip::<oicp_kit::cpo::EvseDataRecord>(json);
        assert_eq!(
            out.get("ChargingStationId").and_then(|v| v.as_str()),
            Some("TEST 1"),
            "{spelling} did not survive into the canonical field"
        );
        assert!(out.get("ChargingStationID").is_none(), "the example's spelling must not be emitted");
    }

    // The EMP's view of the same record behaves identically.
    for spelling in ["ChargingStationId", "ChargingStationID"] {
        let mut json = serde_json::to_value(samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("ChargingStationId");
        object.insert(spelling.to_owned(), serde_json::json!("TEST 1"));
        let out = round_trip::<oicp_kit::emp::PullEvseDataRecord>(json);
        assert_eq!(out.get("ChargingStationId").and_then(|v| v.as_str()), Some("TEST 1"));
    }
}

#[test]
fn oicp23_e003_a_fractional_power_arrives_and_is_reported() {
    use oicp_kit::types::{ChargingFacility, Validate, ViolationCode};

    assert!(Erratum::get("OICP23-E003").is_some());

    // The EMP schema types Power as a number; a real 22.5 kW facility must not be lost.
    let facility: ChargingFacility =
        serde_json::from_str(r#"{"PowerType":"AC_3_PHASE","Power":22.5}"#).expect("it decodes");
    assert_eq!(facility.power.to_string(), "22.5");
    assert_eq!(serde_json::to_string(&facility).unwrap(), r#"{"PowerType":"AC_3_PHASE","Power":22.5}"#);

    // …and the leading document's Integer typing is reported, with the erratum id.
    let violations = facility.validate().expect_err("a fractional power deviates from the leading document");
    let found = violations.iter().find(|v| v.code == ViolationCode::Inconsistent).expect("reported");
    assert!(found.message.contains("OICP23-E003"));

    // An integral power is clean.
    let facility: ChargingFacility = serde_json::from_str(r#"{"PowerType":"DC","Power":150}"#).unwrap();
    assert!(facility.validate().is_ok());
}

#[test]
fn oicp23_e004_cdr_forwarded_reads_both_spellings_and_writes_the_leading_one() {
    assert!(Erratum::get("OICP23-E004").is_some());

    for spelling in ["CDRForwarded", "CDRForwarder"] {
        let json = serde_json::json!({
            "ProviderID": "DE-DCB",
            "From": "2020-08-23T14:20:10.285Z",
            "To": "2020-09-23T14:20:10.285Z",
            spelling: true,
        });
        let out = round_trip::<GetChargeDetailRecordsRequest>(json);
        assert_eq!(
            out.get("CDRForwarded").and_then(serde_json::Value::as_bool),
            Some(true),
            "{spelling} did not survive into the canonical field"
        );
        assert!(out.get("CDRForwarder").is_none());
    }
}

#[test]
fn oicp23_e005_reservation_session_id_reads_both_spellings_and_writes_the_leading_one() {
    use oicp_kit::cpo::{AuthorizeRemoteReservationStartRequest, AuthorizeRemoteReservationStopRequest};

    assert!(Erratum::get("OICP23-E005").is_some());

    for spelling in ["EMPPartnerSessionID", "EMPPartnerSessionId"] {
        let start = serde_json::json!({
            "ProviderID": "DE-DCB",
            "EvseID": "DE*ABC*E1",
            "Identification": {"RemoteIdentification": {"EvcoID": "DE-DCB-C12345678-X"}},
            spelling: "2345ABC",
        });
        let out = round_trip::<AuthorizeRemoteReservationStartRequest>(start);
        assert_eq!(out.get("EMPPartnerSessionID").and_then(|v| v.as_str()), Some("2345ABC"), "{spelling}");
        assert!(out.get("EMPPartnerSessionId").is_none());

        let stop = serde_json::json!({
            "SessionID": "f98efba4-02d8-4fa0-b810-9a9d50d2c527",
            "ProviderID": "DE-DCB",
            "EvseID": "DE*ABC*E1",
            spelling: "2345ABC",
        });
        let out = round_trip::<AuthorizeRemoteReservationStopRequest>(stop);
        assert_eq!(out.get("EMPPartnerSessionID").and_then(|v| v.as_str()), Some("2345ABC"), "{spelling}");
    }
}

#[test]
fn oicp23_e006_the_progress_duration_uses_the_implementable_definition() {
    use oicp_kit::cpo::ChargingNotificationProgress;

    assert!(Erratum::get("OICP23-E006").is_some());

    // The CPO document: ChargingDuration = EventOccurred - ChargingStart, in milliseconds.
    // 14:25:53 - 14:17:53 is 480 s, and the spec's own example says 480000.
    let json = serde_json::json!({
        "Type": "Progress",
        "SessionID": "f98efba4-02d8-4fa0-b810-9a9d50d2c527",
        "EvseID": "DE*ABC*E1",
        "OperatorID": "DE*ABC",
        "ChargingStart": "2020-09-23T14:17:53.038Z",
        "EventOccurred": "2020-09-23T14:25:53.038Z",
        "ChargingDuration": 480_000,
        "ConsumedEnergyProgress": 9,
    });
    let notification: ChargingNotificationProgress = serde_json::from_value(json).expect("it decodes");
    assert_eq!(notification.implied_duration_ms(), Some(480_000));
    assert!(oicp_kit::types::Validate::validate(&notification).is_ok());
}

#[test]
fn the_registry_is_well_formed_and_every_entry_has_a_test() {
    let mut ids: Vec<&str> = ERRATA.iter().map(|e| e.id).collect();
    ids.sort_unstable();
    let unique_count = {
        let mut u = ids.clone();
        u.dedup();
        u.len()
    };
    assert_eq!(ids.len(), unique_count, "duplicate erratum ids");

    for erratum in ERRATA {
        assert!(!erratum.field.is_empty(), "{} has no field", erratum.id);
        assert!(!erratum.impact.is_empty(), "{} does not say what breaks", erratum.id);
        assert!(!erratum.resolution.is_empty(), "{} does not say what the crate does", erratum.id);
        assert_ne!(
            erratum.leading_document, erratum.openapi_document,
            "{} records a disagreement, but the two documents say the same thing",
            erratum.id
        );
    }

    // The tests above cover E001..E006; a new erratum needs one too.
    assert_eq!(ERRATA.len(), 6, "an erratum was added or removed — add or remove its test here");
}
