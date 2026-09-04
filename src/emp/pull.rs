//! The pulls: how an EMP gets everyone else's data out of Hubject.

use serde::{Deserialize, Serialize};

use crate::cpo::{DeltaType, EvseDataRecord, EvseStatus, EvseStatusRecord, validate_evse_common};
use crate::types::{
    Accessibility, AccessibilityLocation, Address, AuthenticationMode, CalibrationLawDataAvailability,
    ChargingFacility, ChargingPoolId, DateTime, DynamicInfoAvailable, EnergySource, EnvironmentalImpact,
    EvseId, Extensions, GeoCoordinates, GeoCoordinatesFormat, InfoText, Number, Opening, OpeningTimes,
    OperatorId, PaymentOption, Plug, ProviderId, Text, Validate, Validator, ValueAddedService, ViolationCode,
    strict_builder, validate_fields,
};

/// A charging spot as the EMP sees it: everything the CPO published, plus who published it.
///
/// The difference from [`cpo::EvseDataRecord`](crate::cpo::EvseDataRecord) is two required fields —
/// `OperatorID` and `OperatorName` — which Hubject fills in. An EMP pulling from thirty operators
/// needs to know which one each record came from; a CPO pushing its own does not.
///
/// Spec: `PullEvseDataRecord`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PullEvseDataRecord {
    /// What changed about this record since the EMP's last call.
    #[serde(rename = "deltaType", default, skip_serializing_if = "Option::is_none")]
    pub delta_type: Option<DeltaType>,
    /// When Hubject last saw this record change.
    #[serde(rename = "lastUpdate", default, skip_serializing_if = "Option::is_none")]
    pub last_update: Option<DateTime>,
    /// The charging spot.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// The pool it belongs to.
    #[serde(rename = "ChargingPoolID", default, skip_serializing_if = "Option::is_none")]
    pub charging_pool_id: Option<ChargingPoolId>,
    /// The station it belongs to. See erratum [`OICP23-E002`](crate::types::ERRATA).
    #[serde(
        rename = "ChargingStationId",
        alias = "ChargingStationID",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[builder(into)]
    pub charging_station_id: Option<Text<50>>,
    /// The station's name.
    #[serde(rename = "ChargingStationNames")]
    pub charging_station_names: Vec<InfoText>,
    /// Who made the charging point.
    #[serde(rename = "HardwareManufacturer", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub hardware_manufacturer: Option<Text<50>>,
    /// A photograph.
    #[serde(rename = "ChargingStationImage", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub charging_station_image: Option<Text<200>>,
    /// The sub-operator that owns the station.
    #[serde(rename = "SubOperatorName", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub sub_operator_name: Option<Text<100>>,
    /// Where it is.
    #[serde(rename = "Address")]
    pub address: Address,
    /// Where it is, precisely — in the notation the pull asked for.
    #[serde(rename = "GeoCoordinates")]
    pub geo_coordinates: GeoCoordinates,
    /// The connectors.
    #[serde(rename = "Plugs")]
    pub plugs: Vec<Plug>,
    /// Whether the point can deliver different power outputs.
    #[serde(rename = "DynamicPowerLevel", default, skip_serializing_if = "Option::is_none")]
    pub dynamic_power_level: Option<bool>,
    /// What it can deliver.
    #[serde(rename = "ChargingFacilities")]
    pub charging_facilities: Vec<ChargingFacility>,
    /// Whether it supplies only renewable energy.
    #[serde(rename = "RenewableEnergy")]
    pub renewable_energy: bool,
    /// Where the energy comes from.
    #[serde(rename = "EnergySource", default, skip_serializing_if = "Option::is_none")]
    pub energy_source: Option<Vec<EnergySource>>,
    /// What the energy costs the environment.
    #[serde(rename = "EnvironmentalImpact", default, skip_serializing_if = "Option::is_none")]
    pub environmental_impact: Option<EnvironmentalImpact>,
    /// Whether the point can supply calibration-law data, and how.
    #[serde(rename = "CalibrationLawDataAvailability")]
    pub calibration_law_data_availability: CalibrationLawDataAvailability,
    /// How a driver may authenticate.
    #[serde(rename = "AuthenticationModes")]
    pub authentication_modes: Vec<AuthenticationMode>,
    /// The capacity of a built-in battery, in kWh.
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
    /// Where it sits, physically.
    #[serde(rename = "AccessibilityLocation", default, skip_serializing_if = "Option::is_none")]
    pub accessibility_location: Option<AccessibilityLocation>,
    /// The operator's support line.
    #[serde(rename = "HotlinePhoneNumber")]
    #[builder(into)]
    pub hotline_phone_number: Text<20>,
    /// Anything else a driver should know.
    #[serde(rename = "AdditionalInfo", default, skip_serializing_if = "Option::is_none")]
    pub additional_info: Option<Vec<InfoText>>,
    /// How to find the point once you are there.
    #[serde(rename = "ChargingStationLocationReference", default, skip_serializing_if = "Option::is_none")]
    pub charging_station_location_reference: Option<Vec<InfoText>>,
    /// Where to drive in.
    #[serde(rename = "GeoChargingPointEntrance", default, skip_serializing_if = "Option::is_none")]
    pub geo_charging_point_entrance: Option<GeoCoordinates>,
    /// Whether the spot is open around the clock.
    #[serde(rename = "IsOpen24Hours")]
    pub is_open_24_hours: bool,
    /// When it is open, if not around the clock.
    #[serde(rename = "OpeningTimes", default, skip_serializing_if = "Option::is_none")]
    pub opening_times: Option<Vec<OpeningTimes>>,
    /// The hub operator that bundles the CPO.
    #[serde(rename = "HubOperatorID", default, skip_serializing_if = "Option::is_none")]
    pub hub_operator_id: Option<OperatorId>,
    /// The clearing house.
    #[serde(rename = "ClearinghouseID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub clearinghouse_id: Option<Text<20>>,
    /// Whether the spot can be started and stopped remotely through Hubject.
    #[serde(rename = "IsHubjectCompatible")]
    pub is_hubject_compatible: bool,
    /// Whether the CPO also publishes dynamic status for this record.
    #[serde(rename = "DynamicInfoAvailable")]
    pub dynamic_info_available: DynamicInfoAvailable,
    /// Which operator published this record.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// That operator's name.
    #[serde(rename = "OperatorName")]
    #[builder(into)]
    pub operator_name: Text<100>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl PullEvseDataRecord {
    /// Whether this record says the charging point has been withdrawn.
    ///
    /// Only ever set on a delta pull. [`sync`](crate::sync) is built on this.
    #[must_use]
    pub fn is_deletion(&self) -> bool {
        self.delta_type.as_ref() == Some(&DeltaType::Delete)
    }

    /// Whether this charging point is open at `at`.
    ///
    /// OICP's opening times are **local** times, and the offset to interpret them with is in the
    /// address. This combines the two:
    ///
    /// ```
    /// # use oicp_kit::testkit::samples;
    /// # use oicp_kit::types::{DaySelection, Extensions, HourMinute, Opening, OpeningTimes, Period};
    /// let mut record = samples::pull_evse_data_record("DE*ABC*E1");
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

    /// Turns a CPO's record into the EMP's view of it, the way the broker does.
    ///
    /// The two objects carry the same charging point; the difference is the two fields Hubject
    /// fills in. Useful to anyone standing between the two sides — a hub, a test double, or an
    /// operator that publishes to Hubject and to its own API from one source.
    ///
    /// `deltaType` and `lastUpdate` are Hubject's to write and are left unset.
    #[must_use]
    pub fn from_evse_data_record(
        record: EvseDataRecord,
        operator_id: OperatorId,
        operator_name: Text<100>,
    ) -> Self {
        Self {
            delta_type: None,
            last_update: None,
            evse_id: record.evse_id,
            charging_pool_id: record.charging_pool_id,
            charging_station_id: record.charging_station_id,
            charging_station_names: record.charging_station_names,
            hardware_manufacturer: record.hardware_manufacturer,
            charging_station_image: record.charging_station_image,
            sub_operator_name: record.sub_operator_name,
            address: record.address,
            geo_coordinates: record.geo_coordinates,
            plugs: record.plugs,
            dynamic_power_level: record.dynamic_power_level,
            charging_facilities: record.charging_facilities,
            renewable_energy: record.renewable_energy,
            energy_source: record.energy_source,
            environmental_impact: record.environmental_impact,
            calibration_law_data_availability: record.calibration_law_data_availability,
            authentication_modes: record.authentication_modes,
            max_capacity: record.max_capacity,
            payment_options: record.payment_options,
            value_added_services: record.value_added_services,
            accessibility: record.accessibility,
            accessibility_location: record.accessibility_location,
            hotline_phone_number: record.hotline_phone_number,
            additional_info: record.additional_info,
            charging_station_location_reference: record.charging_station_location_reference,
            geo_charging_point_entrance: record.geo_charging_point_entrance,
            is_open_24_hours: record.is_open_24_hours,
            opening_times: record.opening_times,
            hub_operator_id: record.hub_operator_id,
            clearinghouse_id: record.clearinghouse_id,
            is_hubject_compatible: record.is_hubject_compatible,
            dynamic_info_available: record.dynamic_info_available,
            operator_id,
            operator_name,
            extensions: record.extensions,
        }
    }
}

