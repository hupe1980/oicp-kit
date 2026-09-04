//! The charge detail record — the document a session is billed from.

use serde::{Deserialize, Serialize};

use crate::oicp_open_enum;
use crate::types::{
    DateTime, EvseId, Extensions, Identification, IdentificationProcess, Number, OperatorId,
    PartnerSessionId, ProviderId, SessionId, Text, Validate, Validator, ViolationCode, strict_builder,
    validate_fields,
};

oicp_open_enum! {
    /// Which point of a charging process a signed meter value was taken at.
    pub enum MeteringStatus {
        /// The beginning of the charging process.
        Start = "Start",
        /// An intermediate value.
        Progress = "Progress",
        /// The end of the charging process.
        End = "End",
    }
}

/// A meter reading signed by the meter itself, in transparency-software format.
///
/// German calibration law (Eichrecht) requires an EV driver to be able to verify, independently,
/// that the energy they were billed for is the energy the meter measured. The signature is opaque
/// to OICP — it is verified by transparency software, not by this crate — but it must survive the
/// journey from meter to invoice **byte for byte**, which is why it is a plain string here and why
/// nothing in this crate rewrites it.
///
/// The spec allows at most ten of these per CDR, and asks for them in order: `Start`, then up to
/// eight `Progress`, then `End`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct SignedMeteringValue {
    /// The signed value, as the meter produced it.
    #[serde(rename = "SignedMeteringValue", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub signed_metering_value: Option<Text<3000>>,
    /// Which point of the session it was taken at.
    #[serde(rename = "MeteringStatus", default, skip_serializing_if = "Option::is_none")]
    pub metering_status: Option<MeteringStatus>,
}

impl Validate for SignedMeteringValue {
    fn validate_in(&self, v: &mut Validator) {
        // The 3000-character cap is the *other half* of defect OICP23-D003, and a partner whose
        // charging point signs a reading every two minutes runs into it. Reporting it as a plain
        // over-long string invites exactly the fix that must not happen — truncation, which
        // destroys the signature and with it the driver's right to check the bill.
        if let Some(value) = &self.signed_metering_value {
            let len = value.len();
            if len > Text::<3000>::MAX {
                let note = crate::types::SpecDefect::get("OICP23-D003")
                    .map_or_else(String::new, |d| format!("; {}", d.note()));
                v.report_at(
                    "SignedMeteringValue",
                    ViolationCode::TooLong,
                    format!(
                        "the signed value is {len} characters and OICP 2.3 allows 3000{note}. Send \
                         it whole: a truncated signature verifies as tampered"
                    ),
                );
            }
        }
        v.field("MeteringStatus", &self.metering_status);
    }
}

/// What a driver needs to check a signed meter value for themselves.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct CalibrationLawVerificationInfo {
    /// The compliance id from the certifying authority, with revision and issue date.
    ///
    /// For example `PTB - X-X-XXXX : V1 : 01Jan2020`.
    #[serde(rename = "CalibrationLawCertificateID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub calibration_law_certificate_id: Option<Text<100>>,
    /// The public key for this EVSE's meter.
    #[serde(rename = "PublicKey", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub public_key: Option<Text<1000>>,
    /// A URL to an XML file with the compiled calibration-law data, for the driver's invoice.
    #[serde(rename = "MeteringSignatureUrl", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub metering_signature_url: Option<Text<200>>,
    /// The encoding of the signature data, with its version, e.g. `EDL40 Mennekes: V1`.
    #[serde(rename = "MeteringSignatureEncodingFormat", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub metering_signature_encoding_format: Option<Text<50>>,
    /// How to use the transparency software.
    #[serde(
        rename = "SignedMeteringValuesVerificationInstruction",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[builder(into)]
    pub signed_metering_values_verification_instruction: Option<Text<400>>,
}

impl Validate for CalibrationLawVerificationInfo {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(
            self,
            v,
            calibration_law_certificate_id as "CalibrationLawCertificateID",
            public_key as "PublicKey",
            metering_signature_url as "MeteringSignatureUrl",
            metering_signature_encoding_format as "MeteringSignatureEncodingFormat",
            signed_metering_values_verification_instruction as "SignedMeteringValuesVerificationInstruction",
        );
    }
}

/// Meter readings taken during a session.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct MeterValuesInBetween {
    /// The readings, in kWh.
    #[serde(rename = "meterValues", default, skip_serializing_if = "Option::is_none")]
    pub meter_values: Option<Vec<Number>>,
}

impl Validate for MeterValuesInBetween {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, meter_values as "meterValues");
    }
}

