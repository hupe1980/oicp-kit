//! Charging notifications — telling the EMP what is happening while it happens.

use serde::{Deserialize, Serialize};

use super::cdr::MeterValuesInBetween;
use crate::types::{
    DateTime, EvseId, Extensions, Identification, Number, OperatorId, PartnerSessionId, SessionId, Text,
    Validate, Validator, ViolationCode, strict_builder, validate_fields,
};
use crate::{oicp_enum, oicp_open_enum};

oicp_enum! {
    /// Which of the four charging notifications this is.
    ///
    /// The discriminator on the shared `charging-notifications` endpoint. Closed, because it is
    /// what selects the shape of the rest of the body — an unrecognised value leaves nothing to
    /// decode.
    pub enum ChargingNotificationType {
        /// Energy has started flowing.
        Start = "Start",
        /// The session is running; here is how far it has got.
        Progress = "Progress",
        /// Energy has stopped flowing, but the session is still open.
        End = "End",
        /// Something went wrong.
        Error = "Error",
    }
}

oicp_open_enum! {
    /// What went wrong at the charging point.
    pub enum ErrorType {
        /// The process cannot start or stop; the driver should check the plug.
        ConnectorError = "ConnectorError",
        /// The process stopped abruptly and the station needs a physical check.
        CriticalError = "CriticalError",
    }
}

/// Energy has started flowing.
///
/// The spec is careful about why this exists: authorization and plugging in *do not* mean the car
/// is charging. Without this notification an EMP cannot tell a driver whether their car is
/// actually taking energy.
///
/// Spec: `eRoamingChargingNotifications Start`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct ChargingNotificationStart {
    /// Always [`ChargingNotificationType::Start`].
    #[serde(rename = "Type")]
    pub notification_type: ChargingNotificationType,
    /// The session.
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
    /// Who is charging.
    #[serde(rename = "Identification", default, skip_serializing_if = "Option::is_none")]
    pub identification: Option<Identification>,
    /// Where.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// When energy started flowing.
    #[serde(rename = "ChargingStart")]
    pub charging_start: DateTime,
    /// When the session started.
    #[serde(rename = "SessionStart", default, skip_serializing_if = "Option::is_none")]
    pub session_start: Option<DateTime>,
    /// The meter reading at the start, in kWh.
    #[serde(rename = "MeterValueStart", default, skip_serializing_if = "Option::is_none")]
    pub meter_value_start: Option<Number>,
    /// The operator.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// The tariff product.
    #[serde(rename = "PartnerProductID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub partner_product_id: Option<Text<50>>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for ChargingNotificationStart {
    fn validate_in(&self, v: &mut Validator) {
        check_type(v, self.notification_type, ChargingNotificationType::Start);
        if let (Some(session_start), true) = (&self.session_start, self.charging_start.is_well_formed()) {
            if session_start.is_well_formed() && session_start > &self.charging_start {
                v.report_at(
                    "ChargingStart",
                    ViolationCode::Inconsistent,
                    "energy started flowing before the session began",
                );
            }
        }
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            identification as "Identification",
            evse_id as "EvseID",
            charging_start as "ChargingStart",
            session_start as "SessionStart",
            meter_value_start as "MeterValueStart",
            operator_id as "OperatorID",
            partner_product_id as "PartnerProductID",
        );
    }
}

