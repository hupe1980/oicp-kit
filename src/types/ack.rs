//! `Acknowledgement` — the answer to almost every OICP request.

use serde::{Deserialize, Serialize};

use super::builder::strict_builder;
use super::extensions::Extensions;
use super::ids::SessionId;
use super::status::{Code, StatusCode};
use super::text::PartnerSessionId;
use super::validate::{Validate, Validator, ViolationCode, validate_fields};

/// The response to a command: whether it worked, and why not if it did not.
///
/// > *The acknowledgement is a message that is sent in response to several requests.*
///
/// OICP has no envelope — a pull returns its page directly — but every *command* comes back as
/// one of these. Note the two-level result: `Result: false` says the operation did not happen, and
/// [`status_code`](Self::status_code) says why. A peer that answers `HTTP 200` with
/// `Result: false` has **failed**, and this crate's client turns that into an `Err`.
///
/// Spec: `eRoamingAcknowledgment`. (Hubject spells it without the second `e`; this crate uses the
/// standard English spelling for the Rust type and keeps the wire name where it belongs.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct Acknowledgement {
    /// Whether the operation was performed successfully.
    #[serde(rename = "Result")]
    pub result: bool,
    /// Why, in the machine-readable terms of the spec's status table.
    #[serde(rename = "StatusCode")]
    pub status_code: StatusCode,
    /// The Hubject session this acknowledgement relates to.
    #[serde(rename = "SessionID", default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// The CPO's own session id for the operation.
    #[serde(rename = "CPOPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub cpo_partner_session_id: Option<PartnerSessionId>,
    /// The EMP's own session id for the operation.
    #[serde(rename = "EMPPartnerSessionID", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub emp_partner_session_id: Option<PartnerSessionId>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Acknowledgement {
    /// A successful acknowledgement: `Result: true`, `000 Success`.
    #[must_use]
    pub fn success() -> Self {
        Self {
            result: true,
            status_code: StatusCode::success(),
            session_id: None,
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            extensions: Extensions::new(),
        }
    }

    /// A failed acknowledgement carrying `code`.
    #[must_use]
    pub fn failure(code: Code) -> Self {
        Self { result: false, status_code: StatusCode::new(code), ..Self::success() }
    }

    /// A failed acknowledgement carrying `code` and free-text detail.
    #[must_use]
    pub fn failure_with(code: Code, additional_info: impl Into<String>) -> Self {
        Self { result: false, status_code: StatusCode::with_info(code, additional_info), ..Self::success() }
    }

    /// Attaches the Hubject session id.
    #[must_use]
    pub fn with_session(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Whether the operation was performed.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.result
    }

    /// The status code carried, for matching.
    #[must_use]
    pub fn code(&self) -> &Code {
        &self.status_code.code
    }
}

impl core::fmt::Display for Acknowledgement {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.result {
            write!(f, "ok ({})", self.status_code)
        } else {
            write!(f, "refused: {}", self.status_code)
        }?;
        if let Some(session) = &self.session_id {
            write!(f, " [session {session}]")?;
        }
        Ok(())
    }
}

/// A **failed** acknowledgement is an error, so it can be `?`-ed and boxed like one.
///
/// A successful one is not an error, and this implementation does not pretend otherwise: it exists
/// because every place that hands back an `Acknowledgement` in the `Err` position — the mock
/// broker's refusals, a client's [`OicpError::Rejected`](crate::transport::OicpError) — is a
/// failure the caller wants to propagate.
impl std::error::Error for Acknowledgement {}

impl Validate for Acknowledgement {
    fn validate_in(&self, v: &mut Validator) {
        // The two fields must agree. A `Result: true` with a failure code — or the reverse — is
        // the single most common way a hand-rolled OICP implementation misleads its partner.
        if self.result && !self.status_code.is_success() {
            v.report(
                ViolationCode::Inconsistent,
                format!(
                    "Result is true but StatusCode is {}; a successful operation reports 000",
                    self.status_code.code
                ),
            );
        }
        if !self.result && self.status_code.is_success() {
            v.report(
                ViolationCode::Inconsistent,
                "Result is false but StatusCode is 000 Success; the peer cannot tell what went wrong",
            );
        }
        validate_fields!(
            self,
            v,
            status_code as "StatusCode",
            session_id as "SessionID",
            cpo_partner_session_id as "CPOPartnerSessionID",
            emp_partner_session_id as "EMPPartnerSessionID",
        );
    }
}

// Every builder's `build()` validates; `build_unchecked()` is the escape hatch.
strict_builder!(Acknowledgement, AcknowledgementBuilder, acknowledgement_builder);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_success_acknowledgement_round_trips() {
        let ack = Acknowledgement::success();
        let json = serde_json::to_string(&ack).unwrap();
        assert_eq!(json, r#"{"Result":true,"StatusCode":{"Code":"000","Description":"Success"}}"#);
        assert_eq!(serde_json::from_str::<Acknowledgement>(&json).unwrap(), ack);
        assert!(ack.validate().is_ok());
    }

    #[test]
    fn result_and_status_code_must_agree() {
        let mut ack = Acknowledgement::success();
        ack.result = false;
        assert_eq!(ack.validate().unwrap_err().as_slice()[0].code, ViolationCode::Inconsistent);

        let mut ack = Acknowledgement::failure(Code::UnknownEvseId);
        ack.result = true;
        assert_eq!(ack.validate().unwrap_err().as_slice()[0].code, ViolationCode::Inconsistent);

        assert!(Acknowledgement::failure(Code::UnknownEvseId).validate().is_ok());
    }

    #[test]
    fn unknown_fields_on_an_acknowledgement_survive() {
        let json = r#"{"Result":true,"StatusCode":{"Code":"000"},"HubjectAddedThis":"later"}"#;
        let ack: Acknowledgement = serde_json::from_str(json).unwrap();
        assert_eq!(ack.extensions.get::<String>("HubjectAddedThis").unwrap().unwrap(), "later");
        assert_eq!(serde_json::to_string(&ack).unwrap(), json);
    }
}