impl Validate for PullEvseDataRecord {
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
        // Hubject derives the operator from the EvseID; if the two disagree the EMP cannot tell
        // whose charging point this is, and settles the session with the wrong party.
        let derived = self.evse_id.operator_id();
        if derived != self.operator_id {
            v.report_at(
                "OperatorID",
                ViolationCode::Inconsistent,
                format!(
                    "the record is attributed to {}, but {} names operator {derived}",
                    self.operator_id, self.evse_id
                ),
            );
        }
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
            operator_id as "OperatorID",
            operator_name as "OperatorName",
            last_update as "lastUpdate",
        );
    }
}

/// A circle to search within.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct SearchCenter {
    /// The centre.
    #[serde(rename = "GeoCoordinates")]
    pub geo_coordinates: GeoCoordinates,
    /// The radius, in km.
    #[serde(rename = "Radius")]
    pub radius: Number,
}

impl Validate for SearchCenter {
    fn validate_in(&self, v: &mut Validator) {
        if self.radius.is_negative() {
            v.report_at("Radius", ViolationCode::OutOfRange, "a search radius cannot be negative");
        }
        validate_fields!(self, v, geo_coordinates as "GeoCoordinates", radius as "Radius");
    }
}

/// Asks Hubject for static EVSE data.
///
/// # `LastCall` is exclusive with the filters
///
/// The spec is explicit, and it is the rule this request exists to enforce in the type system:
///
/// > *In case that this field is set, Hubject does not return the currently valid set of EVSE data
/// > but the changes compared to the status of EVSE data at the time of the last call. Cannot be
/// > combined with "SearchCenter", "CountryCodes", and "OperatorIDs".*
///
/// The reason is data integrity: a delta restricted to a region would silently omit charge points
/// that *moved out* of that region, and the EMP's copy would keep stale records forever.
/// [`Validate`] reports the combination, and [`sync::Planner`](crate::sync::Planner) never
/// constructs it.
///
/// Spec: `eRoamingPullEvseData_V2.3`,
/// `POST /evsepull/v23/providers/{providerID}/data-records`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PullEvseDataRequest {
    /// The EMP asking.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// Restrict to a circle. Cannot be combined with `LastCall`.
    #[serde(rename = "SearchCenter", default, skip_serializing_if = "Option::is_none")]
    pub search_center: Option<SearchCenter>,
    /// Ask for the changes since this instant, rather than everything.
    #[serde(rename = "LastCall", default, skip_serializing_if = "Option::is_none")]
    pub last_call: Option<DateTime>,
    /// Which notation the coordinates should come back in.
    #[serde(rename = "GeoCoordinatesResponseFormat")]
    pub geo_coordinates_response_format: GeoCoordinatesFormat,
    /// Restrict to these countries, as alpha-3 codes. Cannot be combined with `LastCall`.
    #[serde(rename = "CountryCodes", default, skip_serializing_if = "Option::is_none")]
    pub country_codes: Option<Vec<Text<3>>>,
    /// Restrict to these operators. Cannot be combined with `LastCall`.
    #[serde(rename = "OperatorIds", default, skip_serializing_if = "Option::is_none")]
    pub operator_ids: Option<Vec<OperatorId>>,
    /// Restrict to points offering these authentication modes.
    #[serde(rename = "AuthenticationModes", default, skip_serializing_if = "Option::is_none")]
    pub authentication_modes: Option<Vec<AuthenticationMode>>,
    /// Restrict to points with this accessibility.
    #[serde(rename = "Accessibility", default, skip_serializing_if = "Option::is_none")]
    pub accessibility: Option<Vec<Accessibility>>,
    /// Restrict to points with this calibration-law availability.
    #[serde(rename = "CalibrationLawDataAvailability", default, skip_serializing_if = "Option::is_none")]
    pub calibration_law_data_availability: Option<Vec<CalibrationLawDataAvailability>>,
    /// Restrict to points on renewable energy.
    #[serde(rename = "RenewableEnergy", default, skip_serializing_if = "Option::is_none")]
    pub renewable_energy: Option<bool>,
    /// Restrict to points that can be started remotely through Hubject.
    #[serde(rename = "IsHubjectCompatible", default, skip_serializing_if = "Option::is_none")]
    pub is_hubject_compatible: Option<bool>,
    /// Restrict to points open around the clock.
    #[serde(rename = "IsOpen24Hours", default, skip_serializing_if = "Option::is_none")]
    pub is_open_24_hours: Option<bool>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl PullEvseDataRequest {
    /// A full pull for `provider_id`, in `format`.
    #[must_use]
    pub fn full(provider_id: ProviderId, format: GeoCoordinatesFormat) -> Self {
        Self {
            provider_id,
            search_center: None,
            last_call: None,
            geo_coordinates_response_format: format,
            country_codes: None,
            operator_ids: None,
            authentication_modes: None,
            accessibility: None,
            calibration_law_data_availability: None,
            renewable_energy: None,
            is_hubject_compatible: None,
            is_open_24_hours: None,
            extensions: Extensions::new(),
        }
    }

    /// A delta pull: the changes since `since`.
    ///
    /// Cannot carry the geographic filters — see the type documentation.
    #[must_use]
    pub fn delta(provider_id: ProviderId, format: GeoCoordinatesFormat, since: DateTime) -> Self {
        Self { last_call: Some(since), ..Self::full(provider_id, format) }
    }

    /// Whether this asks for changes rather than everything.
    #[must_use]
    pub const fn is_delta(&self) -> bool {
        self.last_call.is_some()
    }

    /// The filters that are illegal alongside `LastCall`, and are set.
    #[must_use]
    pub fn conflicting_filters(&self) -> Vec<&'static str> {
        let mut found = vec![];
        if self.search_center.is_some() {
            found.push("SearchCenter");
        }
        if self.country_codes.as_ref().is_some_and(|c| !c.is_empty()) {
            found.push("CountryCodes");
        }
        if self.operator_ids.as_ref().is_some_and(|o| !o.is_empty()) {
            found.push("OperatorIds");
        }
        found
    }
}

