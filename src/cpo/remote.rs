//! The four messages Hubject sends *into* a CPO: remote start, stop and reservation.
//!
//! These arrive at the CPO's own HTTPS endpoints, registered in the Hubject portal. They are the
//! half of OICP that implementations most often skip, and the reason [`CpoService`] exists.
//!
//! [`CpoService`]: crate::server::CpoService

use serde::{Deserialize, Serialize};

use crate::types::{
    EvseId, Extensions, Identification, IdentificationProcess, PartnerSessionId, ProviderId, SessionId, Text,
    Validate, Validator, ViolationCode, strict_builder, validate_fields,
};

/// An EMP asks the CPO to start a session remotely — from a phone app, typically.
///
/// Hubject derives the CPO's `OperatorID` from the `EvseID` and routes accordingly, which is why
/// there is no operator field here.
///
/// Spec: `eRoamingAuthorizeRemoteStart_V2.1`,
/// `POST /charging/v21/providers/{providerID}/authorize-remote/start`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct AuthorizeRemoteStartRequest {
    /// The session Hubject has opened.
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
    /// The EMP asking.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// The spot to start.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// On whose behalf. Must be a
    /// [`RemoteIdentification`](crate::types::RemoteIdentification).
    #[serde(rename = "Identification")]
    pub identification: Identification,
    /// The tariff product to bill under.
    #[serde(rename = "PartnerProductID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub partner_product_id: Option<Text<50>>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for AuthorizeRemoteStartRequest {
    fn validate_in(&self, v: &mut Validator) {
        v.enter("Identification");
        self.identification.validate_in_process(v, IdentificationProcess::RemoteAuthorization);
        v.leave();
        check_provider_matches_contract(v, &self.provider_id, &self.identification);
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            provider_id as "ProviderID",
            evse_id as "EvseID",
            partner_product_id as "PartnerProductID",
        );
    }
}

/// An EMP asks the CPO to stop a session it started remotely.
///
/// Spec: `eRoamingAuthorizeRemoteStop_V2.1`,
/// `POST /charging/v21/providers/{providerID}/authorize-remote/stop`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct AuthorizeRemoteStopRequest {
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
    /// The EMP asking.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// The spot.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for AuthorizeRemoteStopRequest {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            provider_id as "ProviderID",
            evse_id as "EvseID",
        );
    }
}

/// An EMP asks the CPO to reserve a charging spot.
///
/// Note the field spelling: see erratum [`OICP23-E005`](crate::types::ERRATA). The reservation
/// schemas name the EMP session id `EMPPartnerSessionId` while every other message — and their own
/// examples — use `EMPPartnerSessionID`. This crate writes the consistent spelling and reads both.
///
/// Spec: `eRoamingAuthorizeRemoteReservationStart_V1.1`,
/// `POST /reservation/v11/providers/{providerID}/reservation-start-request`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct AuthorizeRemoteReservationStartRequest {
    /// The session Hubject has opened for the reservation.
    #[serde(rename = "SessionID", default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// The CPO's own session id.
    #[serde(rename = "CPOPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub cpo_partner_session_id: Option<PartnerSessionId>,
    /// The EMP's own session id.
    #[serde(
        rename = "EMPPartnerSessionID",
        alias = "EMPPartnerSessionId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[builder(into)]
    pub emp_partner_session_id: Option<PartnerSessionId>,
    /// The EMP asking.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// The spot to reserve.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// On whose behalf.
    #[serde(rename = "Identification")]
    pub identification: Identification,
    /// The tariff product.
    #[serde(rename = "PartnerProductID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub partner_product_id: Option<Text<50>>,
    /// How long to hold the spot, in minutes: 1 to 99.
    #[serde(rename = "Duration", default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<i64>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for AuthorizeRemoteReservationStartRequest {
    fn validate_in(&self, v: &mut Validator) {
        v.enter("Identification");
        self.identification.validate_in_process(v, IdentificationProcess::RemoteAuthorization);
        v.leave();
        check_provider_matches_contract(v, &self.provider_id, &self.identification);
        if let Some(duration) = self.duration {
            if !(1..=99).contains(&duration) {
                v.report_at(
                    "Duration",
                    ViolationCode::OutOfRange,
                    format!("a reservation lasts 1 to 99 minutes, not {duration}"),
                );
            }
        }
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            provider_id as "ProviderID",
            evse_id as "EvseID",
            partner_product_id as "PartnerProductID",
        );
    }
}

/// An EMP releases a reservation.
///
/// Spec: `eRoamingAuthorizeRemoteReservationStop_V1.1`,
/// `POST /reservation/v11/providers/{providerID}/reservation-stop-request`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct AuthorizeRemoteReservationStopRequest {
    /// The reservation to release.
    #[serde(rename = "SessionID")]
    pub session_id: SessionId,
    /// The CPO's own session id.
    #[serde(rename = "CPOPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub cpo_partner_session_id: Option<PartnerSessionId>,
    /// The EMP's own session id. See erratum `OICP23-E005`.
    #[serde(
        rename = "EMPPartnerSessionID",
        alias = "EMPPartnerSessionId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[builder(into)]
    pub emp_partner_session_id: Option<PartnerSessionId>,
    /// The EMP asking.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// The spot.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for AuthorizeRemoteReservationStopRequest {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(
            self,
            v,
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
            provider_id as "ProviderID",
            evse_id as "EvseID",
        );
    }
}

