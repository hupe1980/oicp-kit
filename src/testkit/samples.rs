//! Valid sample objects, for tests and for seeding a [`MockHubject`](super::MockHubject).
//!
//! Every sample here passes [`Validate::validate`](crate::types::Validate::validate) — checked by
//! `tests/wire.rs`, so a sample cannot rot into a non-conformant one without CI noticing. They are
//! drawn from the examples in Hubject's own OpenAPI documents, corrected where those examples are
//! themselves non-conformant (they fill in all five `Identification` members at once, which no
//! real payload does).

use crate::cpo::{
    AuthorizeStartRequest, AuthorizeStopRequest, ChargeDetailRecord, ChargingNotificationStart,
    ChargingNotificationType, EvseDataRecord, EvseStatus, EvseStatusRecord,
};
use crate::emp::PullEvseDataRecord;
use crate::types::{
    Accessibility, Address, AuthenticationMode, CalibrationLawDataAvailability as Cl, ChargingFacility,
    DateTime, DynamicInfoAvailable, EvseId, Extensions, GeoCoordinates, Identification, InfoText, Number,
    OperatorId, PaymentOption, Plug, PowerType, RfidMifareFamilyIdentification, SessionId, Text, Uid,
    ValueAddedService,
};

/// A conformant address in Berlin — Hubject's own office, from the spec's example.
#[must_use]
pub fn address() -> Address {
    Address::builder()
        .country("DEU")
        .city("Berlin")
        .street("EUREF CAMPUS")
        .postal_code("10829")
        .house_num("22")
        .region("Berlin")
        .time_zone("UTC+01:00")
        .build()
        .expect("the sample address is conformant")
}

/// A 22 kW three-phase AC charging facility.
#[must_use]
pub fn charging_facility() -> ChargingFacility {
    ChargingFacility::builder()
        .power_type(PowerType::Ac3Phase)
        .power(Number::from(22))
        .voltage(Number::from(480))
        .amperage(Number::from(32))
        .build()
        .expect("the sample facility is conformant")
}

/// A conformant `EvseDataRecord` for `evse_id`, as a CPO would push it.
///
/// # Panics
///
/// Panics if `evse_id` is not a valid `EvseID`.
#[must_use]
pub fn evse_data_record(evse_id: &str) -> EvseDataRecord {
    let evse_id: EvseId = evse_id.parse().expect("the sample EvseID is valid");
    EvseDataRecord {
        delta_type: None,
        last_update: None,
        evse_id,
        charging_pool_id: None,
        charging_station_id: Some(Text::new_unchecked("TEST 1")),
        charging_station_names: vec![
            InfoText::new("en", "ABC Charging Station Test").expect("valid"),
            InfoText::new("de", "ABC Testladestation").expect("valid"),
        ],
        hardware_manufacturer: Some(Text::new_unchecked("Charger Hardware Muster Company")),
        charging_station_image: None,
        sub_operator_name: None,
        address: address(),
        geo_coordinates: GeoCoordinates::Google { coordinates: "52.480495 13.356465".into() },
        plugs: vec![Plug::Type2Outlet],
        dynamic_power_level: Some(true),
        charging_facilities: vec![charging_facility()],
        renewable_energy: true,
        energy_source: None,
        environmental_impact: None,
        calibration_law_data_availability: Cl::Local,
        authentication_modes: vec![AuthenticationMode::NfcRfidClassic, AuthenticationMode::Remote],
        max_capacity: None,
        payment_options: vec![PaymentOption::Contract],
        value_added_services: vec![ValueAddedService::Reservation],
        accessibility: Accessibility::FreePubliclyAccessible,
        accessibility_location: None,
        hotline_phone_number: Text::new_unchecked("+49123123123123"),
        additional_info: None,
        charging_station_location_reference: None,
        geo_charging_point_entrance: None,
        is_open_24_hours: true,
        opening_times: None,
        hub_operator_id: None,
        clearinghouse_id: None,
        is_hubject_compatible: true,
        dynamic_info_available: DynamicInfoAvailable::True,
        extensions: Extensions::new(),
    }
}

/// A conformant `PullEvseDataRecord` for `evse_id`, as an EMP would receive it.
///
/// The operator is derived from the `EvseID`, exactly as Hubject does it, so the record passes the
/// consistency check in [`Validate`](crate::types::Validate).
///
/// # Panics
///
/// Panics if `evse_id` is not a valid `EvseID`.
#[must_use]
pub fn pull_evse_data_record(evse_id: &str) -> PullEvseDataRecord {
    let record = evse_data_record(evse_id);
    let operator_id = record.evse_id.operator_id();
    PullEvseDataRecord::from_evse_data_record(record, operator_id, Text::new_unchecked("ABC technologies"))
}

/// A status record saying `evse_id` is available.
///
/// # Panics
///
/// Panics if `evse_id` is not a valid `EvseID`.
#[must_use]
pub fn evse_status_record(evse_id: &str, status: EvseStatus) -> EvseStatusRecord {
    EvseStatusRecord {
        evse_id: evse_id.parse().expect("the sample EvseID is valid"),
        evse_status: status,
        extensions: Extensions::new(),
    }
}

/// An RFID identification for the spec's example card.
#[must_use]
pub fn rfid_identification() -> Identification {
    Identification::RfidMifareFamily(RfidMifareFamilyIdentification {
        uid: Uid::new("7568290FFF765F").expect("the sample UID is valid"),
    })
}

/// The session id from the spec's examples.
#[must_use]
pub fn session_id() -> SessionId {
    "f98efba4-02d8-4fa0-b810-9a9d50d2c527".parse().expect("the sample SessionID is valid")
}