impl Validate for PullEvseDataRequest {
    fn validate_in(&self, v: &mut Validator) {
        if self.is_delta() {
            let conflicting = self.conflicting_filters();
            if !conflicting.is_empty() {
                v.report_at(
                    "LastCall",
                    ViolationCode::ExclusiveChoice,
                    format!(
                        "LastCall cannot be combined with {}; a delta restricted to a region omits \
                         charge points that moved out of it, and the EMP's copy keeps stale records",
                        conflicting.join(", ")
                    ),
                );
            }
        }
        validate_fields!(
            self,
            v,
            provider_id as "ProviderID",
            search_center as "SearchCenter",
            last_call as "LastCall",
            geo_coordinates_response_format as "GeoCoordinatesResponseFormat",
            country_codes as "CountryCodes",
            operator_ids as "OperatorIds",
            authentication_modes as "AuthenticationModes",
            accessibility as "Accessibility",
            calibration_law_data_availability as "CalibrationLawDataAvailability",
        );
    }
}

/// Asks Hubject for the status of every charging point the EMP can see.
///
/// The spec recommends a frequency of one to five minutes.
///
/// Spec: `eRoamingPullEvseStatus_V2.1`,
/// `POST /evsepull/v21/providers/{providerID}/status-records`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PullEvseStatusRequest {
    /// The EMP asking.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// Restrict to a circle.
    #[serde(rename = "SearchCenter", default, skip_serializing_if = "Option::is_none")]
    pub search_center: Option<SearchCenter>,
    /// Restrict to points in this state.
    #[serde(rename = "EvseStatus", default, skip_serializing_if = "Option::is_none")]
    pub evse_status: Option<EvseStatus>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for PullEvseStatusRequest {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(
            self,
            v,
            provider_id as "ProviderID",
            search_center as "SearchCenter",
            evse_status as "EvseStatus",
        );
    }
}