/// A remote request names the EMP twice: once directly, once inside the contract id. When they
/// disagree, one of them is wrong, and Hubject answers `019 Inconsistent EvcoID`.
///
/// A hub provider legitimately acts for a bundled sub-EMP, so this is a report, not a refusal.
fn check_provider_matches_contract(
    v: &mut Validator,
    provider_id: &ProviderId,
    identification: &Identification,
) {
    if let Some(evco_id) = identification.evco_id() {
        let derived = evco_id.provider_id();
        if derived != *provider_id {
            v.report_at(
                "Identification",
                ViolationCode::Inconsistent,
                format!(
                    "the contract {evco_id} belongs to provider {derived}, but the request is from \
                     {provider_id}; that is legitimate only if {provider_id} is a hub provider \
                     bundling {derived}"
                ),
            );
        }
    }
}

strict_builder!(
    AuthorizeRemoteStartRequest,
    AuthorizeRemoteStartRequestBuilder,
    authorize_remote_start_request_builder
);
strict_builder!(
    AuthorizeRemoteStopRequest,
    AuthorizeRemoteStopRequestBuilder,
    authorize_remote_stop_request_builder
);
strict_builder!(
    AuthorizeRemoteReservationStartRequest,
    AuthorizeRemoteReservationStartRequestBuilder,
    authorize_remote_reservation_start_request_builder
);
strict_builder!(
    AuthorizeRemoteReservationStopRequest,
    AuthorizeRemoteReservationStopRequestBuilder,
    authorize_remote_reservation_stop_request_builder
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RemoteIdentification, RfidMifareFamilyIdentification, Uid};

    fn remote_start() -> AuthorizeRemoteStartRequest {
        AuthorizeRemoteStartRequest {
            session_id: "f98efba4-02d8-4fa0-b810-9a9d50d2c527".parse().unwrap(),
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            provider_id: "DE-DCB".parse().unwrap(),
            evse_id: "DE*XYZ*ETEST1".parse().unwrap(),
            identification: Identification::Remote(RemoteIdentification {
                evco_id: "DE-DCB-C12345678-X".parse().unwrap(),
            }),
            partner_product_id: None,
            extensions: Extensions::new(),
        }
    }

    #[test]
    fn a_conformant_remote_start_validates() {
        assert!(remote_start().validate().is_ok());
    }

    #[test]
    fn only_a_remote_identification_belongs_in_a_remote_start() {
        let request = AuthorizeRemoteStartRequest {
            identification: Identification::RfidMifareFamily(RfidMifareFamilyIdentification {
                uid: Uid::new("7568290FFF765F").unwrap(),
            }),
            ..remote_start()
        };
        let err = request.validate().unwrap_err();
        assert!(err.iter().any(|x| x.message.contains("only RemoteIdentification")));
    }

    #[test]
    fn a_contract_from_another_provider_is_reported() {
        let request = AuthorizeRemoteStartRequest {
            identification: Identification::Remote(RemoteIdentification {
                evco_id: "DE-8EO-C12345678-X".parse().unwrap(),
            }),
            ..remote_start()
        };
        let err = request.validate().unwrap_err();
        assert!(err.iter().any(|x| x.message.contains("hub provider")));
    }

    #[test]
    fn reservation_duration_is_bounded() {
        let base = AuthorizeRemoteReservationStartRequest {
            session_id: None,
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            provider_id: "DE-DCB".parse().unwrap(),
            evse_id: "DE*XYZ*ETEST1".parse().unwrap(),
            identification: Identification::Remote(RemoteIdentification {
                evco_id: "DE-DCB-C12345678-X".parse().unwrap(),
            }),
            partner_product_id: None,
            duration: Some(15),
            extensions: Extensions::new(),
        };
        assert!(base.validate().is_ok());
        for bad in [0, 100, -1] {
            let request = AuthorizeRemoteReservationStartRequest { duration: Some(bad), ..base.clone() };
            assert!(request.validate().unwrap_err().iter().any(|x| x.pointer == "/Duration"), "{bad}");
        }
    }

    #[test]
    fn the_reservation_session_id_reads_under_both_spellings() {
        // Erratum OICP23-E005.
        for key in ["EMPPartnerSessionID", "EMPPartnerSessionId"] {
            let json = format!(
                r#"{{"SessionID":"f98efba4-02d8-4fa0-b810-9a9d50d2c527","{key}":"2345ABC",
                    "ProviderID":"DE-DCB","EvseID":"DE*XYZ*ETEST1"}}"#
            );
            let request: AuthorizeRemoteReservationStopRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(request.emp_partner_session_id.as_ref().unwrap().as_str(), "2345ABC", "{key}");
            let out = serde_json::to_value(&request).unwrap();
            assert!(out.get("EMPPartnerSessionID").is_some());
            assert!(out.get("EMPPartnerSessionId").is_none());
        }
    }
}
