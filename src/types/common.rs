//! The domain types shared by the CPO and EMP halves of the protocol.

use serde::{Deserialize, Serialize};

use super::builder::strict_builder;
use super::datetime::HourMinute;
use super::extensions::Extensions;
use super::number::Number;
use super::text::Text;
use super::validate::{Validate, Validator, ViolationCode, validate_fields};
use crate::{oicp_enum, oicp_open_enum};

// --- enumerations -------------------------------------------------------------------------

oicp_open_enum! {
    /// How the charging station can be reached.
    pub enum Accessibility {
        /// EV driver can reach the charging point without paying a fee.
        FreePubliclyAccessible = "Free publicly accessible",
        /// EV driver needs permission, e.g. a campus or building complex.
        RestrictedAccess = "Restricted access",
        /// EV driver needs to pay a fee to reach the point, e.g. a payable parking garage.
        PayingPubliclyAccessible = "Paying publicly accessible",
        /// The station is for testing purposes; access may be restricted.
        TestStation = "Test Station",
    }
}

oicp_open_enum! {
    /// Where a charging point sits, physically.
    pub enum AccessibilityLocation {
        /// On the street.
        OnStreet = "OnStreet",
        /// Inside a parking lot.
        ParkingLot = "ParkingLot",
        /// Inside a parking garage.
        ParkingGarage = "ParkingGarage",
        /// Inside an underground parking garage.
        UndergroundParkingGarage = "UndergroundParkingGarage",
    }
}

oicp_open_enum! {
    /// How a driver may authenticate at a charging point.
    pub enum AuthenticationMode {
        /// NFC RFID Classic.
        NfcRfidClassic = "NFC RFID Classic",
        /// NFC RFID DESFire.
        NfcRfidDesFire = "NFC RFID DESFire",
        /// Plug & Charge, ISO/IEC 15118.
        PnC = "PnC",
        /// App, QR code or phone.
        Remote = "REMOTE",
        /// Remote use via direct payment, e.g. intercharge direct.
        DirectPayment = "Direct Payment",
        /// No authentication method required.
        NoAuthenticationRequired = "No Authentication Required",
    }
}

oicp_open_enum! {
    /// How a driver may pay.
    ///
    /// The spec notes that `No Payment` cannot be combined with any other option;
    /// [`Validate`] on the record checks that.
    pub enum PaymentOption {
        /// Free.
        NoPayment = "No Payment",
        /// Cash, card, SMS and the like.
        Direct = "Direct",
        /// A subscription contract.
        Contract = "Contract",
    }
}

oicp_open_enum! {
    /// The physical connector a charging facility offers.
    pub enum Plug {
        /// Small paddle inductive.
        SmallPaddleInductive = "Small Paddle Inductive",
        /// Large paddle inductive.
        LargePaddleInductive = "Large Paddle Inductive",
        /// AVCON connector.
        AvconConnector = "AVCON Connector",
        /// Tesla connector.
        TeslaConnector = "Tesla Connector",
        /// NEMA 5-20.
        Nema520 = "NEMA 5-20",
        /// CEE 7/5, Type E French standard.
        TypeEFrenchStandard = "Type E French Standard",
        /// CEE 7/4, Type F Schuko.
        TypeFSchuko = "Type F Schuko",
        /// BS 1363, Type G British standard.
        TypeGBritishStandard = "Type G British Standard",
        /// SEV 1011, Type J Swiss standard.
        TypeJSwissStandard = "Type J Swiss Standard",
        /// IEC 62196-1 type 1 / SAE J1772, cable attached.
        Type1ConnectorCableAttached = "Type 1 Connector (Cable Attached)",
        /// IEC 62196-1 type 2 outlet.
        Type2Outlet = "Type 2 Outlet",
        /// IEC 62196-1 type 2, cable attached.
        Type2ConnectorCableAttached = "Type 2 Connector (Cable Attached)",
        /// IEC 62196-1 type 3 outlet.
        Type3Outlet = "Type 3 Outlet",
        /// IEC 60309, single phase.
        Iec60309SinglePhase = "IEC 60309 Single Phase",
        /// IEC 60309, three phase.
        Iec60309ThreePhase = "IEC 60309 Three Phase",
        /// CCS Combo 2 (IEC 62196-3), cable attached.
        CcsCombo2PlugCableAttached = "CCS Combo 2 Plug (Cable Attached)",
        /// CCS Combo 1 (IEC 62196-3), cable attached.
        CcsCombo1PlugCableAttached = "CCS Combo 1 Plug (Cable Attached)",
        /// DC CHAdeMO connector.
        ChaDeMo = "CHAdeMO",
    }
}