/// How far the session has got.
///
/// The spec asks for either [`charging_duration`](Self::charging_duration) or
/// [`consumed_energy_progress`](Self::consumed_energy_progress) — both may be sent, but neither
/// alone makes a useful notification, and neither at all makes an empty one.
///
/// See erratum [`OICP23-E006`](crate::types::ERRATA): the EMP document defines the duration
/// self-referentially. The CPO document's definition is the implementable one:
/// `ChargingDuration = EventOccurred - ChargingStart`, in milliseconds.
///
/// Spec: `eRoamingChargingNotifications Progress`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct ChargingNotificationProgress {
    /// Always [`ChargingNotificationType::Progress`].
    #[serde(rename = "Type")]
    pub notification_type: ChargingNotificationType,
    /// The session.
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
    /// Who is charging.
    #[serde(rename = "Identification", default, skip_serializing_if = "Option::is_none")]
    pub identification: Option<Identification>,
    /// Where.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// When energy started flowing.
    #[serde(rename = "ChargingStart")]
    pub charging_start: DateTime,
    /// When these figures were taken.
    #[serde(rename = "EventOccurred")]
    pub event_occurred: DateTime,
    /// `EventOccurred - ChargingStart`, in milliseconds.
    #[serde(rename = "ChargingDuration", default, skip_serializing_if = "Option::is_none")]
    pub charging_duration: Option<i64>,
    /// When the session started.
    #[serde(rename = "SessionStart", default, skip_serializing_if = "Option::is_none")]
    pub session_start: Option<DateTime>,
    /// Energy delivered since the start of charging, in kWh.
    #[serde(rename = "ConsumedEnergyProgress", default, skip_serializing_if = "Option::is_none")]
    pub consumed_energy_progress: Option<Number>,
    /// The meter reading at the start, in kWh.
    #[serde(rename = "MeterValueStart", default, skip_serializing_if = "Option::is_none")]
    pub meter_value_start: Option<Number>,
    /// Readings taken in between, in kWh.
    #[serde(rename = "MeterValueInBetween", default, skip_serializing_if = "Option::is_none")]
    pub meter_value_in_between: Option<MeterValuesInBetween>,
    /// The operator.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// The tariff product.
    #[serde(rename = "PartnerProductID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub partner_product_id: Option<Text<50>>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl ChargingNotificationProgress {
    /// The duration the timestamps imply, in milliseconds.
    ///
    /// The definition from the CPO document — see erratum `OICP23-E006`.
    #[must_use]
    pub fn implied_duration_ms(&self) -> Option<i64> {
        (self.event_occurred.as_offset()? - self.charging_start.as_offset()?)
            .whole_milliseconds()
            .try_into()
            .ok()
    }
}

impl Validate for ChargingNotificationProgress {
    fn validate_in(&self, v: &mut Validator) {
        check_type(v, self.notification_type, ChargingNotificationType::Progress);
        if self.charging_duration.is_none() && self.consumed_energy_progress.is_none() {
            v.report(
                ViolationCode::MissingConditional,
                "a progress notification carries at least one of ChargingDuration or \
                 ConsumedEnergyProgress; with neither it tells the driver nothing",
            );
        }
        if let (Some(stated), Some(implied)) = (self.charging_duration, self.implied_duration_ms()) {
            // A minute of slack: the CPO's clock and the notification's timestamps are not the
            // same instrument. Beyond that, one of the two is wrong.
            if (stated - implied).abs() > 60_000 {
                v.report_at(
                    "ChargingDuration",
                    ViolationCode::Inconsistent,
                    format!(
                        "ChargingDuration is {stated} ms, but EventOccurred - ChargingStart is {implied} ms"
                    ),
                );
            }
        }
        if self.charging_duration.is_some_and(|d| d < 0) {
            v.report_at("ChargingDuration", ViolationCode::OutOfRange, "a duration cannot be negative");
        }
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            identification as "Identification",
            evse_id as "EvseID",
            charging_start as "ChargingStart",
            event_occurred as "EventOccurred",
            session_start as "SessionStart",
            consumed_energy_progress as "ConsumedEnergyProgress",
            meter_value_start as "MeterValueStart",
            meter_value_in_between as "MeterValueInBetween",
            operator_id as "OperatorID",
            partner_product_id as "PartnerProductID",
        );
    }
}