/// An operator id matching [`evse_data_record`]'s default fleet.
#[must_use]
pub fn operator_id() -> OperatorId {
    "DE*ABC".parse().expect("the sample OperatorID is valid")
}

/// A conformant `AuthorizeStart` for `evse_id`.
///
/// # Panics
///
/// Panics if `evse_id` is not a valid `EvseID`.
#[must_use]
pub fn authorize_start_request(evse_id: &str) -> AuthorizeStartRequest {
    let evse_id: EvseId = evse_id.parse().expect("the sample EvseID is valid");
    AuthorizeStartRequest {
        session_id: None,
        cpo_partner_session_id: None,
        emp_partner_session_id: None,
        operator_id: evse_id.operator_id(),
        evse_id: Some(evse_id),
        identification: rfid_identification(),
        partner_product_id: None,
        extensions: Extensions::new(),
    }
}

/// A conformant `AuthorizeStop` for `evse_id` and `session_id`.
///
/// Carries the same medium as [`authorize_start_request`], which is what the specification
/// requires: *"the session `MUST` only be stopped with the same medium, which was used for
/// starting the session"*.
///
/// # Panics
///
/// Panics if `evse_id` is not a valid `EvseID`.
#[must_use]
pub fn authorize_stop_request(evse_id: &str, session_id: SessionId) -> AuthorizeStopRequest {
    let evse_id: EvseId = evse_id.parse().expect("the sample EvseID is valid");
    AuthorizeStopRequest {
        session_id,
        cpo_partner_session_id: None,
        emp_partner_session_id: None,
        operator_id: evse_id.operator_id(),
        evse_id: Some(evse_id),
        identification: rfid_identification(),
        extensions: Extensions::new(),
    }
}

/// A conformant CDR whose meter readings and `ConsumedEnergy` agree exactly.
///
/// # Panics
///
/// Panics if `evse_id` is not a valid `EvseID`.
#[must_use]
pub fn charge_detail_record(evse_id: &str, session_id: SessionId) -> ChargeDetailRecord {
    ChargeDetailRecord {
        session_id,
        cpo_partner_session_id: None,
        emp_partner_session_id: None,
        partner_product_id: Some(Text::new_unchecked("AC 1")),
        evse_id: evse_id.parse().expect("the sample EvseID is valid"),
        identification: rfid_identification(),
        session_start: "2020-09-23T14:00:00.000Z".parse().expect("valid"),
        charging_start: "2020-09-23T14:05:00.000Z".parse().expect("valid"),
        charging_end: "2020-09-23T15:05:00.000Z".parse().expect("valid"),
        session_end: "2020-09-23T15:10:00.000Z".parse().expect("valid"),
        meter_value_start: Some(Number::ZERO),
        meter_value_end: Some(Number::from(10)),
        meter_value_in_between: None,
        consumed_energy: Number::from(10),
        signed_metering_values: None,
        calibration_law_verification_info: None,
        hub_operator_id: None,
        hub_provider_id: None,
        extensions: Extensions::new(),
    }
}

/// A conformant `ChargingNotification` of type `Start`.
///
/// # Panics
///
/// Panics if `evse_id` is not a valid `EvseID`.
#[must_use]
pub fn charging_notification_start(evse_id: &str, session_id: SessionId) -> ChargingNotificationStart {
    let evse_id: EvseId = evse_id.parse().expect("the sample EvseID is valid");
    ChargingNotificationStart {
        notification_type: ChargingNotificationType::Start,
        session_id,
        cpo_partner_session_id: None,
        emp_partner_session_id: None,
        identification: Some(rfid_identification()),
        operator_id: evse_id.operator_id(),
        evse_id,
        charging_start: "2020-09-23T14:05:00.000Z".parse().expect("valid"),
        session_start: Some("2020-09-23T14:00:00.000Z".parse().expect("valid")),
        meter_value_start: Some(Number::ZERO),
        partner_product_id: None,
        extensions: Extensions::new(),
    }
}

/// A fleet of `count` charging points for `operator`, for exercising a crawl.
///
/// # Panics
///
/// Panics if `operator` is not a valid ISO `OperatorID`.
#[must_use]
pub fn fleet(operator: &str, count: u32) -> Vec<PullEvseDataRecord> {
    (0..count).map(|i| pull_evse_data_record(&format!("{operator}*E{i}"))).collect()
}

/// A timestamp that is stable across runs, for snapshot tests.
#[must_use]
pub fn fixed_time() -> DateTime {
    "2026-08-31T12:00:00.000Z".parse().expect("valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Validate;

    #[test]
    fn every_sample_is_conformant() {
        address().validate().expect("address");
        charging_facility().validate().expect("charging facility");
        evse_data_record("DE*ABC*E1").validate().expect("evse data record");
        pull_evse_data_record("DE*ABC*E1").validate().expect("pull evse data record");
        evse_status_record("DE*ABC*E1", EvseStatus::Available).validate().expect("status record");
        authorize_start_request("DE*ABC*E1").validate().expect("authorize start");
        charge_detail_record("DE*ABC*E1", session_id()).validate().expect("cdr");
        charging_notification_start("DE*ABC*E1", session_id()).validate().expect("notification");
        for record in fleet("DE*ABC", 5) {
            record.validate().expect("fleet record");
        }
    }

    #[test]
    fn the_pull_record_attributes_itself_to_the_right_operator() {
        let record = pull_evse_data_record("DE*XYZ*E1");
        assert_eq!(record.operator_id.as_str(), "DE*XYZ");
        assert!(record.validate().is_ok());
    }
}