oicp_open_enum! {
    /// Services a charging point offers beyond the energy itself.
    pub enum ValueAddedService {
        /// The spot can be reserved via remote services.
        Reservation = "Reservation",
        /// The EVSE supports dynamic pricing.
        DynamicPricing = "DynamicPricing",
        /// Dynamic status info on the parking area in front of the EVSE is available.
        ParkingSensors = "ParkingSensors",
        /// The EVSE offers dynamic maximum power charging.
        MaximumPowerCharging = "MaximumPowerCharging",
        /// Predictive charge point usage info is available.
        PredictiveChargePointUsage = "PredictiveChargePointUsage",
        /// The EVSE offers charging plans, e.g. as described in ISO 15118-2.
        ChargingPlans = "ChargingPlans",
        /// The charging station is under a roof.
        RoofProvided = "RoofProvided",
        /// No value-added services are available.
        None = "None",
    }
}

oicp_open_enum! {
    /// The kind of current a charging facility delivers.
    pub enum PowerType {
        /// Single-phase AC.
        Ac1Phase = "AC_1_PHASE",
        /// Three-phase AC.
        Ac3Phase = "AC_3_PHASE",
        /// DC.
        Dc = "DC",
    }
}

oicp_open_enum! {
    /// The IEC 61851-1 charging mode a facility supports.
    pub enum ChargingMode {
        /// Standard socket, no communication or additional safety features.
        Mode1 = "Mode_1",
        /// Standard socket, with communication and additional safety features.
        Mode2 = "Mode_2",
        /// Permanently connected AC supply equipment, with communication and safety features.
        Mode3 = "Mode_3",
        /// DC supply equipment, with high-level communication and safety features.
        Mode4 = "Mode_4",
        /// CHAdeMO specification.
        ChaDeMo = "CHAdeMO",
    }
}

oicp_open_enum! {
    /// The primary energy source a station draws on.
    pub enum EnergyType {
        /// Solar radiation.
        Solar = "Solar",
        /// Wind.
        Wind = "Wind",
        /// Movement of water.
        HydroPower = "HydroPower",
        /// The sub-surface of the earth.
        GeothermalEnergy = "GeothermalEnergy",
        /// Plant or animal material used as fuel.
        Biomass = "Biomass",
        /// Coal.
        Coal = "Coal",
        /// Nuclear fission.
        NuclearEnergy = "NuclearEnergy",
        /// Petroleum.
        Petroleum = "Petroleum",
        /// Natural gas.
        NaturalGas = "NaturalGas",
    }
}

oicp_open_enum! {
    /// Whether the charging point can supply German calibration-law (Eichrecht) data.
    ///
    /// `External` is the value that makes `SignedMeteringValues` mandatory on every CDR from this
    /// EVSE — [`CdrCheck`](crate::eichrecht::CdrCheck) enforces exactly that.
    pub enum CalibrationLawDataAvailability {
        /// Calibration law data is shown at the charging station.
        Local = "Local",
        /// Calibration law data is provided externally, in the CDR.
        External = "External",
        /// Calibration law data is not provided.
        NotAvailable = "Not Available",
    }
}