/// Asks Hubject for the status of specific charging points.
///
/// At most 100 ids per request — the one hard limit in OICP's pull family.
///
/// Spec: `eRoamingPullEvseStatusByID_V2.1`, sharing the `status-records` endpoint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PullEvseStatusByIdRequest {
    /// The EMP asking.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// The spots, at most 100.
    #[serde(rename = "EvseID")]
    pub evse_id: Vec<EvseId>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

/// The maximum number of ids one `PullEvseStatusByID` may carry.
pub const MAX_EVSE_IDS_PER_STATUS_REQUEST: usize = 100;

impl Validate for PullEvseStatusByIdRequest {
    fn validate_in(&self, v: &mut Validator) {
        if self.evse_id.is_empty() {
            v.report_at("EvseID", ViolationCode::EmptyRequiredList, "name at least one charging point");
        }
        if self.evse_id.len() > MAX_EVSE_IDS_PER_STATUS_REQUEST {
            v.report_at(
                "EvseID",
                ViolationCode::TooManyItems,
                format!(
                    "a PullEvseStatusByID carries at most {MAX_EVSE_IDS_PER_STATUS_REQUEST} ids, not {}",
                    self.evse_id.len()
                ),
            );
        }
        validate_fields!(self, v, provider_id as "ProviderID", evse_id as "EvseID");
    }
}

