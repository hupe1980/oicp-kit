//! `EvseDataRecord` and the EVSE data/status pushes.

use serde::{Deserialize, Serialize};

use crate::oicp_open_enum;
use crate::types::{
    Accessibility, AccessibilityLocation, ActionType, Address, AuthenticationMode,
    CalibrationLawDataAvailability, ChargingFacility, ChargingPoolId, DateTime, DynamicInfoAvailable,
    EnergySource, EnvironmentalImpact, EvseId, Extensions, GeoCoordinates, InfoText, Number, Opening,
    OpeningTimes, OperatorId, PaymentOption, Plug, Text, Validate, Validator, ValueAddedService,
    ViolationCode, strict_builder, validate_fields,
};

oicp_open_enum! {
    /// What changed about a record since the EMP's last pull.
    ///
    /// Hubject assigns this to every record in a `PullEvseData` response that was made with
    /// `LastCall`. It is *not* something a CPO sets on a push — the field appears in the push
    /// schema too, but it is Hubject's to write. [`crate::sync`] is built on it.
    pub enum DeltaType {
        /// The record is new since the last call.
        Insert = "insert",
        /// The record existed and has changed.
        Update = "update",
        /// The record has been withdrawn.
        Delete = "delete",
    }
}

oicp_open_enum! {
    /// The status of a charging spot.
    pub enum EvseStatus {
        /// Available for charging.
        Available = "Available",
        /// Reserved, and not available for charging.
        Reserved = "Reserved",
        /// Busy.
        Occupied = "Occupied",
        /// Out of service, and not available for charging.
        OutOfService = "OutOfService",
        /// The requested EvseID does not exist in the Hubject database.
        EvseNotFound = "EvseNotFound",
        /// No status information available.
        Unknown = "Unknown",
    }
}