oicp_open_enum! {
    /// Which days a period applies on.
    pub enum DaySelection {
        /// Every day.
        Everyday = "Everyday",
        /// Monday to Friday.
        Workdays = "Workdays",
        /// Saturday and Sunday.
        Weekend = "Weekend",
        /// Monday.
        Monday = "Monday",
        /// Tuesday.
        Tuesday = "Tuesday",
        /// Wednesday.
        Wednesday = "Wednesday",
        /// Thursday.
        Thursday = "Thursday",
        /// Friday.
        Friday = "Friday",
        /// Saturday.
        Saturday = "Saturday",
        /// Sunday.
        Sunday = "Sunday",
    }
}

oicp_open_enum! {
    /// Whether a CPO also publishes dynamic EVSE status for a record.
    ///
    /// Modelled as an enum rather than a `bool` because the wire type is the *string* `"true"`,
    /// `"false"` or `"auto"` — a JSON boolean here is a decode failure against a strict peer.
    pub enum DynamicInfoAvailable {
        /// The CPO publishes EVSE status for this record.
        True = "true",
        /// The CPO does not.
        False = "false",
        /// Hubject sets this to true if the operator offers EVSE status data.
        Auto = "auto",
    }
}

oicp_enum! {
    /// What Hubject should do with the data in a push.
    ///
    /// # The most dangerous field in OICP
    ///
    /// `fullLoad` **replaces** everything Hubject holds for the operator. A CPO that sends a
    /// partial list with `fullLoad` deletes the rest of its own fleet from the roaming network,
    /// and every EMP stops seeing those charge points on the next pull.
    ///
    /// This is one of the two enums in the crate that is *closed* rather than open: an
    /// unrecognised action has no safe default, and guessing `fullLoad` would be catastrophic.
    /// The client does not take this as an argument either — see
    /// [`PushPlanner`](crate::sync::PushPlanner), which computes the minimal `insert`/`update`/
    /// `delete` set, and keeps `fullLoad` behind a separately named call.
    pub enum ActionType {
        /// Replace everything Hubject holds for this operator with the payload. Destructive.
        FullLoad = "fullLoad",
        /// Update the records in the payload, leaving the rest alone.
        Update = "update",
        /// Add the records in the payload.
        Insert = "insert",
        /// Remove the records in the payload.
        Delete = "delete",
    }
}

impl ActionType {
    /// Whether this action replaces the operator's entire data set.
    ///
    /// The one call site that matters: anything that logs, confirms or guards a push should ask
    /// this rather than comparing against a variant.
    #[must_use]
    pub const fn is_destructive_replace(self) -> bool {
        matches!(self, Self::FullLoad)
    }
}

oicp_open_enum! {
    /// The unit a price refers to.
    pub enum ReferenceUnit {
        /// Per hour.
        Hour = "HOUR",
        /// Per kilowatt-hour.
        KilowattHour = "KILOWATT_HOUR",
        /// Per minute.
        Minute = "MINUTE",
    }
}

// --- structures ---------------------------------------------------------------------------

/// A postal address, per ISO 19773.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct Address {
    /// The alpha-3 country code, e.g. `DEU`. OICP 2.2 and 2.3 allow **only** alpha-3.
    #[serde(rename = "Country")]
    #[builder(into)]
    pub country: Text<3>,
    /// The city.
    #[serde(rename = "City")]
    #[builder(into)]
    pub city: Text<50>,
    /// The street.
    #[serde(rename = "Street")]
    #[builder(into)]
    pub street: Text<100>,
    /// The postal code.
    #[serde(rename = "PostalCode")]
    #[builder(into)]
    pub postal_code: Text<10>,
    /// The house number.
    #[serde(rename = "HouseNum")]
    #[builder(into)]
    pub house_num: Text<10>,
    /// The floor.
    #[serde(rename = "Floor", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub floor: Option<Text<5>>,
    /// The region.
    #[serde(rename = "Region", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub region: Option<Text<50>>,
    /// Whether a parking facility is available.
    #[serde(rename = "ParkingFacility", default, skip_serializing_if = "Option::is_none")]
    pub parking_facility: Option<bool>,
    /// The parking spot.
    #[serde(rename = "ParkingSpot", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub parking_spot: Option<Text<5>>,
    /// The time zone as a fixed UTC offset, e.g. `UTC+01:00`.
    #[serde(rename = "TimeZone", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub time_zone: Option<String>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Address {
    /// Whether [`time_zone`](Self::time_zone) matches `[U][T][C][+,-][0-9][0-9][:][0-9][0-9]`.
    #[must_use]
    pub fn time_zone_is_well_formed(&self) -> bool {
        let Some(tz) = &self.time_zone else { return true };
        let b = tz.as_bytes();
        b.len() == 9
            && &b[..3] == b"UTC"
            && (b[3] == b'+' || b[3] == b'-')
            && b[4].is_ascii_digit()
            && b[5].is_ascii_digit()
            && b[6] == b':'
            && b[7].is_ascii_digit()
            && b[8].is_ascii_digit()
    }
}