/// Asks Hubject for the status of every charging point of specific operators.
///
/// Spec: `eRoamingPullEvseStatusByOperatorID_V2.1`, sharing the `status-records` endpoint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PullEvseStatusByOperatorIdRequest {
    /// The EMP asking.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// The operators.
    #[serde(rename = "OperatorID")]
    pub operator_id: Vec<OperatorId>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for PullEvseStatusByOperatorIdRequest {
    fn validate_in(&self, v: &mut Validator) {
        if self.operator_id.is_empty() {
            v.report_at("OperatorID", ViolationCode::EmptyRequiredList, "name at least one operator");
        }
        validate_fields!(self, v, provider_id as "ProviderID", operator_id as "OperatorID");
    }
}

/// One operator's statuses, as they come back from a pull.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OperatorEvseStatusRecords {
    /// The operator.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// The operator's name.
    #[serde(rename = "OperatorName", default, skip_serializing_if = "Option::is_none")]
    pub operator_name: Option<String>,
    /// The statuses.
    #[serde(rename = "EvseStatusRecord")]
    pub evse_status_record: Vec<EvseStatusRecord>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Validate for OperatorEvseStatusRecords {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, operator_id as "OperatorID", evse_status_record as "EvseStatusRecord");
    }
}

/// The wrapper Hubject puts around the operators' status blocks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvseStatuses {
    /// One block per operator.
    #[serde(rename = "OperatorEvseStatus")]
    pub operator_evse_status: Vec<OperatorEvseStatusRecords>,
}

impl Validate for EvseStatuses {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, operator_evse_status as "OperatorEvseStatus");
    }
}

/// The answer to [`PullEvseStatusRequest`].
///
/// Note that this one is **not** paginated — unlike `PullEvseData`, the status pull returns
/// everything at once, grouped by operator.
///
/// Spec: `eRoamingEVSEStatus`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvseStatusResponse {
    /// The statuses, grouped by operator.
    #[serde(rename = "EvseStatuses")]
    pub evse_statuses: EvseStatuses,
    /// Whether the query itself succeeded.
    #[serde(rename = "StatusCode", default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<crate::types::StatusCode>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl EvseStatusResponse {
    /// Every status in the response, flattened across operators.
    pub fn records(&self) -> impl Iterator<Item = (&OperatorId, &EvseStatusRecord)> {
        self.evse_statuses
            .operator_evse_status
            .iter()
            .flat_map(|block| block.evse_status_record.iter().map(move |r| (&block.operator_id, r)))
    }
}

impl Validate for EvseStatusResponse {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, evse_statuses as "EvseStatuses", status_code as "StatusCode");
    }
}

/// The answer to [`PullEvseStatusByIdRequest`].
///
/// Spec: `eRoamingEVSEStatusByID`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvseStatusByIdResponse {
    /// The statuses.
    #[serde(rename = "EVSEStatusRecords")]
    pub evse_status_records: EvseStatusRecords,
    /// Whether the query itself succeeded.
    #[serde(rename = "StatusCode", default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<crate::types::StatusCode>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

/// The wrapper around a by-id status answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvseStatusRecords {
    /// The statuses.
    #[serde(rename = "EvseStatusRecord")]
    pub evse_status_record: Vec<EvseStatusRecord>,
}

