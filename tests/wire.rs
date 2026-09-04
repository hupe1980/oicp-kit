//! The wire model against the specification's own examples: decode, validate, re-encode.
//!
//! The examples are taken from Hubject's OpenAPI documents. Where an example is itself
//! non-conformant — several fill in all five `Identification` members at once, which no real
//! payload does — the test says so rather than pretending otherwise.

use oicp_kit::cpo::{
    ChargeDetailRecord, ChargingNotification, EvseDataRecord, EvseStatus, EvseStatusRecord,
    PushEvseDataRequest,
};
use oicp_kit::emp::{EvseDataResponse, PullEvseDataRequest};
use oicp_kit::testkit::samples;
use oicp_kit::types::{Extensions, Validate};

/// Decodes `json` as `T`, re-encodes it, and asserts the JSON is unchanged.
fn assert_round_trips<T>(json: &str)
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let value: T = serde_json::from_str(json).unwrap_or_else(|e| panic!("failed to decode: {e}\n{json}"));
    let out = serde_json::to_value(&value).expect("re-encodes");
    let original: serde_json::Value = serde_json::from_str(json).expect("the fixture is JSON");
    assert_eq!(out, original, "the payload changed on the way through");
}

#[test]
fn the_specs_acknowledgement_example_round_trips() {
    assert_round_trips::<oicp_kit::types::Acknowledgement>(
        r#"{
            "Result": true,
            "StatusCode": {"Code": "000", "Description": "Success", "AdditionalInfo": "Success"},
            "SessionID": "f98efba4-02d8-4fa0-b810-9a9d50d2c527",
            "CPOPartnerSessionID": "1234XYZ",
            "EMPPartnerSessionID": "2345ABC"
        }"#,
    );
}

#[test]
fn the_specs_evse_status_example_round_trips() {
    assert_round_trips::<EvseStatusRecord>(r#"{"EvseID": "DE*XYZ*ETEST1", "EvseStatus": "Available"}"#);
}

#[test]
fn the_specs_charge_detail_record_example_round_trips() {
    // Hubject's own example, with the Identification reduced to one member: the published example
    // fills in all five at once, which is not a payload any implementation produces.
    assert_round_trips::<ChargeDetailRecord>(
        r#"{
            "SessionID": "f98efba4-02d8-4fa0-b810-9a9d50d2c527",
            "CPOPartnerSessionID": "1234XYZ",
            "EMPPartnerSessionID": "9876655",
            "PartnerProductID": "AC 1",
            "EvseID": "DE*XYZ*ETEST1",
            "Identification": {"RFIDMifareFamilyIdentification": {"UID": "1234ABCD"}},
            "ChargingStart": "2020-09-23T14:17:53.038Z",
            "ChargingEnd": "2020-09-23T14:17:53.038Z",
            "SessionStart": "2020-09-23T14:17:53.039Z",
            "SessionEnd": "2020-09-23T14:17:53.039Z",
            "MeterValueStart": 0,
            "MeterValueEnd": 10,
            "MeterValueInBetween": {"meterValues": [10]},
            "ConsumedEnergy": 10,
            "SignedMeteringValues": [
                {"SignedMeteringValue": "AAAA", "MeteringStatus": "Start"},
                {"SignedMeteringValue": "BBBB", "MeteringStatus": "End"}
            ],
            "CalibrationLawVerificationInfo": {
                "CalibrationLawCertificateID": "CD-12BD-2783T",
                "PublicKey": "a9sdh839alskldh",
                "MeteringSignatureUrl": "http://www.meteringexample1234.com",
                "MeteringSignatureEncodingFormat": "UTF-8",
                "SignedMeteringValuesVerificationInstruction": "please follow instructions"
            },
            "HubOperatorID": "DE*ABC",
            "HubProviderID": "DE-DCB"
        }"#,
    );
}