impl Validate for Address {
    fn validate_in(&self, v: &mut Validator) {
        if self.country.len() != 3 || !self.country.as_str().bytes().all(|c| c.is_ascii_alphabetic()) {
            v.report_at(
                "Country",
                ViolationCode::PatternMismatch,
                format!(
                    "{:?} is not an alpha-3 country code; OICP 2.2 and 2.3 allow only alpha-3 (ISO 3166-1)",
                    self.country.as_str()
                ),
            );
        }
        if self.city.is_empty() {
            v.report_at(
                "City",
                ViolationCode::TooShort,
                "the city is required and has a minimum length of 1",
            );
        }
        if self.street.len() < 2 && !self.street.is_empty() {
            v.report_at("Street", ViolationCode::TooShort, "the street has a minimum length of 2");
        }
        if !self.time_zone_is_well_formed() {
            v.report_at(
                "TimeZone",
                ViolationCode::PatternMismatch,
                format!(
                    "{:?} is not of the form UTC+HH:MM or UTC-HH:MM",
                    self.time_zone.as_deref().unwrap_or("")
                ),
            );
        }
        validate_fields!(
            self,
            v,
            country as "Country",
            city as "City",
            street as "Street",
            postal_code as "PostalCode",
            house_num as "HouseNum",
            floor as "Floor",
            region as "Region",
            parking_spot as "ParkingSpot",
        );
    }
}

/// What a charging point can physically deliver.
///
/// See erratum [`OICP23-E003`](super::errata::ERRATA): the leading document types `Power` as an
/// integer of at most three digits, while the EMP OpenAPI schema types it as an unconstrained
/// number. This crate decodes it as an exact decimal — so a real-world `22.5` arrives — and
/// [`Validate`] reports the deviation rather than refusing the record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct ChargingFacility {
    /// AC single phase, AC three phase, or DC.
    #[serde(rename = "PowerType")]
    pub power_type: PowerType,
    /// Voltage line to neutral, in volts.
    #[serde(rename = "Voltage", default, skip_serializing_if = "Option::is_none")]
    pub voltage: Option<Number>,
    /// Amperage, in amperes.
    #[serde(rename = "Amperage", default, skip_serializing_if = "Option::is_none")]
    pub amperage: Option<Number>,
    /// Power, in kW.
    #[serde(rename = "Power")]
    pub power: Number,
    /// The IEC 61851-1 modes supported.
    #[serde(rename = "ChargingModes", default, skip_serializing_if = "Option::is_none")]
    pub charging_modes: Option<Vec<ChargingMode>>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for ChargingFacility {
    fn validate_in(&self, v: &mut Validator) {
        v.enter("Power");
        self.power.validate_in(v);
        self.power.validate_range(v, 0, 999);
        if !self.power.is_integer() {
            v.report(
                ViolationCode::Inconsistent,
                format!(
                    "the leading OICP 2.3 document types Power as an Integer of at most three digits, \
                     but this facility reports {}; see erratum OICP23-E003",
                    self.power
                ),
            );
        }
        v.leave();
        // The voltage and amperage caps are narrower than real DC hardware — an 800 V vehicle
        // charges at ~920 V, a 350 kW charger draws ~500 A. The violation is real, because Hubject
        // validates against the specification, but the *specification* is what is wrong, and the
        // message says so rather than leaving a CPO to conclude this crate is broken.
        if let Some(voltage) = self.voltage {
            v.enter("Voltage");
            voltage.validate_range_with_defect(v, 0, 999, "OICP23-D002");
            v.leave();
        }
        if let Some(amperage) = self.amperage {
            v.enter("Amperage");
            amperage.validate_range_with_defect(v, 0, 99, "OICP23-D001");
            v.leave();
        }
        validate_fields!(self, v, power_type as "PowerType", charging_modes as "ChargingModes");
    }
}

