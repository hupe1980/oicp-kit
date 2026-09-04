//! `StatusCode` and `Code` — OICP's error table, as a type that knows what its values mean.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};

use super::builder::strict_builder;
use super::text::Text;
use super::validate::{Validate, Validator, ViolationCode, validate_fields};

/// One of the status codes OICP 2.3 defines, or a code this crate has not seen.
///
/// The specification gives these as a table of three-digit strings in the `StatusCode.Code`
/// description. They are the *only* machine-readable signal a partner gets about why an operation
/// failed, so this crate makes them a type rather than a string: the semantics —
/// [is this retryable?](Self::is_retryable), [is this an authorization
/// failure?](Self::is_authorization_failure) — are encoded once, from the spec table, instead of
/// being re-derived (and got wrong) at every call site.
///
/// A code Hubject adds later lands in [`Code::Custom`] and is preserved, like every other open
/// value in this crate.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Code {
    /// `000` — Success. *(General)*
    Success,
    /// `001` — Hubject system error. *(Internal system)*
    HubjectSystemError,
    /// `002` — Hubject database error. *(Internal system)*
    HubjectDatabaseError,
    /// `009` — Data transaction error. *(Internal system)*
    DataTransactionError,
    /// `017` — Unauthorized Access. *(Internal system)*
    ///
    /// In practice this almost always means the `OperatorID`/`ProviderID` in the URL does not
    /// match the TLS client certificate Hubject sees. See
    /// [`ClientIdentity`](crate::client::ClientIdentity), which catches the mismatch locally.
    UnauthorizedAccess,
    /// `018` — Inconsistent EvseID. *(Internal system)*
    InconsistentEvseId,
    /// `019` — Inconsistent EvcoID. *(Internal system)*
    InconsistentEvcoId,
    /// `021` — System error. *(General)*
    SystemError,
    /// `022` — Data error. *(General)*
    DataError,
    /// `101` — QR Code Authentication failed – Invalid Credentials. *(Authentication)*
    QrCodeAuthenticationFailed,
    /// `102` — RFID Authentication failed – invalid UID. *(Authentication)*
    RfidInvalidUid,
    /// `103` — RFID Authentication failed – card not readable. *(Authentication)*
    RfidCardNotReadable,
    /// `105` — PLC Authentication failed - invalid EvcoID. *(Authentication)*
    PlcInvalidEvcoId,
    /// `106` — No positive authentication response. *(Authentication / Internal system)*
    NoPositiveAuthenticationResponse,
    /// `110` — QR Code App Authentication failed – time out error. *(Authentication)*
    QrCodeAppTimeout,
    /// `120` — PLC (ISO/IEC 15118) Authentication failed – invalid underlying EvcoID.
    /// *(Authentication)*
    PlcInvalidUnderlyingEvcoId,
    /// `121` — PLC (ISO/IEC 15118) Authentication failed – invalid certificate. *(Authentication)*
    PlcInvalidCertificate,
    /// `122` — PLC (ISO/IEC 15118) Authentication failed – time out error. *(Authentication)*
    PlcTimeout,
    /// `200` — EvcoID locked. *(Authentication)*
    EvcoIdLocked,
    /// `210` — No valid contract. *(Session)*
    NoValidContract,
    /// `300` — Partner not found. *(Session)*
    PartnerNotFound,
    /// `310` — Partner did not respond. *(Session)*
    PartnerDidNotRespond,
    /// `320` — Service not available. *(Session)*
    ServiceNotAvailable,
    /// `400` — Session is invalid. *(Session)*
    SessionIsInvalid,
    /// `501` — Communication to EVSE failed. *(EVSE)*
    CommunicationToEvseFailed,
    /// `510` — No EV connected to EVSE. *(EVSE)*
    NoEvConnectedToEvse,
    /// `601` — EVSE already reserved. *(EVSE)*
    EvseAlreadyReserved,
    /// `602` — EVSE already in use / wrong token. *(EVSE)*
    EvseAlreadyInUse,
    /// `603` — Unknown EVSE ID. *(EVSE)*
    UnknownEvseId,
    /// `604` — EVSE ID is not Hubject compatible. *(EVSE)*
    EvseIdNotHubjectCompatible,
    /// `700` — EVSE out of service. *(EVSE)*
    EvseOutOfService,
    /// A code this crate does not know, preserved exactly as it arrived.
    Custom(String),
}

/// Which area of the protocol a [`Code`] belongs to, per the spec table's third column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum CodeArea {
    /// General codes.
    General,
    /// Internal system codes.
    InternalSystem,
    /// Authentication codes.
    Authentication,
    /// Session codes.
    Session,
    /// EVSE codes.
    Evse,
    /// A code this crate does not know.
    Unknown,
}