#[test]
fn the_specs_push_evse_data_example_decodes_and_validates() {
    // From `eRoamingPushEvseData.yaml`, with the example's `ChargingStationID` spelling — which is
    // erratum OICP23-E002 — and its `deltaType`/`lastUpdate`, which are Hubject's to write.
    let json = r#"{
        "ActionType": "fullLoad",
        "OperatorEvseData": {
            "OperatorID": "DE*ABC",
            "OperatorName": "ABC technologies",
            "EvseDataRecord": [{
                "EvseID": "DE*ABC*ETEST1",
                "ChargingStationID": "TEST 1",
                "ChargingStationNames": [{"lang": "en", "value": "ABC Charging Station Test"}],
                "Address": {
                    "Country": "DEU", "City": "Berlin", "Street": "EUREF CAMPUS",
                    "PostalCode": "10829", "HouseNum": "22", "Floor": "6OG",
                    "Region": "Berlin", "TimeZone": "UTC+01:00",
                    "ParkingFacility": true, "ParkingSpot": "E36"
                },
                "GeoCoordinates": {"Google": {"Coordinates": "52.480495 13.356465"}},
                "Plugs": ["Type 2 Outlet"],
                "ChargingFacilities": [{"PowerType": "AC_3_PHASE", "Power": 22, "Voltage": 480, "Amperage": 32,
                                        "ChargingModes": ["Mode_4"]}],
                "RenewableEnergy": true,
                "CalibrationLawDataAvailability": "Local",
                "AuthenticationModes": ["NFC RFID Classic", "REMOTE"],
                "PaymentOptions": ["No Payment"],
                "ValueAddedServices": ["Reservation"],
                "Accessibility": "Restricted access",
                "AccessibilityLocation": "ParkingGarage",
                "HotlinePhoneNumber": "+49123123123123",
                "IsOpen24Hours": true,
                "IsHubjectCompatible": true,
                "DynamicInfoAvailable": "true",
                "MaxCapacity": 50,
                "EnergySource": [{"Energy": "Solar", "Percentage": 85}, {"Energy": "Wind", "Percentage": 15}],
                "EnvironmentalImpact": {"CO2Emission": 30.3},
                "deltaType": "insert",
                "lastUpdate": "2018-01-23T14:04:29.377Z"
            }]
        }
    }"#;

    let request: PushEvseDataRequest = serde_json::from_str(json).expect("the spec's example decodes");
    request.validate().expect("the spec's example is conformant");

    let record = &request.operator_evse_data.evse_data_record[0];
    assert_eq!(record.charging_station_id.as_ref().unwrap().as_str(), "TEST 1");
    assert_eq!(record.charging_facilities[0].power.to_string(), "22");
    // The example's 30.3 g/kWh is a decimal, and stays one.
    assert_eq!(record.environmental_impact.as_ref().unwrap().co2_emission.unwrap().to_string(), "30.3");
}

#[test]
fn the_specs_pull_evse_data_example_decodes() {
    // The published example sets LastCall *and* the filters, which the spec forbids. It decodes —
    // parse permissively — and `validate` reports exactly that.
    let json = r#"{
        "ProviderID": "DE-DCB",
        "GeoCoordinatesResponseFormat": "Google",
        "CountryCodes": ["DEU"],
        "OperatorIds": ["DE*ABC"],
        "LastCall": "2020-09-23T14:27:43.052Z",
        "IsHubjectCompatible": true,
        "IsOpen24Hours": true,
        "RenewableEnergy": true,
        "AuthenticationModes": ["PnC"],
        "Accessibility": ["Free publicly accessible"],
        "CalibrationLawDataAvailability": ["Local"]
    }"#;
    let request: PullEvseDataRequest = serde_json::from_str(json).expect("it decodes");
    assert!(request.is_delta());
    let violations = request.validate().expect_err("the spec's own example breaks the exclusivity rule");
    assert!(violations.iter().any(|v| v.pointer == "/LastCall"));
    assert_eq!(request.conflicting_filters(), vec!["CountryCodes", "OperatorIds"]);
}

#[test]
fn a_page_of_evse_data_decodes_with_its_metadata() {
    let json = serde_json::json!({
        "content": [samples::pull_evse_data_record("DE*ABC*E1")],
        "number": 0, "size": 20, "totalElements": 1, "totalPages": 1,
        "first": true, "last": true, "numberOfElements": 1, "empty": false,
        "pageable": {"sort": {"sorted": false, "empty": true, "unsorted": true},
                     "pageSize": 20, "pageNumber": 0, "offset": 0, "paged": true, "unpaged": false},
        "StatusCode": {"Code": "000", "Description": "Success"}
    });
    let page: EvseDataResponse = serde_json::from_value(json).expect("it decodes");
    page.validate().expect("the page is conformant");
    assert_eq!(page.next_page(), None);
    assert!(!page.is_error());
    assert_eq!(page.pageable.as_ref().unwrap().page_size, Some(20));
}

#[test]
fn every_sample_in_the_testkit_is_conformant() {
    // The samples are what MockHubject and everyone's tests are built on, so they must not rot.
    samples::address().validate().expect("address");
    samples::charging_facility().validate().expect("charging facility");
    samples::evse_data_record("DE*ABC*E1").validate().expect("evse data record");
    samples::pull_evse_data_record("DE*ABC*E1").validate().expect("pull evse data record");
    samples::evse_status_record("DE*ABC*E1", EvseStatus::Available).validate().expect("status record");
    samples::authorize_start_request("DE*ABC*E1").validate().expect("authorize start");
    samples::charge_detail_record("DE*ABC*E1", samples::session_id()).validate().expect("cdr");
    samples::charging_notification_start("DE*ABC*E1", samples::session_id())
        .validate()
        .expect("notification");
    for record in samples::fleet("DE*ABC", 10) {
        record.validate().expect("fleet record");
    }
}