/// What happened during one charging session, and what it should be billed as.
///
/// # This is the money document
///
/// Everything else in OICP is operational; this is the record two companies settle against. Its
/// cross-field rules are therefore not decoration — they are the difference between an invoice
/// that reconciles and a dispute:
///
/// * `ConsumedEnergy` is *defined* as `MeterValueEnd - MeterValueStart`. Exactly — which is why
///   every number here is a [`Number`] and not an `f64`.
/// * The four timestamps have an order: session start, charging start, charging end, session end.
/// * `SignedMeteringValues` is **mandatory** when the EVSE reports
///   [`CalibrationLawDataAvailability::External`](crate::types::CalibrationLawDataAvailability),
///   and there may be at most ten of them.
///
/// [`Validate`] checks what is checkable from the record alone.
/// [`eichrecht::CdrCheck`](crate::eichrecht::CdrCheck) checks the rest — the rules that need the
/// EVSE's data record too — before the CDR is submitted, rather than weeks later in a dispute.
///
/// Spec: `eRoamingChargeDetailRecord_V2.2`,
/// `POST /cdrmgmt/v22/operators/{operatorID}/charge-detail-record`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct ChargeDetailRecord {
    /// The session this record settles.
    #[serde(rename = "SessionID")]
    pub session_id: SessionId,
    /// The CPO's own session id.
    #[serde(rename = "CPOPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub cpo_partner_session_id: Option<PartnerSessionId>,
    /// The EMP's own session id.
    #[serde(rename = "EMPPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub emp_partner_session_id: Option<PartnerSessionId>,
    /// The tariff product the session was billed under.
    #[serde(rename = "PartnerProductID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub partner_product_id: Option<Text<50>>,
    /// Where the session happened.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// Who charged.
    #[serde(rename = "Identification")]
    pub identification: Identification,
    /// When energy started flowing.
    #[serde(rename = "ChargingStart")]
    pub charging_start: DateTime,
    /// When energy stopped flowing.
    #[serde(rename = "ChargingEnd")]
    pub charging_end: DateTime,
    /// When the session started — the swipe of the card, or the cable going in.
    #[serde(rename = "SessionStart")]
    pub session_start: DateTime,
    /// When the session ended — the swipe of the card, or the cable coming out.
    #[serde(rename = "SessionEnd")]
    pub session_end: DateTime,
    /// The meter reading at the start, in kWh.
    #[serde(rename = "MeterValueStart", default, skip_serializing_if = "Option::is_none")]
    pub meter_value_start: Option<Number>,
    /// The meter reading at the end, in kWh.
    #[serde(rename = "MeterValueEnd", default, skip_serializing_if = "Option::is_none")]
    pub meter_value_end: Option<Number>,
    /// Readings taken in between, in kWh.
    #[serde(rename = "MeterValueInBetween", default, skip_serializing_if = "Option::is_none")]
    pub meter_value_in_between: Option<MeterValuesInBetween>,
    /// The energy delivered, in kWh: the difference between the end and start readings.
    #[serde(rename = "ConsumedEnergy")]
    pub consumed_energy: Number,
    /// The meter's own signed readings, for calibration-law verification.
    #[serde(rename = "SignedMeteringValues", default, skip_serializing_if = "Option::is_none")]
    pub signed_metering_values: Option<Vec<SignedMeteringValue>>,
    /// What a driver needs to verify those readings.
    #[serde(rename = "CalibrationLawVerificationInfo", default, skip_serializing_if = "Option::is_none")]
    pub calibration_law_verification_info: Option<CalibrationLawVerificationInfo>,
    /// The hub operator that bundles the CPO, if any.
    #[serde(rename = "HubOperatorID", default, skip_serializing_if = "Option::is_none")]
    pub hub_operator_id: Option<OperatorId>,
    /// The hub provider that bundles the EMP, if any.
    ///
    /// See erratum [`OICP23-E001`](crate::types::ERRATA): the EMP OpenAPI schema names this field
    /// `HubProviderId`, while both leading documents, every example and the CPO schema name it
    /// `HubProviderID`. This crate writes `HubProviderID` and accepts both.
    #[serde(
        rename = "HubProviderID",
        alias = "HubProviderId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub hub_provider_id: Option<ProviderId>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl ChargeDetailRecord {
    /// The energy the meter readings imply, if both are present.
    ///
    /// This is what [`consumed_energy`](Self::consumed_energy) is *defined* to be. Exact decimal
    /// arithmetic, so the comparison is meaningful.
    #[must_use]
    pub fn metered_energy(&self) -> Option<Number> {
        Some(self.meter_value_end? - self.meter_value_start?)
    }

    /// How long the session lasted, in seconds, or `None` if either timestamp is unreadable.
    #[must_use]
    pub fn session_duration_seconds(&self) -> Option<i64> {
        Some((self.session_end.as_offset()? - self.session_start.as_offset()?).whole_seconds())
    }

    /// How long energy flowed, in seconds, or `None` if either timestamp is unreadable.
    #[must_use]
    pub fn charging_duration_seconds(&self) -> Option<i64> {
        Some((self.charging_end.as_offset()? - self.charging_start.as_offset()?).whole_seconds())
    }
}

