//! Authorization: the four messages that decide whether a car may charge.

use serde::{Deserialize, Serialize};

use crate::oicp_enum;
use crate::types::{
    EvseId, Extensions, Identification, IdentificationProcess, OperatorId, PartnerSessionId, ProviderId,
    SessionId, StatusCode, Text, Validate, Validator, ViolationCode, strict_builder, validate_fields,
};

oicp_enum! {
    /// Whether the driver may charge.
    ///
    /// One of the two closed enums in this crate: a third value has no safe reading. A CPO that
    /// treated an unrecognised status as "probably authorized" would give away energy; as "not
    /// authorized" it would strand a driver. Neither is a decision a library should make silently.
    pub enum AuthorizationStatus {
        /// The user is authorized.
        Authorized = "Authorized",
        /// The user is not authorized.
        NotAuthorized = "NotAuthorized",
    }
}

impl AuthorizationStatus {
    /// Whether charging may begin.
    #[must_use]
    pub const fn is_authorized(self) -> bool {
        matches!(self, Self::Authorized)
    }
}

/// Asks Hubject whether a driver may start charging.
///
/// The CPO sends this when someone presents a card or an app at a charging point. Hubject finds
/// the EMP from the identification and forwards the question.
///
/// The spec's advice on the optional `EvseID`: *"If the Evse ID can be provided, we recommend to
/// include the EVSE ID in this message; it will help for support matters."*
///
/// Spec: `eRoamingAuthorizeStart_V2.1`,
/// `POST /charging/v21/operators/{operatorID}/authorize/start`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct AuthorizeStartRequest {
    /// The Hubject session, when the CPO already has one.
    #[serde(rename = "SessionID", default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// The CPO's own session id.
    #[serde(rename = "CPOPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub cpo_partner_session_id: Option<PartnerSessionId>,
    /// The EMP's own session id.
    #[serde(rename = "EMPPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub emp_partner_session_id: Option<PartnerSessionId>,
    /// The operator asking.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// The charging spot, if known.
    #[serde(rename = "EvseID", default, skip_serializing_if = "Option::is_none")]
    pub evse_id: Option<EvseId>,
    /// Who is asking to charge.
    #[serde(rename = "Identification")]
    pub identification: Identification,
    /// The tariff product the session should be billed under.
    #[serde(rename = "PartnerProductID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub partner_product_id: Option<Text<50>>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for AuthorizeStartRequest {
    fn validate_in(&self, v: &mut Validator) {
        v.enter("Identification");
        self.identification.validate_in_process(v, IdentificationProcess::Authorization);
        v.leave();
        if let Some(evse_id) = &self.evse_id {
            let derived = evse_id.operator_id();
            if derived != self.operator_id {
                v.report_at(
                    "EvseID",
                    ViolationCode::Inconsistent,
                    format!(
                        "{evse_id} belongs to operator {derived}, but the request is from {}",
                        self.operator_id
                    ),
                );
            }
        }
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            operator_id as "OperatorID",
            evse_id as "EvseID",
            partner_product_id as "PartnerProductID",
        );
    }
}