impl EvseStatus {
    /// Whether a session could start at a spot in this state.
    #[must_use]
    pub fn is_chargeable(&self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Everything static about one charging spot.
///
/// This is the largest object in OICP and the one a CPO spends most of its time getting right: it
/// is what an EV driver sees on a map, so a wrong `GeoCoordinates` or a missing `Plugs` entry is a
/// driver who cannot find or cannot use the charge point.
///
/// Sixteen of its fields are mandatory. [`Validate`] checks the ones with cross-field rules — the
/// `No Payment` exclusivity, the `HotlinePhoneNumber` pattern, `IsOpen24Hours` against
/// `OpeningTimes` — and the strict builder refuses to produce a record that breaks them.
///
/// Spec: `EvseDataRecord`. The EMP sees a superset of this on a pull — see
/// [`emp::PullEvseDataRecord`](crate::emp::PullEvseDataRecord), which adds the operator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct EvseDataRecord {
    /// What changed about this record, when Hubject answers a delta pull.
    #[serde(rename = "deltaType", default, skip_serializing_if = "Option::is_none")]
    pub delta_type: Option<DeltaType>,
    /// When Hubject last saw this record change.
    #[serde(rename = "lastUpdate", default, skip_serializing_if = "Option::is_none")]
    pub last_update: Option<DateTime>,
    /// The charging spot this record describes.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// The pool this spot belongs to, per the emi³ definition.
    #[serde(rename = "ChargingPoolID", default, skip_serializing_if = "Option::is_none")]
    pub charging_pool_id: Option<ChargingPoolId>,
    /// The station this spot belongs to.
    ///
    /// See erratum [`OICP23-E002`](crate::types::ERRATA): the data-type table spells this
    /// `ChargingStationId`, but every example Hubject publishes spells it `ChargingStationID`.
    /// This crate writes the table's spelling and accepts both.
    #[serde(
        rename = "ChargingStationId",
        alias = "ChargingStationID",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[builder(into)]
    pub charging_station_id: Option<Text<50>>,
    /// The station's name, in one or more languages.
    #[serde(rename = "ChargingStationNames")]
    pub charging_station_names: Vec<InfoText>,
    /// Who made the charging point.
    #[serde(rename = "HardwareManufacturer", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub hardware_manufacturer: Option<Text<50>>,
    /// A URL to a photograph of the charging point.
    #[serde(rename = "ChargingStationImage", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub charging_station_image: Option<Text<200>>,
    /// The sub-operator that owns the station, when the CPO is a hub operator.
    #[serde(rename = "SubOperatorName", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub sub_operator_name: Option<Text<100>>,
    /// Where the charging point is.
    #[serde(rename = "Address")]
    pub address: Address,
    /// Where the charging point is, precisely.
    #[serde(rename = "GeoCoordinates")]
    pub geo_coordinates: GeoCoordinates,
    /// The connectors on offer.
    #[serde(rename = "Plugs")]
    pub plugs: Vec<Plug>,
    /// Whether the point can deliver different power outputs.
    #[serde(rename = "DynamicPowerLevel", default, skip_serializing_if = "Option::is_none")]
    pub dynamic_power_level: Option<bool>,
    /// What the point can deliver.
    #[serde(rename = "ChargingFacilities")]
    pub charging_facilities: Vec<ChargingFacility>,
    /// Whether the point supplies only renewable energy.
    #[serde(rename = "RenewableEnergy")]
    pub renewable_energy: bool,
    /// Where the energy comes from.
    #[serde(rename = "EnergySource", default, skip_serializing_if = "Option::is_none")]
    pub energy_source: Option<Vec<EnergySource>>,
    /// What the energy costs the environment.
    #[serde(rename = "EnvironmentalImpact", default, skip_serializing_if = "Option::is_none")]
    pub environmental_impact: Option<EnvironmentalImpact>,
    /// Whether the point can supply German calibration-law data, and how.
    #[serde(rename = "CalibrationLawDataAvailability")]
    pub calibration_law_data_availability: CalibrationLawDataAvailability,
    /// How a driver may authenticate.
    #[serde(rename = "AuthenticationModes")]
    pub authentication_modes: Vec<AuthenticationMode>,
    /// The capacity of a built-in battery, in kWh, if the EVSE has one.
    #[serde(rename = "MaxCapacity", default, skip_serializing_if = "Option::is_none")]
    pub max_capacity: Option<Number>,
    /// How a driver may pay.
    #[serde(rename = "PaymentOptions")]
    pub payment_options: Vec<PaymentOption>,
    /// What else the point offers.
    #[serde(rename = "ValueAddedServices")]
    pub value_added_services: Vec<ValueAddedService>,
    /// How the point can be reached.
    #[serde(rename = "Accessibility")]
    pub accessibility: Accessibility,
    /// Where the point sits, physically.
    #[serde(rename = "AccessibilityLocation", default, skip_serializing_if = "Option::is_none")]
    pub accessibility_location: Option<AccessibilityLocation>,
    /// The operator's support line, as `+` followed by 5 to 15 digits.
    #[serde(rename = "HotlinePhoneNumber")]
    #[builder(into)]
    pub hotline_phone_number: Text<20>,
    /// Anything else a driver should know.
    #[serde(rename = "AdditionalInfo", default, skip_serializing_if = "Option::is_none")]
    pub additional_info: Option<Vec<InfoText>>,
    /// How to find the charging point once you are there.
    #[serde(rename = "ChargingStationLocationReference", default, skip_serializing_if = "Option::is_none")]
    pub charging_station_location_reference: Option<Vec<InfoText>>,
    /// Where to drive in, if that is not where the charge point is.
    #[serde(rename = "GeoChargingPointEntrance", default, skip_serializing_if = "Option::is_none")]
    pub geo_charging_point_entrance: Option<GeoCoordinates>,
    /// Whether the spot is open around the clock.
    #[serde(rename = "IsOpen24Hours")]
    pub is_open_24_hours: bool,
    /// When the spot is open, if not around the clock.
    #[serde(rename = "OpeningTimes", default, skip_serializing_if = "Option::is_none")]
    pub opening_times: Option<Vec<OpeningTimes>>,
    /// The hub operator that bundles this CPO, if any.
    #[serde(rename = "HubOperatorID", default, skip_serializing_if = "Option::is_none")]
    pub hub_operator_id: Option<OperatorId>,
    /// The clearing house, for roaming between clearing houses.
    #[serde(rename = "ClearinghouseID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub clearinghouse_id: Option<Text<20>>,
    /// Whether the spot can be started and stopped remotely through Hubject.
    #[serde(rename = "IsHubjectCompatible")]
    pub is_hubject_compatible: bool,
    /// Whether the CPO also publishes dynamic status for this record.
    #[serde(rename = "DynamicInfoAvailable")]
    pub dynamic_info_available: DynamicInfoAvailable,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl EvseDataRecord {
    /// Whether this charging point is open at `at`.
    ///
    /// OICP's opening times are **local** times, and the offset to interpret them with is in the
    /// address. This combines the two:
    ///
    /// ```
    /// # use oicp_kit::testkit::samples;
    /// # use oicp_kit::types::{DaySelection, Extensions, HourMinute, Opening, OpeningTimes, Period};
    /// let mut record = samples::evse_data_record("DE*ABC*E1");
    /// record.is_open_24_hours = false;
    /// record.opening_times = Some(vec![OpeningTimes {
    ///     period: vec![Period { begin: HourMinute::new("08:00")?, end: HourMinute::new("18:00")? }],
    ///     on: DaySelection::Workdays,
    ///     extensions: Extensions::new(),
    /// }]);
    /// // The address says UTC+01:00, so 07:00 UTC is 08:00 locally — just open.
    /// assert_eq!(record.is_open_at(&"2026-08-31T07:30:00.000Z".parse()?), Opening::Open);
    /// assert_eq!(record.is_open_at(&"2026-08-31T05:00:00.000Z".parse()?), Opening::Closed);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Returns [`Opening::Unknown`] when the record does not carry enough to decide — which an EMP
    /// routing a driver should treat differently from a definite "closed".
    #[must_use]
    pub fn is_open_at(&self, at: &DateTime) -> Opening {
        crate::types::opening_at(
            self.is_open_24_hours,
            self.opening_times.as_ref(),
            self.address.time_zone.as_deref(),
            at,
        )
    }
}

/// Checks the rules an `EvseDataRecord` and its EMP-side twin share.
///
/// The two records are structurally different types with the same rules, so the shared fields are
/// passed individually rather than behind a trait that would exist only for this call.
#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_evse_common(
    v: &mut Validator,
    plugs: &[Plug],
    charging_facilities: &[ChargingFacility],
    authentication_modes: &[AuthenticationMode],
    payment_options: &[PaymentOption],
    charging_station_names: &[InfoText],
    hotline_phone_number: &Text<20>,
    is_open_24_hours: bool,
    opening_times: Option<&Vec<OpeningTimes>>,
) {
    for (name, empty) in [
        ("Plugs", plugs.is_empty()),
        ("ChargingFacilities", charging_facilities.is_empty()),
        ("AuthenticationModes", authentication_modes.is_empty()),
        ("PaymentOptions", payment_options.is_empty()),
        ("ChargingStationNames", charging_station_names.is_empty()),
    ] {
        if empty {
            v.report_at(
                name,
                ViolationCode::EmptyRequiredList,
                format!("{name} is mandatory and must not be empty"),
            );
        }
    }

    // "No Payment can not be combined with other payment option."
    if payment_options.contains(&PaymentOption::NoPayment) && payment_options.len() > 1 {
        v.report_at(
            "PaymentOptions",
            ViolationCode::Inconsistent,
            "'No Payment' cannot be combined with another payment option",
        );
    }

    // `^\+[0-9]{5,15}$`
    let phone = hotline_phone_number.as_str();
    let digits = phone.strip_prefix('+').unwrap_or("");
    if !phone.starts_with('+')
        || !(5..=15).contains(&digits.len())
        || !digits.bytes().all(|c| c.is_ascii_digit())
    {
        v.report_at(
            "HotlinePhoneNumber",
            ViolationCode::PatternMismatch,
            format!("{phone:?} is not a '+' followed by 5 to 15 digits"),
        );
    }

    // A station that is not open around the clock has to say when it is.
    match (is_open_24_hours, opening_times) {
        (false, None) => v.report_at(
            "OpeningTimes",
            ViolationCode::MissingConditional,
            "IsOpen24Hours is false, so OpeningTimes is required — otherwise no driver can tell when \
             the station can be used",
        ),
        (false, Some(times)) if times.is_empty() => v.report_at(
            "OpeningTimes",
            ViolationCode::EmptyRequiredList,
            "IsOpen24Hours is false, so OpeningTimes must list at least one period",
        ),
        (true, Some(times)) if !times.is_empty() => v.report_at(
            "OpeningTimes",
            ViolationCode::Inconsistent,
            "IsOpen24Hours is true, so OpeningTimes contradicts it",
        ),
        _ => {}
    }
}

impl Validate for EvseDataRecord {
    fn validate_in(&self, v: &mut Validator) {
        validate_evse_common(
            v,
            &self.plugs,
            &self.charging_facilities,
            &self.authentication_modes,
            &self.payment_options,
            &self.charging_station_names,
            &self.hotline_phone_number,
            self.is_open_24_hours,
            self.opening_times.as_ref(),
        );
        validate_fields!(
            self,
            v,
            evse_id as "EvseID",
            charging_pool_id as "ChargingPoolID",
            charging_station_id as "ChargingStationId",
            charging_station_names as "ChargingStationNames",
            hardware_manufacturer as "HardwareManufacturer",
            charging_station_image as "ChargingStationImage",
            sub_operator_name as "SubOperatorName",
            address as "Address",
            geo_coordinates as "GeoCoordinates",
            plugs as "Plugs",
            charging_facilities as "ChargingFacilities",
            energy_source as "EnergySource",
            environmental_impact as "EnvironmentalImpact",
            calibration_law_data_availability as "CalibrationLawDataAvailability",
            authentication_modes as "AuthenticationModes",
            payment_options as "PaymentOptions",
            value_added_services as "ValueAddedServices",
            accessibility as "Accessibility",
            accessibility_location as "AccessibilityLocation",
            additional_info as "AdditionalInfo",
            charging_station_location_reference as "ChargingStationLocationReference",
            geo_charging_point_entrance as "GeoChargingPointEntrance",
            opening_times as "OpeningTimes",
            hub_operator_id as "HubOperatorID",
            clearinghouse_id as "ClearinghouseID",
            dynamic_info_available as "DynamicInfoAvailable",
            last_update as "lastUpdate",
        );
    }
}

/// One operator's EVSE records, as they go up to Hubject.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct OperatorEvseData {
    /// The operator these records belong to.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// The operator's name, as a driver should see it.
    #[serde(rename = "OperatorName")]
    #[builder(into)]
    pub operator_name: Text<100>,
    /// The records.
    #[serde(rename = "EvseDataRecord")]
    pub evse_data_record: Vec<EvseDataRecord>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for OperatorEvseData {
    fn validate_in(&self, v: &mut Validator) {
        // Hubject derives the operator from each EvseID; a record that names a different operator
        // than the envelope is rejected with 018 Inconsistent EvseID.
        for (i, record) in self.evse_data_record.iter().enumerate() {
            let derived = record.evse_id.operator_id();
            if derived != self.operator_id {
                v.enter("EvseDataRecord");
                v.enter(&i.to_string());
                v.report_at(
                    "EvseID",
                    ViolationCode::Inconsistent,
                    format!(
                        "{} belongs to operator {derived}, but this push is for {}; \
                         Hubject answers that with 018 Inconsistent EvseID",
                        record.evse_id, self.operator_id
                    ),
                );
                v.leave();
                v.leave();
            }
        }
        validate_fields!(
            self,
            v,
            operator_id as "OperatorID",
            operator_name as "OperatorName",
            evse_data_record as "EvseDataRecord",
        );
    }
}

/// Uploads static EVSE data to Hubject.
///
/// # `ActionType` is the dangerous part
///
/// [`ActionType::FullLoad`] **replaces** everything Hubject holds for the operator. Prefer
/// [`sync::PushPlanner`](crate::sync::PushPlanner), which computes the minimal insert/update/delete
/// set from a snapshot, over choosing the action by hand.
///
/// Spec: `eRoamingPushEvseData_V2.3`, `POST /evsepush/v23/operators/{operatorID}/data-records`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PushEvseDataRequest {
    /// What Hubject should do with the payload.
    #[serde(rename = "ActionType")]
    pub action_type: ActionType,
    /// The payload.
    #[serde(rename = "OperatorEvseData")]
    pub operator_evse_data: OperatorEvseData,
}