impl Validate for ChargeDetailRecord {
    fn validate_in(&self, v: &mut Validator) {
        v.enter("Identification");
        self.identification.validate_in_process(v, IdentificationProcess::Record);
        v.leave();

        // ConsumedEnergy is *defined* as MeterValueEnd - MeterValueStart. When both readings are
        // present and the arithmetic disagrees, one of the three numbers is wrong, and the EMP
        // will be invoiced for the one this field carries.
        if let Some(metered) = self.metered_energy()
            && metered != self.consumed_energy
        {
            v.report_at(
                "ConsumedEnergy",
                ViolationCode::Inconsistent,
                format!(
                    "ConsumedEnergy is {} but MeterValueEnd - MeterValueStart is {metered}; \
                         the spec defines the first as the second",
                    self.consumed_energy
                ),
            );
        }
        if self.consumed_energy.is_negative() {
            v.report_at(
                "ConsumedEnergy",
                ViolationCode::OutOfRange,
                "a session cannot consume negative energy",
            );
        }
        if let (Some(start), Some(end)) = (self.meter_value_start, self.meter_value_end)
            && end < start
        {
            v.report_at(
                "MeterValueEnd",
                ViolationCode::Inconsistent,
                format!("the meter reads {end} at the end but {start} at the start"),
            );
        }

        // The four timestamps have an order. Charging happens inside the session.
        let ordering: [(&str, &DateTime, &str, &DateTime); 3] = [
            ("SessionStart", &self.session_start, "ChargingStart", &self.charging_start),
            ("ChargingStart", &self.charging_start, "ChargingEnd", &self.charging_end),
            ("ChargingEnd", &self.charging_end, "SessionEnd", &self.session_end),
        ];
        for (before_name, before, after_name, after) in ordering {
            if before.is_well_formed() && after.is_well_formed() && before > after {
                v.report_at(
                    after_name,
                    ViolationCode::Inconsistent,
                    format!("{after_name} ({after}) is before {before_name} ({before})"),
                );
            }
        }

        // "In total you can provide maximum 10 metering signature values." A long session at a
        // charging point that signs a reading every two minutes exceeds that, and truncating the
        // list would destroy the driver's ability to verify the bill — see defect OICP23-D003.
        if let Some(values) = &self.signed_metering_values
            && values.len() > 10
        {
            let note = crate::types::SpecDefect::get("OICP23-D003")
                .map_or_else(String::new, |d| format!("; {}", d.note()));
            v.report_at(
                "SignedMeteringValues",
                ViolationCode::TooManyItems,
                format!("a CDR carries at most 10 signed metering values, not {}{note}", values.len()),
            );
        }

        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            partner_product_id as "PartnerProductID",
            evse_id as "EvseID",
            charging_start as "ChargingStart",
            charging_end as "ChargingEnd",
            session_start as "SessionStart",
            session_end as "SessionEnd",
            meter_value_start as "MeterValueStart",
            meter_value_end as "MeterValueEnd",
            meter_value_in_between as "MeterValueInBetween",
            consumed_energy as "ConsumedEnergy",
            signed_metering_values as "SignedMeteringValues",
            calibration_law_verification_info as "CalibrationLawVerificationInfo",
            hub_operator_id as "HubOperatorID",
            hub_provider_id as "HubProviderID",
        );
    }
}