/// Where a station's electricity comes from, and in what proportion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct EnergySource {
    /// The source.
    #[serde(rename = "Energy", default, skip_serializing_if = "Option::is_none")]
    pub energy: Option<EnergyType>,
    /// What share of the mix it accounts for, 0 to 99.
    #[serde(rename = "Percentage", default, skip_serializing_if = "Option::is_none")]
    pub percentage: Option<Number>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for EnergySource {
    fn validate_in(&self, v: &mut Validator) {
        if let Some(pct) = self.percentage {
            v.enter("Percentage");
            pct.validate_range(v, 0, 99);
            v.leave();
        }
        validate_fields!(self, v, energy as "Energy");
    }
}

/// The environmental cost of a station's energy mix.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct EnvironmentalImpact {
    /// Total CO₂ emitted, in g/kWh.
    #[serde(rename = "CO2Emission", default, skip_serializing_if = "Option::is_none")]
    pub co2_emission: Option<Number>,
    /// Total nuclear waste produced, in g/kWh.
    #[serde(rename = "NuclearWaste", default, skip_serializing_if = "Option::is_none")]
    pub nuclear_waste: Option<Number>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for EnvironmentalImpact {
    fn validate_in(&self, v: &mut Validator) {
        for (name, value) in [("CO2Emission", self.co2_emission), ("NuclearWaste", self.nuclear_waste)] {
            if let Some(value) = value {
                v.enter(name);
                value.validate_range(v, 0, 99_999);
                v.leave();
            }
        }
    }
}

/// A window of time within a day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct Period {
    /// When the window opens.
    #[serde(rename = "begin")]
    pub begin: HourMinute,
    /// When it closes.
    #[serde(rename = "end")]
    pub end: HourMinute,
}

impl Validate for Period {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, begin as "begin", end as "end");
        if let (Some(b), Some(e)) = (self.begin.minutes_of_day(), self.end.minutes_of_day()) {
            // An overnight window (22:00–02:00) is legitimate, so only an exactly-equal pair is
            // reported: a zero-length window is never what anyone meant.
            if b == e {
                v.report(ViolationCode::Inconsistent, "the period begins and ends at the same time");
            }
        }
    }
}

/// When a charging station is open, if not around the clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct OpeningTimes {
    /// The windows within the day.
    #[serde(rename = "Period")]
    pub period: Vec<Period>,
    /// Which days the windows apply on.
    #[serde(rename = "on")]
    pub on: DaySelection,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for OpeningTimes {
    fn validate_in(&self, v: &mut Validator) {
        if self.period.is_empty() {
            v.report_at("Period", ViolationCode::EmptyRequiredList, "opening times need at least one period");
        }
        validate_fields!(self, v, period as "Period", on as "on");
    }
}