/// Energy has stopped flowing, but the car is still plugged in.
///
/// The distinction matters commercially: this is the moment a blocking fee may start, which is
/// what [`penalty_time_start`](Self::penalty_time_start) records.
///
/// Spec: `eRoamingChargingNotifications End`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct ChargingNotificationEnd {
    /// Always [`ChargingNotificationType::End`].
    #[serde(rename = "Type")]
    pub notification_type: ChargingNotificationType,
    /// The session.
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
    /// Who charged.
    #[serde(rename = "Identification", default, skip_serializing_if = "Option::is_none")]
    pub identification: Option<Identification>,
    /// Where.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// When energy started flowing.
    #[serde(rename = "ChargingStart", default, skip_serializing_if = "Option::is_none")]
    pub charging_start: Option<DateTime>,
    /// When energy stopped flowing.
    #[serde(rename = "ChargingEnd")]
    pub charging_end: DateTime,
    /// When the session started.
    #[serde(rename = "SessionStart", default, skip_serializing_if = "Option::is_none")]
    pub session_start: Option<DateTime>,
    /// When the session ended.
    #[serde(rename = "SessionEnd", default, skip_serializing_if = "Option::is_none")]
    pub session_end: Option<DateTime>,
    /// Energy delivered, in kWh.
    #[serde(rename = "ConsumedEnergy", default, skip_serializing_if = "Option::is_none")]
    pub consumed_energy: Option<Number>,
    /// The meter reading at the start, in kWh.
    #[serde(rename = "MeterValueStart", default, skip_serializing_if = "Option::is_none")]
    pub meter_value_start: Option<Number>,
    /// The meter reading at the end, in kWh.
    #[serde(rename = "MeterValueEnd", default, skip_serializing_if = "Option::is_none")]
    pub meter_value_end: Option<Number>,
    /// Readings taken in between, in kWh.
    #[serde(rename = "MeterValueInBetween", default, skip_serializing_if = "Option::is_none")]
    pub meter_value_in_between: Option<MeterValuesInBetween>,
    /// The operator.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// The tariff product.
    #[serde(rename = "PartnerProductID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub partner_product_id: Option<Text<50>>,
    /// When a blocking fee starts, after the grace period.
    #[serde(rename = "PenaltyTimeStart", default, skip_serializing_if = "Option::is_none")]
    pub penalty_time_start: Option<DateTime>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for ChargingNotificationEnd {
    fn validate_in(&self, v: &mut Validator) {
        check_type(v, self.notification_type, ChargingNotificationType::End);
        if let (Some(start), Some(end), Some(consumed)) =
            (self.meter_value_start, self.meter_value_end, self.consumed_energy)
        {
            if end - start != consumed {
                v.report_at(
                    "ConsumedEnergy",
                    ViolationCode::Inconsistent,
                    format!("ConsumedEnergy is {consumed} but the meter readings differ by {}", end - start),
                );
            }
        }
        if let (Some(charging_start), true) = (&self.charging_start, self.charging_end.is_well_formed()) {
            if charging_start.is_well_formed() && charging_start > &self.charging_end {
                v.report_at("ChargingEnd", ViolationCode::Inconsistent, "charging ended before it started");
            }
        }
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            identification as "Identification",
            evse_id as "EvseID",
            charging_start as "ChargingStart",
            charging_end as "ChargingEnd",
            session_start as "SessionStart",
            session_end as "SessionEnd",
            consumed_energy as "ConsumedEnergy",
            meter_value_start as "MeterValueStart",
            meter_value_end as "MeterValueEnd",
            meter_value_in_between as "MeterValueInBetween",
            operator_id as "OperatorID",
            partner_product_id as "PartnerProductID",
            penalty_time_start as "PenaltyTimeStart",
        );
    }
}

