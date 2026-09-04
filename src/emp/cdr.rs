//! CDR retrieval, and the authentication-data upload.

use serde::{Deserialize, Serialize};

use crate::types::{
    ActionType, DateTime, Extensions, Identification, IdentificationProcess, OperatorId, ProviderId,
    SessionId, Validate, Validator, ViolationCode, strict_builder, validate_fields,
};

/// Asks Hubject for the charge detail records of a time range.
///
/// > *This message is only mandatory for offline EMPs.*
///
/// An EMP that receives CDRs pushed to it does not need this. One that reconciles — or one
/// recovering from an outage during which pushes were lost — does.
///
/// See erratum [`OICP23-E004`](crate::types::ERRATA) on the `CDRForwarded` field: the schema
/// spells the property `CDRForwarder` while the leading document and the schema's own example say
/// `CDRForwarded`. This crate writes the leading document's spelling and reads both, because a
/// filter that is silently ignored returns the *unfiltered* set — and an EMP that double-counts
/// CDRs pays twice.
///
/// Spec: `eRoamingGetChargeDetailRecords_V2.2`,
/// `POST /cdrmgmt/v22/providers/{providerID}/get-charge-detail-records-request`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct GetChargeDetailRecordsRequest {
    /// The EMP asking.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// The start of the range.
    #[serde(rename = "From")]
    pub from: DateTime,
    /// The end of the range.
    #[serde(rename = "To")]
    pub to: DateTime,
    /// Restrict to these sessions.
    #[serde(rename = "SessionID", default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Vec<SessionId>>,
    /// Restrict to this operator.
    #[serde(rename = "OperatorID", default, skip_serializing_if = "Option::is_none")]
    pub operator_id: Option<OperatorId>,
    /// Restrict to records that were, or were not, already forwarded to the EMP.
    #[serde(
        rename = "CDRForwarded",
        alias = "CDRForwarder",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cdr_forwarded: Option<bool>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for GetChargeDetailRecordsRequest {
    fn validate_in(&self, v: &mut Validator) {
        if self.from.is_well_formed() && self.to.is_well_formed() && self.from > self.to {
            v.report_at(
                "To",
                ViolationCode::Inconsistent,
                format!("the range ends ({}) before it starts ({})", self.to, self.from),
            );
        }
        validate_fields!(
            self,
            v,
            provider_id as "ProviderID",
            from as "From",
            to as "To",
            session_id as "SessionID",
            operator_id as "OperatorID",
        );
    }
}

/// One card or contract an offline EMP uploads to Hubject.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct AuthenticationDataRecord {
    /// The card or contract.
    #[serde(rename = "Identification")]
    pub identification: Identification,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for AuthenticationDataRecord {
    fn validate_in(&self, v: &mut Validator) {
        v.enter("Identification");
        self.identification.validate_in_process(v, IdentificationProcess::AuthenticationData);
        v.leave();
    }
}

/// One EMP's card base.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct ProviderAuthenticationData {
    /// The EMP.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// The cards and contracts.
    #[serde(rename = "AuthenticationDataRecord")]
    pub authentication_data_record: Vec<AuthenticationDataRecord>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for ProviderAuthenticationData {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(
            self,
            v,
            provider_id as "ProviderID",
            authentication_data_record as "AuthenticationDataRecord",
        );
    }
}

/// Uploads an offline EMP's card base to Hubject, so CPOs can authorize against it.
///
/// > *This message is only for EMPs onboarded to the Hubject platform as offline EMPs.*
///
/// An offline EMP does not answer authorization requests in real time; Hubject answers them on the
/// EMP's behalf, from this data. Which makes [`ActionType::FullLoad`] here as dangerous as it is
/// on an EVSE push: a truncated full load silently invalidates every card it left out, and those
/// drivers are refused at charging points until the next upload.
///
/// Spec: `eRoamingPushAuthenticationData_V2.1`,
/// `POST /authdata/v21/providers/{providerID}/push-request`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PushAuthenticationDataRequest {
    /// What Hubject should do with the payload.
    #[serde(rename = "ActionType")]
    pub action_type: ActionType,
    /// The card base.
    #[serde(rename = "ProviderAuthenticationData")]
    pub provider_authentication_data: ProviderAuthenticationData,
}

impl Validate for PushAuthenticationDataRequest {
    fn validate_in(&self, v: &mut Validator) {
        if self.action_type.is_destructive_replace()
            && self.provider_authentication_data.authentication_data_record.is_empty()
        {
            v.report(
                ViolationCode::Inconsistent,
                "a fullLoad with no records invalidates every card this EMP has issued; \
                 its drivers will be refused at every charging point until the next upload",
            );
        }
        validate_fields!(
            self,
            v,
            action_type as "ActionType",
            provider_authentication_data as "ProviderAuthenticationData",
        );
    }
}

strict_builder!(
    GetChargeDetailRecordsRequest,
    GetChargeDetailRecordsRequestBuilder,
    get_charge_detail_records_request_builder
);
strict_builder!(
    AuthenticationDataRecord,
    AuthenticationDataRecordBuilder,
    authentication_data_record_builder
);
strict_builder!(
    ProviderAuthenticationData,
    ProviderAuthenticationDataBuilder,
    provider_authentication_data_builder
);
strict_builder!(
    PushAuthenticationDataRequest,
    PushAuthenticationDataRequestBuilder,
    push_authentication_data_request_builder
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cdr_forwarded_filter_reads_under_both_spellings() {
        // Erratum OICP23-E004.
        for key in ["CDRForwarded", "CDRForwarder"] {
            let json = format!(
                r#"{{"ProviderID":"DE-DCB","From":"2020-08-23T14:20:10.285Z","To":"2020-09-23T14:20:10.285Z","{key}":false}}"#
            );
            let request: GetChargeDetailRecordsRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(request.cdr_forwarded, Some(false), "{key} was not read");
            let out = serde_json::to_value(&request).unwrap();
            assert!(out.get("CDRForwarded").is_some(), "the leading document's spelling is written");
            assert!(out.get("CDRForwarder").is_none());
        }
    }

    #[test]
    fn a_backwards_time_range_is_reported() {
        let request = GetChargeDetailRecordsRequest {
            provider_id: "DE-DCB".parse().unwrap(),
            from: "2020-09-23T14:20:10.285Z".parse().unwrap(),
            to: "2020-08-23T14:20:10.285Z".parse().unwrap(),
            session_id: None,
            operator_id: None,
            cdr_forwarded: None,
            extensions: Extensions::new(),
        };
        assert_eq!(request.validate().unwrap_err().as_slice()[0].pointer, "/To");
    }

    #[test]
    fn a_truncated_full_load_of_the_card_base_is_reported() {
        let request = PushAuthenticationDataRequest {
            action_type: ActionType::FullLoad,
            provider_authentication_data: ProviderAuthenticationData {
                provider_id: "DE-DCB".parse().unwrap(),
                authentication_data_record: vec![],
                extensions: Extensions::new(),
            },
        };
        assert!(request.validate().unwrap_err().as_slice()[0].message.contains("invalidates every card"));
    }
}