// Every builder's `build()` validates; `build_unchecked()` is the escape hatch.
strict_builder!(Address, AddressBuilder, address_builder);
strict_builder!(ChargingFacility, ChargingFacilityBuilder, charging_facility_builder);
strict_builder!(EnergySource, EnergySourceBuilder, energy_source_builder);
strict_builder!(EnvironmentalImpact, EnvironmentalImpactBuilder, environmental_impact_builder);
strict_builder!(Period, PeriodBuilder, period_builder);
strict_builder!(OpeningTimes, OpeningTimesBuilder, opening_times_builder);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_type_is_closed_because_guessing_would_delete_a_fleet() {
        assert!(serde_json::from_str::<ActionType>(r#""replace""#).is_err());
        assert!(ActionType::FullLoad.is_destructive_replace());
        assert!(!ActionType::Update.is_destructive_replace());
    }

    #[test]
    fn dynamic_info_available_is_a_string_enum_not_a_bool() {
        let value: DynamicInfoAvailable = serde_json::from_str(r#""auto""#).unwrap();
        assert_eq!(value, DynamicInfoAvailable::Auto);
        assert_eq!(serde_json::to_string(&DynamicInfoAvailable::True).unwrap(), r#""true""#);
    }

    #[test]
    fn a_plug_hubject_adds_later_is_kept() {
        let plug: Plug = serde_json::from_str(r#""MCS""#).unwrap();
        assert!(!plug.is_known());
        assert_eq!(serde_json::to_string(&plug).unwrap(), r#""MCS""#);
    }

    #[test]
    fn charging_facility_accepts_a_fractional_power_and_reports_it() {
        let json = r#"{"PowerType":"AC_3_PHASE","Power":22.5}"#;
        let facility: ChargingFacility = serde_json::from_str(json).unwrap();
        assert_eq!(facility.power.to_string(), "22.5");
        // Accepted, round-trips, and flagged against the leading document.
        assert_eq!(serde_json::to_string(&facility).unwrap(), json);
        let err = facility.validate().unwrap_err();
        assert_eq!(err.as_slice()[0].code, ViolationCode::Inconsistent);
        assert!(err.as_slice()[0].message.contains("OICP23-E003"));
    }

    #[test]
    fn an_integral_power_within_range_is_clean() {
        let facility: ChargingFacility =
            serde_json::from_str(r#"{"PowerType":"AC_3_PHASE","Power":22,"Voltage":480,"Amperage":32}"#)
                .unwrap();
        assert!(facility.validate().is_ok());
    }

    #[test]
    fn out_of_range_power_is_reported() {
        let facility: ChargingFacility = serde_json::from_str(r#"{"PowerType":"DC","Power":1500}"#).unwrap();
        let err = facility.validate().unwrap_err();
        assert!(err.iter().any(|x| x.code == ViolationCode::OutOfRange));
    }

    #[test]
    fn addresses_require_an_alpha_3_country_code() {
        let address = Address::builder()
            .country("DEU")
            .city("Berlin")
            .street("EUREF CAMPUS")
            .postal_code("10829")
            .house_num("22")
            .time_zone("UTC+01:00")
            .build()
            .expect("a conformant address builds");
        assert!(address.validate().is_ok());

        // Constructing strictly: the builder refuses an alpha-2 code outright.
        let err = Address::builder()
            .country("DE")
            .city("Berlin")
            .street("EUREF CAMPUS")
            .postal_code("10829")
            .house_num("22")
            .build()
            .unwrap_err();
        assert_eq!(err.as_slice()[0].pointer, "/Country");
    }

    #[test]
    fn a_malformed_time_zone_is_reported() {
        let err = Address::builder()
            .country("DEU")
            .city("Berlin")
            .street("Somewhere")
            .postal_code("10829")
            .house_num("22")
            .time_zone("Europe/Berlin")
            .build()
            .unwrap_err();
        assert_eq!(err.as_slice()[0].pointer, "/TimeZone");
    }

    #[test]
    fn build_unchecked_lets_a_nonconformant_object_exist_for_tests() {
        let address = Address::builder()
            .country("DE")
            .city("Berlin")
            .street("Somewhere")
            .postal_code("10829")
            .house_num("22")
            .build_unchecked();
        assert!(address.validate().is_err());
    }
}