#[test]
fn every_sample_round_trips_byte_identically() {
    macro_rules! round_trip {
        ($value:expr) => {{
            let value = $value;
            let json = serde_json::to_string(&value).expect("encodes");
            let back = serde_json::from_str(&json).expect("decodes");
            assert_eq!(value, back, "the value changed on the way through");
            assert_eq!(serde_json::to_string(&back).unwrap(), json, "the JSON changed");
        }};
    }
    round_trip!(samples::evse_data_record("DE*ABC*E1"));
    round_trip!(samples::pull_evse_data_record("DE*ABC*E1"));
    round_trip!(samples::charge_detail_record("DE*ABC*E1", samples::session_id()));
    round_trip!(samples::authorize_start_request("DE*ABC*E1"));
    round_trip!(ChargingNotification::Start(samples::charging_notification_start(
        "DE*ABC*E1",
        samples::session_id()
    )));
}

#[test]
fn a_field_hubject_adds_later_survives_every_object() {
    // OICP 2.3 is edited in place. Anything this crate has not heard of must come back out.
    let mut json = serde_json::to_value(samples::evse_data_record("DE*ABC*E1")).unwrap();
    json.as_object_mut()
        .unwrap()
        .insert("HubjectAddedThisIn2027".into(), serde_json::json!({"nested": [1, 2]}));

    let record: EvseDataRecord = serde_json::from_value(json.clone()).expect("it decodes");
    assert_eq!(record.extensions.len(), 1);
    assert_eq!(serde_json::to_value(&record).unwrap(), json, "the unknown field was lost");
}

#[test]
fn an_enum_value_hubject_adds_later_survives_and_is_reported() {
    let json = serde_json::json!({"EvseID": "DE*ABC*E1", "EvseStatus": "Maintenance"});
    let record: EvseStatusRecord = serde_json::from_value(json.clone()).expect("it decodes");

    assert!(!record.evse_status.is_known());
    assert_eq!(record.evse_status.as_str(), "Maintenance");
    assert_eq!(serde_json::to_value(&record).unwrap(), json);
    // Preserved, and still visible in a conformance report.
    assert!(record.validate().is_err());
}

#[test]
fn one_malformed_record_does_not_cost_the_page() {
    // The property that makes an EMP's crawl survivable: a page carries records from dozens of
    // operators, and one operator's mistake must not lose the other 1999.
    let mut good = serde_json::to_value(samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
    let mut bad = serde_json::to_value(samples::pull_evse_data_record("DE*XYZ*E1")).unwrap();
    bad.as_object_mut().unwrap().insert("HotlinePhoneNumber".into(), serde_json::json!("call us"));
    bad.as_object_mut().unwrap().insert("EvseID".into(), serde_json::json!("this is not an EvseID"));
    good.as_object_mut().unwrap().insert("_note".into(), serde_json::json!("fine"));

    let page = serde_json::json!({
        "content": [good, bad],
        "number": 0, "size": 20, "totalElements": 2, "totalPages": 1,
        "first": true, "last": true, "numberOfElements": 2
    });

    let page: EvseDataResponse = serde_json::from_value(page).expect("the page still decodes");
    assert_eq!(page.content.len(), 2, "both records arrived");
    assert!(page.content[0].validate().is_ok(), "the good record is clean");

    let violations = page.content[1].validate().expect_err("the bad record is reported");
    assert!(violations.iter().any(|v| v.pointer == "/EvseID"));
    assert!(violations.iter().any(|v| v.pointer == "/HotlinePhoneNumber"));
}

#[test]
fn identifiers_are_never_rewritten_on_the_way_out() {
    // Hubject matches these against the TLS certificate as text.
    for spelling in ["DE*ABC*E1", "DEABCE1"] {
        let record = samples::evse_data_record(spelling);
        let json = serde_json::to_value(&record).unwrap();
        assert_eq!(json.get("EvseID").and_then(|v| v.as_str()), Some(spelling));
    }
}

#[test]
fn an_empty_extensions_map_is_not_serialised() {
    let record = EvseStatusRecord {
        evse_id: "DE*ABC*E1".parse().unwrap(),
        evse_status: EvseStatus::Available,
        extensions: Extensions::new(),
    };
    let json = serde_json::to_string(&record).unwrap();
    assert_eq!(json, r#"{"EvseID":"DE*ABC*E1","EvseStatus":"Available"}"#);
}