impl Validate for PushEvseDataRequest {
    fn validate_in(&self, v: &mut Validator) {
        if self.action_type.is_destructive_replace() && self.operator_evse_data.evse_data_record.is_empty() {
            v.report(
                ViolationCode::Inconsistent,
                "a fullLoad with no records removes every charging point this operator has from the \
                 roaming network; send an explicit delete if that is the intention",
            );
        }
        validate_fields!(self, v, action_type as "ActionType", operator_evse_data as "OperatorEvseData");
    }
}

/// The status of one charging spot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct EvseStatusRecord {
    /// The spot.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// Its status.
    #[serde(rename = "EvseStatus")]
    pub evse_status: EvseStatus,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for EvseStatusRecord {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, evse_id as "EvseID", evse_status as "EvseStatus");
    }
}

/// One operator's EVSE statuses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct OperatorEvseStatus {
    /// The operator.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// The operator's name.
    #[serde(rename = "OperatorName", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub operator_name: Option<Text<100>>,
    /// The statuses.
    #[serde(rename = "EvseStatusRecord")]
    pub evse_status_record: Vec<EvseStatusRecord>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for OperatorEvseStatus {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(
            self,
            v,
            operator_id as "OperatorID",
            operator_name as "OperatorName",
            evse_status_record as "EvseStatusRecord",
        );
    }
}