strict_builder!(SignedMeteringValue, SignedMeteringValueBuilder, signed_metering_value_builder);
strict_builder!(
    CalibrationLawVerificationInfo,
    CalibrationLawVerificationInfoBuilder,
    calibration_law_verification_info_builder
);
strict_builder!(MeterValuesInBetween, MeterValuesInBetweenBuilder, meter_values_in_between_builder);
strict_builder!(ChargeDetailRecord, ChargeDetailRecordBuilder, charge_detail_record_builder);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RfidMifareFamilyIdentification, Uid};

    fn cdr() -> ChargeDetailRecord {
        ChargeDetailRecord {
            session_id: "f98efba4-02d8-4fa0-b810-9a9d50d2c527".parse().unwrap(),
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            partner_product_id: None,
            evse_id: "DE*XYZ*ETEST1".parse().unwrap(),
            identification: Identification::RfidMifareFamily(RfidMifareFamilyIdentification {
                uid: Uid::new("7568290FFF765F").unwrap(),
            }),
            session_start: "2020-09-23T14:00:00.000Z".parse().unwrap(),
            charging_start: "2020-09-23T14:05:00.000Z".parse().unwrap(),
            charging_end: "2020-09-23T15:05:00.000Z".parse().unwrap(),
            session_end: "2020-09-23T15:10:00.000Z".parse().unwrap(),
            meter_value_start: Some("0.1".parse().unwrap()),
            meter_value_end: Some("10.1".parse().unwrap()),
            meter_value_in_between: None,
            consumed_energy: "10.0".parse().unwrap(),
            signed_metering_values: None,
            calibration_law_verification_info: None,
            hub_operator_id: None,
            hub_provider_id: None,
            extensions: Extensions::new(),
        }
    }

    #[test]
    fn the_energy_identity_holds_exactly_where_a_float_would_drift() {
        // 10.1_f64 - 0.1_f64 == 10.000000000000002, which would fail this check spuriously.
        let record = cdr();
        assert_eq!(record.metered_energy().unwrap(), record.consumed_energy);
        assert!(record.validate().is_ok());
    }

    #[test]
    fn energy_that_disagrees_with_the_meter_is_reported() {
        let mut record = cdr();
        record.consumed_energy = "12.0".parse().unwrap();
        let err = record.validate().unwrap_err();
        assert_eq!(err.as_slice()[0].pointer, "/ConsumedEnergy");
        assert!(err.as_slice()[0].message.contains("10.0"));
    }

    #[test]
    fn timestamps_out_of_order_are_reported_individually() {
        let mut record = cdr();
        record.charging_start = "2020-09-23T13:00:00.000Z".parse().unwrap();
        let err = record.validate().unwrap_err();
        // Charging cannot start before the session does.
        assert!(err.iter().any(|x| x.pointer == "/ChargingStart"));

        let mut record = cdr();
        record.session_end = "2020-09-23T14:00:00.000Z".parse().unwrap();
        assert!(record.validate().unwrap_err().iter().any(|x| x.pointer == "/SessionEnd"));
    }

    #[test]
    fn a_running_meter_cannot_go_backwards() {
        let mut record = cdr();
        record.meter_value_end = Some("0.0".parse().unwrap());
        record.consumed_energy = "-0.1".parse().unwrap();
        let err = record.validate().unwrap_err();
        assert!(err.iter().any(|x| x.pointer == "/MeterValueEnd"));
        assert!(err.iter().any(|x| x.pointer == "/ConsumedEnergy"));
    }

    #[test]
    fn an_over_long_signed_value_names_the_defect_rather_than_inviting_a_truncation() {
        let mut record = cdr();
        record.signed_metering_values = Some(vec![SignedMeteringValue {
            signed_metering_value: Some(Text::new_unchecked("A".repeat(3001))),
            metering_status: Some(MeteringStatus::End),
        }]);
        let err = record.validate().unwrap_err();
        let reported = err
            .iter()
            .find(|x| x.pointer == "/SignedMeteringValues/0/SignedMeteringValue")
            .expect("the over-long value is reported");
        assert_eq!(reported.code, ViolationCode::TooLong);
        assert!(reported.message.contains("OICP23-D003"), "{}", reported.message);
        assert!(reported.message.contains("truncated"), "{}", reported.message);
    }

    #[test]
    fn at_most_ten_signed_metering_values() {
        let mut record = cdr();
        record.signed_metering_values = Some(
            (0..11)
                .map(|_| SignedMeteringValue {
                    signed_metering_value: Some(Text::new("AAAA").unwrap()),
                    metering_status: Some(MeteringStatus::Progress),
                })
                .collect(),
        );
        assert!(record.validate().unwrap_err().iter().any(|x| x.code == ViolationCode::TooManyItems));
    }

    #[test]
    fn hub_provider_id_decodes_under_both_spellings_and_writes_the_canonical_one() {
        // Erratum OICP23-E001.
        for key in ["HubProviderID", "HubProviderId"] {
            let mut value = serde_json::to_value(cdr()).unwrap();
            value.as_object_mut().unwrap().insert(key.to_owned(), serde_json::json!("DE-DCB"));
            let decoded: ChargeDetailRecord = serde_json::from_value(value).unwrap();
            assert_eq!(decoded.hub_provider_id.as_ref().unwrap().as_str(), "DE-DCB", "{key} was not read");
            let out = serde_json::to_value(&decoded).unwrap();
            assert!(out.get("HubProviderID").is_some(), "the canonical spelling is written");
            assert!(out.get("HubProviderId").is_none());
        }
    }

    #[test]
    fn durations_come_out_of_the_timestamps() {
        let record = cdr();
        assert_eq!(record.session_duration_seconds(), Some(70 * 60));
        assert_eq!(record.charging_duration_seconds(), Some(60 * 60));

        // An unreadable timestamp yields no duration, rather than one measured from 1970.
        let mut broken = cdr();
        broken.charging_end = DateTime::new_unchecked("23.09.2020 15:05");
        assert_eq!(broken.charging_duration_seconds(), None);
    }
}