/// Something went wrong at the charging point.
///
/// Spec: `eRoamingChargingNotifications Error`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct ChargingNotificationError {
    /// Always [`ChargingNotificationType::Error`].
    #[serde(rename = "Type")]
    pub notification_type: ChargingNotificationType,
    /// The session.
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
    /// Who was charging.
    #[serde(rename = "Identification", default, skip_serializing_if = "Option::is_none")]
    pub identification: Option<Identification>,
    /// The operator.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// Where.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// What kind of problem it is.
    #[serde(rename = "ErrorType")]
    pub error_type: ErrorType,
    /// What the CPO knows about it.
    #[serde(rename = "ErrorAdditionalInfo", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub error_additional_info: Option<Text<250>>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for ChargingNotificationError {
    fn validate_in(&self, v: &mut Validator) {
        check_type(v, self.notification_type, ChargingNotificationType::Error);
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            identification as "Identification",
            operator_id as "OperatorID",
            evse_id as "EvseID",
            error_type as "ErrorType",
            error_additional_info as "ErrorAdditionalInfo",
        );
    }
}

fn check_type(v: &mut Validator, actual: ChargingNotificationType, expected: ChargingNotificationType) {
    if actual != expected {
        v.report_at(
            "Type",
            ViolationCode::Inconsistent,
            format!("this is a {expected} notification, but Type says {actual}"),
        );
    }
}

/// Any of the four charging notifications.
///
/// All four go to the same endpoint, `POST /notificationmgmt/v11/charging-notifications`, and are
/// told apart by their `Type` field. This is that discriminated union, so a server handler can
/// take one argument and `match`.
///
/// The dispatch is written by hand rather than with `#[serde(tag = "Type")]`, because `Type` is a
/// real field of each payload as well as the discriminator: serde's internally-tagged
/// representation consumes the tag, and the round trip would then lose it.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ChargingNotification {
    /// Energy has started flowing.
    Start(ChargingNotificationStart),
    /// The session is running.
    Progress(ChargingNotificationProgress),
    /// Energy has stopped flowing.
    End(ChargingNotificationEnd),
    /// Something went wrong.
    Error(ChargingNotificationError),
}

impl ChargingNotification {
    /// Which notification this is.
    #[must_use]
    pub const fn notification_type(&self) -> ChargingNotificationType {
        match self {
            Self::Start(_) => ChargingNotificationType::Start,
            Self::Progress(_) => ChargingNotificationType::Progress,
            Self::End(_) => ChargingNotificationType::End,
            Self::Error(_) => ChargingNotificationType::Error,
        }
    }

    /// The session this notification is about.
    #[must_use]
    pub fn session_id(&self) -> &SessionId {
        match self {
            Self::Start(n) => &n.session_id,
            Self::Progress(n) => &n.session_id,
            Self::End(n) => &n.session_id,
            Self::Error(n) => &n.session_id,
        }
    }

    /// The charging spot this notification is about.
    #[must_use]
    pub fn evse_id(&self) -> &EvseId {
        match self {
            Self::Start(n) => &n.evse_id,
            Self::Progress(n) => &n.evse_id,
            Self::End(n) => &n.evse_id,
            Self::Error(n) => &n.evse_id,
        }
    }

    /// The operator that sent it.
    #[must_use]
    pub fn operator_id(&self) -> &OperatorId {
        match self {
            Self::Start(n) => &n.operator_id,
            Self::Progress(n) => &n.operator_id,
            Self::End(n) => &n.operator_id,
            Self::Error(n) => &n.operator_id,
        }
    }
}

impl Validate for ChargingNotification {
    fn validate_in(&self, v: &mut Validator) {
        match self {
            Self::Start(n) => n.validate_in(v),
            Self::Progress(n) => n.validate_in(v),
            Self::End(n) => n.validate_in(v),
            Self::Error(n) => n.validate_in(v),
        }
    }
}