/// Uploads dynamic EVSE status to Hubject.
///
/// The spec recommends sending status at a frequency of one to five minutes.
///
/// Spec: `eRoamingPushEvseStatus_V2.1`,
/// `POST /evsepush/v21/operators/{operatorID}/status-records`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PushEvseStatusRequest {
    /// What Hubject should do with the payload.
    #[serde(rename = "ActionType")]
    pub action_type: ActionType,
    /// The payload.
    #[serde(rename = "OperatorEvseStatus")]
    pub operator_evse_status: OperatorEvseStatus,
}

impl Validate for PushEvseStatusRequest {
    fn validate_in(&self, v: &mut Validator) {
        if self.action_type.is_destructive_replace()
            && self.operator_evse_status.evse_status_record.is_empty()
        {
            v.report(
                ViolationCode::Inconsistent,
                "a fullLoad with no records withdraws the status of every charging point this \
                 operator has",
            );
        }
        validate_fields!(self, v, action_type as "ActionType", operator_evse_status as "OperatorEvseStatus");
    }
}

strict_builder!(EvseDataRecord, EvseDataRecordBuilder, evse_data_record_builder);
strict_builder!(OperatorEvseData, OperatorEvseDataBuilder, operator_evse_data_builder);
strict_builder!(PushEvseDataRequest, PushEvseDataRequestBuilder, push_evse_data_request_builder);
strict_builder!(EvseStatusRecord, EvseStatusRecordBuilder, evse_status_record_builder);
strict_builder!(OperatorEvseStatus, OperatorEvseStatusBuilder, operator_evse_status_builder);
strict_builder!(PushEvseStatusRequest, PushEvseStatusRequestBuilder, push_evse_status_request_builder);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_record_keeps_fields_hubject_adds_later() {
        let json = r#"{"EvseID":"DE*XYZ*ETEST1","EvseStatus":"Available","HubjectAddedThis":42}"#;
        let record: EvseStatusRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.extensions.get::<u32>("HubjectAddedThis").unwrap(), Some(42));
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
    }

    #[test]
    fn a_status_value_hubject_adds_later_is_kept() {
        let record: EvseStatusRecord =
            serde_json::from_str(r#"{"EvseID":"DE*XYZ*ETEST1","EvseStatus":"Maintenance"}"#).unwrap();
        assert!(!record.evse_status.is_known());
        assert!(!record.evse_status.is_chargeable());
        assert_eq!(record.evse_status.as_str(), "Maintenance");
    }

    #[test]
    fn charging_station_id_decodes_under_both_spellings() {
        // Erratum OICP23-E002: the table says ChargingStationId, every example says …ID.
        #[derive(serde::Deserialize)]
        struct Probe {
            #[serde(rename = "ChargingStationId", alias = "ChargingStationID")]
            id: String,
        }
        for key in ["ChargingStationId", "ChargingStationID"] {
            let json = format!(r#"{{"{key}":"TEST 1"}}"#);
            let probe: Probe = serde_json::from_str(&json).unwrap();
            assert_eq!(probe.id, "TEST 1");
        }
    }

    #[test]
    fn a_full_load_with_no_records_is_reported() {
        let push = PushEvseDataRequest {
            action_type: ActionType::FullLoad,
            operator_evse_data: OperatorEvseData {
                operator_id: "DE*ABC".parse().unwrap(),
                operator_name: Text::new("ABC technologies").unwrap(),
                evse_data_record: vec![],
                extensions: Extensions::new(),
            },
        };
        let err = push.validate().unwrap_err();
        assert!(err.as_slice()[0].message.contains("removes every charging point"));

        // The same payload as a delete is fine — that is what delete means.
        let push = PushEvseDataRequest { action_type: ActionType::Delete, ..push };
        assert!(push.validate().is_ok());
    }

    #[test]
    fn a_record_whose_operator_does_not_match_the_push_is_reported() {
        let data = OperatorEvseData {
            operator_id: "DE*ABC".parse().unwrap(),
            operator_name: Text::new("ABC technologies").unwrap(),
            evse_data_record: vec![],
            extensions: Extensions::new(),
        };
        assert!(data.validate().is_ok());
        // The cross-check itself is exercised end to end in tests/wire.rs, where a full record
        // is available to attach.
    }
}