macro_rules! code_table {
    ($( $variant:ident = $wire:literal, $area:ident, $text:literal );* $(;)?) => {
        impl Code {
            /// Every code this crate knows, in numeric order.
            pub const ALL: &'static [Self] = &[ $( Self::$variant ),* ];

            /// The three-digit code as it appears on the wire.
            #[must_use]
            pub fn as_str(&self) -> &str {
                match self {
                    $( Self::$variant => $wire, )*
                    Self::Custom(s) => s.as_str(),
                }
            }

            /// The specification's description of this code.
            #[must_use]
            pub fn description(&self) -> &str {
                match self {
                    $( Self::$variant => $text, )*
                    Self::Custom(_) => "a status code this version of oicp-kit does not know",
                }
            }

            /// Which area of the protocol this code belongs to.
            #[must_use]
            pub fn area(&self) -> CodeArea {
                match self {
                    $( Self::$variant => CodeArea::$area, )*
                    Self::Custom(_) => CodeArea::Unknown,
                }
            }

            /// Whether this is a code the specification documents.
            #[must_use]
            pub fn is_known(&self) -> bool {
                !matches!(self, Self::Custom(_))
            }
        }

        impl From<&str> for Code {
            fn from(s: &str) -> Self {
                match s {
                    $( $wire => Self::$variant, )*
                    other => Self::Custom(other.to_owned()),
                }
            }
        }
    };
}

code_table! {
    Success = "000", General, "Success";
    HubjectSystemError = "001", InternalSystem, "Hubject system error";
    HubjectDatabaseError = "002", InternalSystem, "Hubject database error";
    DataTransactionError = "009", InternalSystem, "Data transaction error";
    UnauthorizedAccess = "017", InternalSystem, "Unauthorized Access";
    InconsistentEvseId = "018", InternalSystem, "Inconsistent EvseID";
    InconsistentEvcoId = "019", InternalSystem, "Inconsistent EvcoID";
    SystemError = "021", General, "System error";
    DataError = "022", General, "Data error";
    QrCodeAuthenticationFailed = "101", Authentication, "QR Code Authentication failed - Invalid Credentials";
    RfidInvalidUid = "102", Authentication, "RFID Authentication failed - invalid UID";
    RfidCardNotReadable = "103", Authentication, "RFID Authentication failed - card not readable";
    PlcInvalidEvcoId = "105", Authentication, "PLC Authentication failed - invalid EvcoID";
    NoPositiveAuthenticationResponse = "106", Authentication, "No positive authentication response";
    QrCodeAppTimeout = "110", Authentication, "QR Code App Authentication failed - time out error";
    PlcInvalidUnderlyingEvcoId = "120", Authentication, "PLC (ISO/IEC 15118) Authentication failed - invalid underlying EvcoID";
    PlcInvalidCertificate = "121", Authentication, "PLC (ISO/IEC 15118) Authentication failed - invalid certificate";
    PlcTimeout = "122", Authentication, "PLC (ISO/IEC 15118) Authentication failed - time out error";
    EvcoIdLocked = "200", Authentication, "EvcoID locked";
    NoValidContract = "210", Session, "No valid contract";
    PartnerNotFound = "300", Session, "Partner not found";
    PartnerDidNotRespond = "310", Session, "Partner did not respond";
    ServiceNotAvailable = "320", Session, "Service not available";
    SessionIsInvalid = "400", Session, "Session is invalid";
    CommunicationToEvseFailed = "501", Evse, "Communication to EVSE failed";
    NoEvConnectedToEvse = "510", Evse, "No EV connected to EVSE";
    EvseAlreadyReserved = "601", Evse, "EVSE already reserved";
    EvseAlreadyInUse = "602", Evse, "EVSE already in use/ wrong token";
    UnknownEvseId = "603", Evse, "Unknown EVSE ID";
    EvseIdNotHubjectCompatible = "604", Evse, "EVSE ID is not Hubject compatible";
    EvseOutOfService = "700", Evse, "EVSE out of service";
}

impl Code {
    /// Whether this code means the operation succeeded.
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }

    /// Whether re-sending the identical request could plausibly succeed.
    ///
    /// True for transient conditions on Hubject's side or the counterparty's — `001`, `002`,
    /// `009`, `021`, `310`, `320`. **False** for every authorization decision, every data error
    /// and every EVSE-state code: retrying `210 No valid contract` or `601 EVSE already reserved`
    /// cannot help, and retrying `017 Unauthorized Access` will not fix a certificate.
    ///
    /// [`RetryPolicy`](crate::client::RetryPolicy) uses exactly this.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::HubjectSystemError
                | Self::HubjectDatabaseError
                | Self::DataTransactionError
                | Self::SystemError
                | Self::PartnerDidNotRespond
                | Self::ServiceNotAvailable
        )
    }

    /// Whether this code reports that the driver may not charge.
    ///
    /// Every `Authentication`-area code plus `210 No valid contract` — the codes that answer
    /// "may this session start?" with "no", as opposed to "something broke".
    #[must_use]
    pub fn is_authorization_failure(&self) -> bool {
        self.area() == CodeArea::Authentication || matches!(self, Self::NoValidContract)
    }

    /// Whether this code is about the state of the charging point rather than the request.
    #[must_use]
    pub fn is_evse_problem(&self) -> bool {
        self.area() == CodeArea::Evse
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.as_str(), self.description())
    }
}

impl FromStr for Code {
    type Err = core::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(s))
    }
}