/// Hubject's answer to [`AuthorizeStartRequest`], relayed from the EMP.
///
/// A `NotAuthorized` answer carries the reason in [`status_code`](Self::status_code) — one of the
/// `1xx`/`2xx` authentication codes, or `210 No valid contract`.
///
/// Spec: `eRoamingAuthorizationStart`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct AuthorizationStartResponse {
    /// The Hubject session that was opened for this charging process.
    #[serde(rename = "SessionID", default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// The CPO's own session id.
    #[serde(rename = "CPOPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub cpo_partner_session_id: Option<PartnerSessionId>,
    /// The EMP's own session id.
    #[serde(rename = "EMPPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub emp_partner_session_id: Option<PartnerSessionId>,
    /// The EMP that answered.
    #[serde(rename = "ProviderID", default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<ProviderId>,
    /// Whether the driver may charge.
    #[serde(rename = "AuthorizationStatus")]
    pub authorization_status: AuthorizationStatus,
    /// Why.
    #[serde(rename = "StatusCode")]
    pub status_code: StatusCode,
    /// Which identifications may stop this session.
    ///
    /// The EMP can name a set of cards — typically every card on the contract — that the CPO must
    /// accept as a stop request. A CPO that ignores this field strands a driver whose partner has
    /// the other card.
    #[serde(rename = "AuthorizationStopIdentifications", default, skip_serializing_if = "Option::is_none")]
    pub authorization_stop_identifications: Option<Vec<Identification>>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl AuthorizationStartResponse {
    /// An `Authorized` answer that opens `session_id`.
    #[must_use]
    pub fn authorized(session_id: SessionId) -> Self {
        Self {
            session_id: Some(session_id),
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            provider_id: None,
            authorization_status: AuthorizationStatus::Authorized,
            status_code: StatusCode::success(),
            authorization_stop_identifications: None,
            extensions: Extensions::new(),
        }
    }

    /// A `NotAuthorized` answer carrying `code` as the reason.
    #[must_use]
    pub fn not_authorized(code: crate::types::Code) -> Self {
        Self {
            session_id: None,
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            provider_id: None,
            authorization_status: AuthorizationStatus::NotAuthorized,
            status_code: StatusCode::new(code),
            authorization_stop_identifications: None,
            extensions: Extensions::new(),
        }
    }

    /// Whether charging may begin.
    #[must_use]
    pub const fn is_authorized(&self) -> bool {
        self.authorization_status.is_authorized()
    }
}

impl Validate for AuthorizationStartResponse {
    fn validate_in(&self, v: &mut Validator) {
        // The two fields have to tell the same story, or the CPO cannot act on either.
        if self.authorization_status.is_authorized() && !self.status_code.is_success() {
            v.report(
                ViolationCode::Inconsistent,
                format!(
                    "AuthorizationStatus is Authorized but StatusCode is {}; an authorized session reports 000",
                    self.status_code.code
                ),
            );
        }
        if !self.authorization_status.is_authorized() && self.status_code.is_success() {
            v.report(
                ViolationCode::Inconsistent,
                "AuthorizationStatus is NotAuthorized but StatusCode is 000 Success; \
                 the driver cannot be told why they were refused",
            );
        }
        // An authorized start without a session id leaves the CPO unable to send a CDR.
        if self.authorization_status.is_authorized() && self.session_id.is_none() {
            v.report_at(
                "SessionID",
                ViolationCode::MissingConditional,
                "an Authorized response needs a SessionID; without it the CPO has nothing to put on \
                 the charge detail record and the session cannot be billed",
            );
        }
        if let Some(stops) = &self.authorization_stop_identifications {
            v.enter("AuthorizationStopIdentifications");
            for (i, id) in stops.iter().enumerate() {
                v.enter(&i.to_string());
                id.validate_in_process(v, IdentificationProcess::Authorization);
                v.leave();
            }
            v.leave();
        }
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            provider_id as "ProviderID",
            status_code as "StatusCode",
        );
    }
}

/// Asks Hubject whether a driver may stop a charging session.
///
/// Spec: `eRoamingAuthorizeStop_V2.1`,
/// `POST /charging/v21/operators/{operatorID}/authorize/stop`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct AuthorizeStopRequest {
    /// The session to stop.
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
    /// The operator asking.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// The charging spot.
    #[serde(rename = "EvseID", default, skip_serializing_if = "Option::is_none")]
    pub evse_id: Option<EvseId>,
    /// Who is asking to stop.
    #[serde(rename = "Identification")]
    pub identification: Identification,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for AuthorizeStopRequest {
    fn validate_in(&self, v: &mut Validator) {
        v.enter("Identification");
        self.identification.validate_in_process(v, IdentificationProcess::Authorization);
        v.leave();
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            operator_id as "OperatorID",
            evse_id as "EvseID",
        );
    }
}