impl Validate for EvseStatusRecords {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, evse_status_record as "EvseStatusRecord");
    }
}

impl Validate for EvseStatusByIdResponse {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, evse_status_records as "EVSEStatusRecords", status_code as "StatusCode");
    }
}

strict_builder!(PullEvseDataRecord, PullEvseDataRecordBuilder, pull_evse_data_record_builder);
strict_builder!(SearchCenter, SearchCenterBuilder, search_center_builder);
strict_builder!(PullEvseDataRequest, PullEvseDataRequestBuilder, pull_evse_data_request_builder);
strict_builder!(PullEvseStatusRequest, PullEvseStatusRequestBuilder, pull_evse_status_request_builder);
strict_builder!(
    PullEvseStatusByIdRequest,
    PullEvseStatusByIdRequestBuilder,
    pull_evse_status_by_id_request_builder
);
strict_builder!(
    PullEvseStatusByOperatorIdRequest,
    PullEvseStatusByOperatorIdRequestBuilder,
    pull_evse_status_by_operator_id_request_builder
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delta_pull_cannot_carry_the_geographic_filters() {
        let provider: ProviderId = "DE-DCB".parse().unwrap();
        let since: DateTime = "2020-09-23T14:27:43.052Z".parse().unwrap();

        let plain = PullEvseDataRequest::delta(provider.clone(), GeoCoordinatesFormat::Google, since.clone());
        assert!(plain.is_delta());
        assert!(plain.validate().is_ok());

        let with_countries = PullEvseDataRequest {
            country_codes: Some(vec![Text::new("DEU").unwrap()]),
            ..PullEvseDataRequest::delta(provider.clone(), GeoCoordinatesFormat::Google, since.clone())
        };
        let err = with_countries.validate().unwrap_err();
        assert_eq!(err.as_slice()[0].code, ViolationCode::ExclusiveChoice);
        assert!(err.as_slice()[0].message.contains("CountryCodes"));

        // The same filters are fine on a full pull.
        let full = PullEvseDataRequest {
            country_codes: Some(vec![Text::new("DEU").unwrap()]),
            operator_ids: Some(vec!["DE*ABC".parse().unwrap()]),
            ..PullEvseDataRequest::full(provider, GeoCoordinatesFormat::Google)
        };
        assert!(full.validate().is_ok());
    }

    #[test]
    fn every_conflicting_filter_is_named_at_once() {
        let request = PullEvseDataRequest {
            search_center: Some(SearchCenter {
                geo_coordinates: GeoCoordinates::Google { coordinates: "52.480495 13.356465".into() },
                radius: "10".parse().unwrap(),
            }),
            country_codes: Some(vec![Text::new("DEU").unwrap()]),
            operator_ids: Some(vec!["DE*ABC".parse().unwrap()]),
            ..PullEvseDataRequest::delta(
                "DE-DCB".parse().unwrap(),
                GeoCoordinatesFormat::Google,
                "2020-09-23T14:27:43.052Z".parse().unwrap(),
            )
        };
        assert_eq!(request.conflicting_filters(), vec!["SearchCenter", "CountryCodes", "OperatorIds"]);
    }

    #[test]
    fn the_hundred_id_limit_is_enforced() {
        let ids: Vec<EvseId> = (0..101).map(|i| format!("DE*ABC*E{i}").parse().unwrap()).collect();
        let request = PullEvseStatusByIdRequest {
            provider_id: "DE-DCB".parse().unwrap(),
            evse_id: ids,
            extensions: Extensions::new(),
        };
        assert!(request.validate().unwrap_err().iter().any(|x| x.code == ViolationCode::TooManyItems));
    }

    #[test]
    fn statuses_flatten_across_operators() {
        let json = r#"{
            "EvseStatuses": {"OperatorEvseStatus": [
                {"OperatorID":"DE*ABC","EvseStatusRecord":[{"EvseID":"DE*ABC*E1","EvseStatus":"Available"}]},
                {"OperatorID":"DE*XYZ","EvseStatusRecord":[{"EvseID":"DE*XYZ*E1","EvseStatus":"Occupied"}]}
            ]}
        }"#;
        let response: EvseStatusResponse = serde_json::from_str(json).unwrap();
        let all: Vec<_> = response.records().collect();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0.as_str(), "DE*ABC");
        assert!(all[0].1.evse_status.is_chargeable());
        assert!(!all[1].1.evse_status.is_chargeable());
    }
}