impl Serialize for ChargingNotification {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Start(n) => n.serialize(s),
            Self::Progress(n) => n.serialize(s),
            Self::End(n) => n.serialize(s),
            Self::Error(n) => n.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for ChargingNotification {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;

        let value = serde_json::Value::deserialize(d)?;
        let tag = value
            .get("Type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| D::Error::missing_field("Type"))?;
        let notification_type: ChargingNotificationType = tag.parse().map_err(D::Error::custom)?;
        macro_rules! parse {
            ($variant:ident) => {
                Self::$variant(serde_json::from_value(value).map_err(D::Error::custom)?)
            };
        }
        Ok(match notification_type {
            ChargingNotificationType::Start => parse!(Start),
            ChargingNotificationType::Progress => parse!(Progress),
            ChargingNotificationType::End => parse!(End),
            ChargingNotificationType::Error => parse!(Error),
        })
    }
}

strict_builder!(
    ChargingNotificationStart,
    ChargingNotificationStartBuilder,
    charging_notification_start_builder
);
strict_builder!(
    ChargingNotificationProgress,
    ChargingNotificationProgressBuilder,
    charging_notification_progress_builder
);
strict_builder!(ChargingNotificationEnd, ChargingNotificationEndBuilder, charging_notification_end_builder);
strict_builder!(
    ChargingNotificationError,
    ChargingNotificationErrorBuilder,
    charging_notification_error_builder
);

#[cfg(test)]
mod tests {
    use super::*;

    fn progress() -> ChargingNotificationProgress {
        ChargingNotificationProgress {
            notification_type: ChargingNotificationType::Progress,
            session_id: "f98efba4-02d8-4fa0-b810-9a9d50d2c527".parse().unwrap(),
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            identification: None,
            evse_id: "DE*XYZ*ETEST1".parse().unwrap(),
            charging_start: "2020-09-23T14:17:53.038Z".parse().unwrap(),
            event_occurred: "2020-09-23T14:25:53.038Z".parse().unwrap(),
            charging_duration: Some(480_000),
            session_start: None,
            consumed_energy_progress: Some("9".parse().unwrap()),
            meter_value_start: Some(Number::ZERO),
            meter_value_in_between: None,
            operator_id: "DE*ABC".parse().unwrap(),
            partner_product_id: None,
            extensions: Extensions::new(),
        }
    }

    #[test]
    fn the_notification_union_dispatches_on_the_type_tag() {
        let json = serde_json::to_string(&ChargingNotification::Progress(progress())).unwrap();
        assert!(json.contains(r#""Type":"Progress""#));
        let back: ChargingNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(back.notification_type(), ChargingNotificationType::Progress);
        assert_eq!(back.evse_id().as_str(), "DE*XYZ*ETEST1");
    }

    #[test]
    fn a_progress_notification_with_neither_figure_says_nothing() {
        let mut n = progress();
        n.charging_duration = None;
        n.consumed_energy_progress = None;
        assert_eq!(n.validate().unwrap_err().as_slice()[0].code, ViolationCode::MissingConditional);
    }

    #[test]
    fn a_duration_that_contradicts_the_timestamps_is_reported() {
        // 14:25:53 - 14:17:53 is 480 s = 480000 ms, which is what the spec's own example says.
        let n = progress();
        assert_eq!(n.implied_duration_ms(), Some(480_000));
        assert!(n.validate().is_ok());

        let wrong = ChargingNotificationProgress { charging_duration: Some(48_000), ..progress() };
        let err = wrong.validate().unwrap_err();
        assert_eq!(err.as_slice()[0].pointer, "/ChargingDuration");
    }

    #[test]
    fn clock_skew_within_a_minute_is_tolerated() {
        let n = ChargingNotificationProgress { charging_duration: Some(480_000 + 30_000), ..progress() };
        assert!(n.validate().is_ok(), "half a minute of skew is not a spec violation");
    }

    #[test]
    fn a_mislabelled_notification_is_reported() {
        let n =
            ChargingNotificationProgress { notification_type: ChargingNotificationType::Start, ..progress() };
        assert!(n.validate().unwrap_err().iter().any(|x| x.pointer == "/Type"));
    }
}