/// Hubject's answer to [`AuthorizeStopRequest`].
///
/// Spec: `eRoamingAuthorizationStop`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct AuthorizationStopResponse {
    /// The session.
    #[serde(rename = "SessionID", default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// The CPO's own session id.
    #[serde(rename = "CPOPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub cpo_partner_session_id: Option<PartnerSessionId>,
    /// The EMP's own session id.
    #[serde(rename = "EMPPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub emp_partner_session_id: Option<PartnerSessionId>,
    /// The EMP that answered.
    #[serde(rename = "ProviderID", default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<ProviderId>,
    /// Whether the session may stop.
    #[serde(rename = "AuthorizationStatus")]
    pub authorization_status: AuthorizationStatus,
    /// Why.
    #[serde(rename = "StatusCode")]
    pub status_code: StatusCode,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for AuthorizationStopResponse {
    fn validate_in(&self, v: &mut Validator) {
        if self.authorization_status.is_authorized() && !self.status_code.is_success() {
            v.report(
                ViolationCode::Inconsistent,
                format!("AuthorizationStatus is Authorized but StatusCode is {}", self.status_code.code),
            );
        }
        if !self.authorization_status.is_authorized() && self.status_code.is_success() {
            v.report(
                ViolationCode::Inconsistent,
                "AuthorizationStatus is NotAuthorized but StatusCode is 000 Success",
            );
        }
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            provider_id as "ProviderID",
            status_code as "StatusCode",
        );
    }
}

strict_builder!(AuthorizeStartRequest, AuthorizeStartRequestBuilder, authorize_start_request_builder);
strict_builder!(
    AuthorizationStartResponse,
    AuthorizationStartResponseBuilder,
    authorization_start_response_builder
);
strict_builder!(AuthorizeStopRequest, AuthorizeStopRequestBuilder, authorize_stop_request_builder);
strict_builder!(
    AuthorizationStopResponse,
    AuthorizationStopResponseBuilder,
    authorization_stop_response_builder
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Code, RfidMifareFamilyIdentification, Uid};

    fn rfid() -> Identification {
        Identification::RfidMifareFamily(RfidMifareFamilyIdentification {
            uid: Uid::new("7568290FFF765F").unwrap(),
        })
    }

    #[test]
    fn authorization_status_is_closed_because_neither_default_is_safe() {
        assert!(serde_json::from_str::<AuthorizationStatus>(r#""Maybe""#).is_err());
    }

    #[test]
    fn an_authorized_response_without_a_session_id_cannot_be_billed() {
        let mut response =
            AuthorizationStartResponse::authorized("b2688855-7f00-0002-6d8e-48d883f6abb6".parse().unwrap());
        assert!(response.validate().is_ok());

        response.session_id = None;
        let err = response.validate().unwrap_err();
        assert_eq!(err.as_slice()[0].pointer, "/SessionID");
        assert_eq!(err.as_slice()[0].code, ViolationCode::MissingConditional);
    }

    #[test]
    fn status_and_code_must_tell_the_same_story() {
        let mut response =
            AuthorizationStartResponse::authorized("b2688855-7f00-0002-6d8e-48d883f6abb6".parse().unwrap());
        response.status_code = StatusCode::new(Code::NoValidContract);
        assert!(response.validate().unwrap_err().iter().any(|x| x.code == ViolationCode::Inconsistent));

        // A refusal carries a real reason.
        let refused = AuthorizationStartResponse::not_authorized(Code::NoValidContract);
        assert!(refused.validate().is_ok());
        assert!(!refused.is_authorized());
    }

    #[test]
    fn an_evse_from_another_operator_is_reported() {
        let request = AuthorizeStartRequest {
            session_id: None,
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            operator_id: "DE*ABC".parse().unwrap(),
            evse_id: Some("DE*XYZ*ETEST1".parse().unwrap()),
            identification: rfid(),
            partner_product_id: None,
            extensions: Extensions::new(),
        };
        let err = request.validate().unwrap_err();
        assert_eq!(err.as_slice()[0].pointer, "/EvseID");

        let ok = AuthorizeStartRequest { evse_id: Some("DE*ABC*E123".parse().unwrap()), ..request };
        assert!(ok.validate().is_ok());
    }
}