impl Serialize for Code {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Code {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = Code;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an OICP status code")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Code, E> {
                Ok(Code::from(v))
            }
            // Hubject writes these as strings ("000"), but a partner that hand-rolls its JSON
            // sometimes sends the number. Accepting it costs nothing and saves an integration.
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Code, E> {
                Ok(Code::from(format!("{v:03}").as_str()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Code, E> {
                Ok(Code::from(format!("{v:03}").as_str()))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Code {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "StatusCodeValue".into()
    }
    fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "pattern": "^[0-9]{3}$" })
    }
}

/// The status of an operation: a [`Code`], and optional human-readable detail.
///
/// > *The structure consists of a defined code, an optional functional description of the status,
/// > and optional additional information. It can be used e.g. to send error details or detailed
/// > reasons for a certain process or system behavior.*
///
/// Spec: `StatusCode`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct StatusCode {
    /// The machine-readable code.
    #[serde(rename = "Code")]
    pub code: Code,
    /// A functional description of the status.
    #[serde(rename = "Description", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub description: Option<Text<200>>,
    /// Further individual, non-standardised information.
    #[serde(rename = "AdditionalInfo", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub additional_info: Option<Text<1000>>,
}

impl StatusCode {
    /// A status carrying just `code`, with the spec's own description filled in.
    #[must_use]
    pub fn new(code: Code) -> Self {
        let description = Text::new(code.description()).ok();
        Self { code, description, additional_info: None }
    }

    /// `000 Success`.
    #[must_use]
    pub fn success() -> Self {
        Self::new(Code::Success)
    }

    /// A status carrying `code` and free-text detail.
    #[must_use]
    pub fn with_info(code: Code, additional_info: impl Into<String>) -> Self {
        Self { additional_info: Text::new_unchecked(additional_info).into(), ..Self::new(code) }
    }

    /// Whether this reports success.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.code.is_success()
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)?;
        if let Some(info) = &self.additional_info {
            write!(f, " ({info})")?;
        }
        Ok(())
    }
}

impl Validate for StatusCode {
    fn validate_in(&self, v: &mut Validator) {
        if let Code::Custom(value) = &self.code {
            v.report_at(
                "Code",
                ViolationCode::PatternMismatch,
                format!("{value:?} is not one of the {} status codes OICP 2.3 defines", Code::ALL.len()),
            );
        }
        validate_fields!(self, v, description as "Description", additional_info as "AdditionalInfo");
    }
}

impl From<Code> for StatusCode {
    fn from(code: Code) -> Self {
        Self::new(code)
    }
}

// Every builder's `build()` validates; `build_unchecked()` is the escape hatch.
strict_builder!(StatusCode, StatusCodeBuilder, status_code_builder);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_in_the_spec_table_round_trips() {
        for code in Code::ALL {
            let json = serde_json::to_string(code).unwrap();
            let back: Code = serde_json::from_str(&json).unwrap();
            assert_eq!(*code, back);
            assert_eq!(code.as_str().len(), 3);
        }
        assert_eq!(Code::ALL.len(), 31);
    }

    #[test]
    fn an_unknown_code_is_preserved_not_dropped() {
        let code: Code = serde_json::from_str(r#""999""#).unwrap();
        assert!(!code.is_known());
        assert_eq!(code.as_str(), "999");
        assert_eq!(serde_json::to_string(&code).unwrap(), r#""999""#);
    }

    #[test]
    fn a_code_sent_as_a_number_is_understood() {
        let code: Code = serde_json::from_str("0").unwrap();
        assert_eq!(code, Code::Success);
        // …and goes back out as the string the spec requires.
        assert_eq!(serde_json::to_string(&code).unwrap(), r#""000""#);
    }

    #[test]
    fn retry_semantics_come_from_the_table_not_from_guesswork() {
        // Transient: worth another attempt.
        assert!(Code::HubjectSystemError.is_retryable());
        assert!(Code::PartnerDidNotRespond.is_retryable());
        assert!(Code::ServiceNotAvailable.is_retryable());
        // Decisions and states: retrying is pointless, and for a start/stop, harmful.
        assert!(!Code::NoValidContract.is_retryable());
        assert!(!Code::EvseAlreadyReserved.is_retryable());
        assert!(!Code::UnauthorizedAccess.is_retryable());
        assert!(!Code::Success.is_retryable());
    }

    #[test]
    fn authorization_failures_are_classified_as_such() {
        assert!(Code::RfidInvalidUid.is_authorization_failure());
        assert!(Code::EvcoIdLocked.is_authorization_failure());
        assert!(Code::NoValidContract.is_authorization_failure());
        assert!(!Code::EvseOutOfService.is_authorization_failure());
        assert!(Code::EvseOutOfService.is_evse_problem());
    }

    #[test]
    fn a_status_carries_the_specs_own_description_by_default() {
        let status = StatusCode::new(Code::UnknownEvseId);
        assert_eq!(status.description.as_ref().unwrap().as_str(), "Unknown EVSE ID");
        assert!(status.validate().is_ok());
    }
}
