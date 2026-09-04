//! `Identification` — how a driver proves who they are, in five mutually exclusive shapes.

use serde::{Deserialize, Serialize};

use super::builder::strict_builder;
use super::datetime::DateTime;
use super::ids::{EvcoId, Uid};
use super::text::Text;
use super::validate::{Validate, Validator, ViolationCode, validate_fields};
use crate::oicp_open_enum;

oicp_open_enum! {
    /// The RFID technology a card uses.
    pub enum RfidType {
        /// Mifare Classic.
        MifareCls = "mifareCls",
        /// Mifare DESFire.
        MifareDes = "mifareDes",
        /// Calypso.
        Calypso = "calypso",
        /// NFC.
        Nfc = "nfc",
        /// The Mifare family, unspecified.
        MifareFamily = "mifareFamily",
    }
}

oicp_open_enum! {
    /// The hash function used to protect a QR-code PIN.
    pub enum HashFunction {
        /// bcrypt — the only function the current spec allows for new data.
        Bcrypt = "Bcrypt",
    }
}

oicp_open_enum! {
    /// A hash function that is no longer acceptable for new data, kept for migration.
    ///
    /// The spec allows `MD5` and `SHA-1` **only** inside `LegacyHashData`, for PINs an EMP hashed
    /// before bcrypt was required. Neither is fit to protect a secret today, which is why they
    /// are a separate type from [`HashFunction`] rather than two more variants of it: a value of
    /// this type in a payload is, by construction, in the legacy slot.
    pub enum LegacyHashFunction {
        /// MD5. Broken; migration only.
        Md5 = "MD5",
        /// SHA-1. Broken; migration only.
        Sha1 = "SHA-1",
    }
}

/// A PIN hashed by the EMP, so the plaintext never crosses the roaming network.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct HashedPin {
    /// The hash value, 10 to 100 characters.
    #[serde(rename = "Value")]
    #[builder(into)]
    pub value: Text<100>,
    /// The function used to produce [`value`](Self::value).
    #[serde(rename = "Function")]
    pub function: HashFunction,
    /// A hash the partner produced with a superseded function, for migration.
    #[serde(rename = "LegacyHashData", default, skip_serializing_if = "Option::is_none")]
    pub legacy_hash_data: Option<LegacyHashData>,
}

impl Validate for HashedPin {
    fn validate_in(&self, v: &mut Validator) {
        let len = self.value.len();
        if !(10..=100).contains(&len) {
            v.report_at(
                "Value",
                if len < 10 { ViolationCode::TooShort } else { ViolationCode::TooLong },
                format!("a hashed PIN is 10 to 100 characters, not {len}"),
            );
        }
        validate_fields!(self, v, function as "Function", legacy_hash_data as "LegacyHashData");
    }
}

/// A PIN hash produced with a function the spec has since superseded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct LegacyHashData {
    /// The superseded function.
    #[serde(rename = "Function")]
    pub function: LegacyHashFunction,
    /// The salt the partner used.
    #[serde(rename = "Salt", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub salt: Option<Text<100>>,
    /// The hash value.
    #[serde(rename = "Value", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub value: Option<Text<20>>,
}

impl Validate for LegacyHashData {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, function as "Function", salt as "Salt", value as "Value");
    }
}

/// Identification by RFID card, giving only the card's UID.
///
/// The shape the spec tells you to use for RFID in the authorization process.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct RfidMifareFamilyIdentification {
    /// The card's unique identifier.
    #[serde(rename = "UID")]
    pub uid: Uid,
}

impl Validate for RfidMifareFamilyIdentification {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, uid as "UID");
    }
}

/// Identification by RFID card, with the contract and card metadata attached.
///
/// The spec restricts this shape to `PushAuthenticationData`: *"The option RFIDIdentification MUST
/// not be used in the eRoamingAuthorization process."*
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct RfidIdentification {
    /// The card's unique identifier.
    #[serde(rename = "UID")]
    pub uid: Uid,
    /// The contract the card belongs to.
    #[serde(rename = "EvcoID", default, skip_serializing_if = "Option::is_none")]
    pub evco_id: Option<EvcoId>,
    /// The card technology.
    #[serde(rename = "RFID")]
    pub rfid: RfidType,
    /// A number printed on the card, for manual authorization via a call centre.
    #[serde(rename = "PrintedNumber", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub printed_number: Option<Text<150>>,
    /// When the card stops being valid. Absent if it does not expire.
    #[serde(rename = "ExpiryDate", default, skip_serializing_if = "Option::is_none")]
    pub expiry_date: Option<DateTime>,
}

impl Validate for RfidIdentification {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(
            self,
            v,
            uid as "UID",
            evco_id as "EvcoID",
            rfid as "RFID",
            printed_number as "PrintedNumber",
            expiry_date as "ExpiryDate",
        );
    }
}

/// Identification by QR code or app, with an optional hashed PIN.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct QrCodeIdentification {
    /// The contract being presented.
    #[serde(rename = "EvcoID")]
    pub evco_id: EvcoId,
    /// The PIN, hashed by the EMP.
    #[serde(rename = "HashedPIN", default, skip_serializing_if = "Option::is_none")]
    pub hashed_pin: Option<HashedPin>,
    /// The PIN, in plaintext, 0 to 20 characters.
    ///
    /// # Which of the two to send is not a preference
    ///
    /// The specification splits them by process, in opposite directions:
    ///
    /// > *`HashedPIN`: […] This field can be provided only when uploading Authentication data. In
    /// > Authorization requests this field must be null!*
    /// >
    /// > *`PIN`: The pin number, this field is required in Authorization requests!*
    ///
    /// So an authorization request carries the **plaintext** PIN and no hash, and a
    /// `PushAuthenticationData` upload carries the hash. Sending the hash to authorize is the
    /// instinct, and is what the specification forbids in so many words.
    /// [`Identification::validate_in_process`] checks both directions; a context-free `validate`
    /// cannot, because the same object is right in one message and wrong in the other.
    #[serde(rename = "PIN", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub pin: Option<String>,
}

impl Validate for QrCodeIdentification {
    fn validate_in(&self, v: &mut Validator) {
        if self.pin.is_some() && self.hashed_pin.is_some() {
            v.report(
                ViolationCode::Inconsistent,
                "a QR-code identification carries both a plaintext PIN and a hashed one; \
                 the specification asks for exactly one, and which one depends on the message",
            );
        }
        if let Some(pin) = &self.pin {
            let len = pin.chars().count();
            if len > 20 {
                v.report_at(
                    "PIN",
                    ViolationCode::TooLong,
                    format!("a PIN is at most 20 characters, not {len}"),
                );
            }
        }
        validate_fields!(self, v, evco_id as "EvcoID", hashed_pin as "HashedPIN");
    }
}

/// Identification by ISO 15118 Plug & Charge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PlugAndChargeIdentification {
    /// The contract presented by the vehicle.
    #[serde(rename = "EvcoID")]
    pub evco_id: EvcoId,
}

impl Validate for PlugAndChargeIdentification {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, evco_id as "EvcoID");
    }
}

/// Identification for a remotely started session.
///
/// The spec requires this shape, and only this shape, in the remote authorization process.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct RemoteIdentification {
    /// The contract on whose behalf the session is started.
    #[serde(rename = "EvcoID")]
    pub evco_id: EvcoId,
}

impl Validate for RemoteIdentification {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, evco_id as "EvcoID");
    }
}

/// Which process an [`Identification`] appears in, for the rules that depend on it.
///
/// The spec's constraints on `Identification` are **contextual** — the same object is legal in one
/// message and forbidden in another:
///
/// > *1. The option RFIDIdentification MUST not be used in the eRoamingAuthorization process. For
/// >    RFID Authorization, only the option RFIDMifareFamilyIdentification SHOULD be used […]*
/// > *2. For the Remote Authorization process, only the option RemoteIdentification MUST be used
/// >    in the respective messages.*
///
/// …and a third, on the QR-code variant's two PIN fields:
///
/// > *`HashedPIN`: […] can be provided only when uploading Authentication data. In Authorization
/// > requests this field must be null!* — *`PIN`: […] required in Authorization requests!*
///
/// A validator that does not know the context cannot check any of them, so the wire types pass
/// their context to [`Identification::validate_in_process`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IdentificationProcess {
    /// `AuthorizeStart` / `AuthorizeStop` — a driver at the charging point.
    Authorization,
    /// `AuthorizeRemoteStart` / `AuthorizeRemoteStop` and the reservation equivalents.
    RemoteAuthorization,
    /// `PushAuthenticationData` — an EMP uploading its card base.
    AuthenticationData,
    /// A CDR or a charging notification: a record of what happened, not a decision.
    Record,
}

/// How a driver identified themselves: exactly one of five shapes.
///
/// # Why this is a Rust enum and not a struct of five options
///
/// OICP models this as an object with five optional members, and the examples in Hubject's own
/// OpenAPI documents fill in **all five at once** — which is not a thing any real payload does.
/// The wire shape stays exactly that (one member, the others absent), but the Rust type is a
/// closed choice, so "which identification is this?" is a `match` rather than a chain of
/// `if let Some(…)` with an unreachable fallback.
///
/// A payload that really does carry more than one member decodes into the first present variant
/// in spec order — which is what Hubject itself acts on — and the surplus members are dropped.
/// That is the one place in this crate where data does not survive a round trip, and it is
/// deliberate: an `Identification` naming two different drivers has no faithful representation,
/// and forwarding the ambiguity would move a billing dispute downstream. Use
/// [`Identification::from_wire`] when you need to know that it happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Identification {
    /// An RFID card, by UID only. Use this one for RFID authorization.
    RfidMifareFamily(RfidMifareFamilyIdentification),
    /// An RFID card with contract and metadata. `PushAuthenticationData` only.
    Rfid(RfidIdentification),
    /// A QR code or app.
    QrCode(QrCodeIdentification),
    /// ISO 15118 Plug & Charge.
    PlugAndCharge(PlugAndChargeIdentification),
    /// A remotely started session. Required in the remote authorization process.
    Remote(RemoteIdentification),
}

impl Identification {
    /// The wire name of the member this variant serialises to.
    #[must_use]
    pub const fn member_name(&self) -> &'static str {
        match self {
            Self::RfidMifareFamily(_) => "RFIDMifareFamilyIdentification",
            Self::Rfid(_) => "RFIDIdentification",
            Self::QrCode(_) => "QRCodeIdentification",
            Self::PlugAndCharge(_) => "PlugAndChargeIdentification",
            Self::Remote(_) => "RemoteIdentification",
        }
    }

    /// The contract this identification names, if it names one.
    ///
    /// `None` for a bare RFID UID, which is exactly the case where the CPO does not yet know whose
    /// card it is and must ask Hubject.
    #[must_use]
    pub fn evco_id(&self) -> Option<&EvcoId> {
        match self {
            Self::RfidMifareFamily(_) => None,
            Self::Rfid(r) => r.evco_id.as_ref(),
            Self::QrCode(q) => Some(&q.evco_id),
            Self::PlugAndCharge(p) => Some(&p.evco_id),
            Self::Remote(r) => Some(&r.evco_id),
        }
    }

    /// The RFID UID this identification names, if it names one.
    #[must_use]
    pub fn uid(&self) -> Option<&Uid> {
        match self {
            Self::RfidMifareFamily(r) => Some(&r.uid),
            Self::Rfid(r) => Some(&r.uid),
            _ => None,
        }
    }

    /// Decodes the wire shape, reporting every member that was present.
    ///
    /// [`Deserialize`] keeps the first member in spec order and discards the rest, because that is
    /// what Hubject acts on. This is the same decode, but it hands back the names of *all* the
    /// members the payload carried, so a conformance run — or a hub that wants to refuse an
    /// ambiguous request rather than guess — can see the ambiguity.
    ///
    /// # Errors
    ///
    /// Returns the `serde_json` error when the value is not an `Identification` object at all.
    pub fn from_wire(value: &serde_json::Value) -> Result<(Self, Vec<&'static str>), serde_json::Error> {
        let wire: IdentificationWire = serde_json::from_value(value.clone())?;
        let present: Vec<&'static str> = [
            wire.rfid_mifare_family.is_some().then_some("RFIDMifareFamilyIdentification"),
            wire.rfid.is_some().then_some("RFIDIdentification"),
            wire.qr_code.is_some().then_some("QRCodeIdentification"),
            wire.plug_and_charge.is_some().then_some("PlugAndChargeIdentification"),
            wire.remote.is_some().then_some("RemoteIdentification"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let chosen: Self = serde_json::from_value(value.clone())?;
        Ok((chosen, present))
    }

    /// Checks the rules that depend on which message this identification appears in.
    ///
    /// See [`IdentificationProcess`] for the two rules and where they come from.
    pub fn validate_in_process(&self, v: &mut Validator, process: IdentificationProcess) {
        self.validate_in(v);
        match (process, self) {
            (IdentificationProcess::Authorization, Self::Rfid(_)) => v.report(
                ViolationCode::Inconsistent,
                "RFIDIdentification MUST NOT be used in the eRoamingAuthorization process; \
                 use RFIDMifareFamilyIdentification",
            ),
            (IdentificationProcess::Authorization, Self::QrCode(qr)) => {
                // Two segments, so two `enter`s: a "/" inside one is escaped to `~1` by RFC 6901
                // and would point at a field nobody has.
                v.enter("QRCodeIdentification");
                if qr.hashed_pin.is_some() {
                    v.report_at(
                        "HashedPIN",
                        ViolationCode::Inconsistent,
                        "HashedPIN must be null in an authorization request; it is for uploading \
                         authentication data. Send the plaintext PIN here",
                    );
                }
                if qr.pin.is_none() {
                    v.report_at(
                        "PIN",
                        ViolationCode::MissingConditional,
                        "PIN is required in an authorization request",
                    );
                }
                v.leave();
            }
            (IdentificationProcess::RemoteAuthorization, other) if !matches!(other, Self::Remote(_)) => v
                .report(
                    ViolationCode::Inconsistent,
                    format!(
                        "only RemoteIdentification MUST be used in the remote authorization process, \
                     but this is a {}",
                        other.member_name()
                    ),
                ),
            _ => {}
        }
    }
}

impl Validate for Identification {
    fn validate_in(&self, v: &mut Validator) {
        match self {
            Self::RfidMifareFamily(x) => v.field("RFIDMifareFamilyIdentification", x),
            Self::Rfid(x) => v.field("RFIDIdentification", x),
            Self::QrCode(x) => v.field("QRCodeIdentification", x),
            Self::PlugAndCharge(x) => v.field("PlugAndChargeIdentification", x),
            Self::Remote(x) => v.field("RemoteIdentification", x),
        }
    }
}

/// The wire shape: an object with five optional members.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct IdentificationWire {
    #[serde(rename = "RFIDMifareFamilyIdentification", default, skip_serializing_if = "Option::is_none")]
    rfid_mifare_family: Option<RfidMifareFamilyIdentification>,
    #[serde(rename = "RFIDIdentification", default, skip_serializing_if = "Option::is_none")]
    rfid: Option<RfidIdentification>,
    #[serde(rename = "QRCodeIdentification", default, skip_serializing_if = "Option::is_none")]
    qr_code: Option<QrCodeIdentification>,
    #[serde(rename = "PlugAndChargeIdentification", default, skip_serializing_if = "Option::is_none")]
    plug_and_charge: Option<PlugAndChargeIdentification>,
    #[serde(rename = "RemoteIdentification", default, skip_serializing_if = "Option::is_none")]
    remote: Option<RemoteIdentification>,
}

impl Serialize for Identification {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut wire = IdentificationWire {
            rfid_mifare_family: None,
            rfid: None,
            qr_code: None,
            plug_and_charge: None,
            remote: None,
        };
        match self.clone() {
            Self::RfidMifareFamily(x) => wire.rfid_mifare_family = Some(x),
            Self::Rfid(x) => wire.rfid = Some(x),
            Self::QrCode(x) => wire.qr_code = Some(x),
            Self::PlugAndCharge(x) => wire.plug_and_charge = Some(x),
            Self::Remote(x) => wire.remote = Some(x),
        }
        wire.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Identification {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = IdentificationWire::deserialize(d)?;
        // Spec order. A payload with several members present keeps the first; `Validate` on the
        // containing message reports the others.
        if let Some(x) = wire.rfid_mifare_family {
            return Ok(Self::RfidMifareFamily(x));
        }
        if let Some(x) = wire.rfid {
            return Ok(Self::Rfid(x));
        }
        if let Some(x) = wire.qr_code {
            return Ok(Self::QrCode(x));
        }
        if let Some(x) = wire.plug_and_charge {
            return Ok(Self::PlugAndCharge(x));
        }
        if let Some(x) = wire.remote {
            return Ok(Self::Remote(x));
        }
        Err(serde::de::Error::custom(
            "an Identification must carry one of RFIDMifareFamilyIdentification, RFIDIdentification, \
             QRCodeIdentification, PlugAndChargeIdentification or RemoteIdentification",
        ))
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Identification {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Identification".into()
    }
    fn json_schema(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        IdentificationWire::json_schema(g)
    }
}

impl From<Uid> for Identification {
    fn from(uid: Uid) -> Self {
        Self::RfidMifareFamily(RfidMifareFamilyIdentification { uid })
    }
}

// Every builder's `build()` validates; `build_unchecked()` is the escape hatch.
strict_builder!(HashedPin, HashedPinBuilder, hashed_pin_builder);
strict_builder!(LegacyHashData, LegacyHashDataBuilder, legacy_hash_data_builder);
strict_builder!(
    RfidMifareFamilyIdentification,
    RfidMifareFamilyIdentificationBuilder,
    rfid_mifare_family_identification_builder
);
strict_builder!(RfidIdentification, RfidIdentificationBuilder, rfid_identification_builder);
strict_builder!(QrCodeIdentification, QrCodeIdentificationBuilder, qr_code_identification_builder);
strict_builder!(
    PlugAndChargeIdentification,
    PlugAndChargeIdentificationBuilder,
    plug_and_charge_identification_builder
);
strict_builder!(RemoteIdentification, RemoteIdentificationBuilder, remote_identification_builder);

#[cfg(test)]
mod tests {
    use super::*;

    fn qr(pin: Option<&str>, hashed: Option<HashedPin>) -> Identification {
        Identification::QrCode(QrCodeIdentification {
            evco_id: "DE-8EO-CAet5e4XY-3".parse().unwrap(),
            hashed_pin: hashed,
            pin: pin.map(ToOwned::to_owned),
        })
    }

    fn hashed() -> HashedPin {
        HashedPin {
            value: Text::new("0123456789abcdef").unwrap(),
            function: HashFunction::Bcrypt,
            legacy_hash_data: None,
        }
    }

    fn violations(id: &Identification, process: IdentificationProcess) -> Vec<(String, ViolationCode)> {
        let mut v = Validator::new();
        id.validate_in_process(&mut v, process);
        v.finish().into_vec().into_iter().map(|x| (x.pointer, x.code)).collect()
    }

    #[test]
    fn an_authorization_request_carries_the_plaintext_pin_and_not_the_hash() {
        // The specification is explicit and counter-intuitive: "In Authorization requests this
        // field must be null!" of HashedPIN, and "required in Authorization requests" of PIN.
        let found = violations(&qr(None, Some(hashed())), IdentificationProcess::Authorization);
        assert!(
            found.contains(&("/QRCodeIdentification/HashedPIN".to_owned(), ViolationCode::Inconsistent)),
            "{found:?}"
        );
        assert!(
            found.contains(&("/QRCodeIdentification/PIN".to_owned(), ViolationCode::MissingConditional)),
            "{found:?}"
        );

        assert!(violations(&qr(Some("1234"), None), IdentificationProcess::Authorization).is_empty());
    }

    #[test]
    fn uploading_authentication_data_is_the_other_way_round() {
        // The same object, the other message: a hash is what belongs here, and no rule fires.
        assert!(violations(&qr(None, Some(hashed())), IdentificationProcess::AuthenticationData).is_empty());
    }

    #[test]
    fn a_pin_longer_than_the_field_is_reported() {
        let found = violations(&qr(Some(&"9".repeat(21)), None), IdentificationProcess::Authorization);
        assert!(
            found.contains(&("/QRCodeIdentification/PIN".to_owned(), ViolationCode::TooLong)),
            "{found:?}"
        );
    }

    #[test]
    fn an_empty_hashed_pin_is_too_short_rather_than_acceptable() {
        let empty = HashedPin { value: Text::new("").unwrap(), ..hashed() };
        let err = empty.validate().unwrap_err();
        assert_eq!(err.as_slice()[0].code, ViolationCode::TooShort);
    }

    #[test]
    fn one_member_in_one_member_out() {
        let json = r#"{"RFIDMifareFamilyIdentification":{"UID":"7568290FFF765F"}}"#;
        let id: Identification = serde_json::from_str(json).unwrap();
        assert!(matches!(id, Identification::RfidMifareFamily(_)));
        assert_eq!(serde_json::to_string(&id).unwrap(), json);
        assert_eq!(id.uid().unwrap().as_str(), "7568290FFF765F");
        assert!(id.evco_id().is_none());
    }

    #[test]
    fn an_empty_identification_is_a_decode_error() {
        assert!(serde_json::from_str::<Identification>("{}").is_err());
    }

    #[test]
    fn the_remote_process_rule_is_enforced_with_context() {
        let remote =
            Identification::Remote(RemoteIdentification { evco_id: "DE-DCB-C12345678-X".parse().unwrap() });
        let rfid = Identification::RfidMifareFamily(RfidMifareFamilyIdentification {
            uid: "7568290FFF765F".parse().unwrap(),
        });

        let mut v = Validator::new();
        remote.validate_in_process(&mut v, IdentificationProcess::RemoteAuthorization);
        assert!(v.finish().is_empty());

        let mut v = Validator::new();
        rfid.validate_in_process(&mut v, IdentificationProcess::RemoteAuthorization);
        assert_eq!(v.finish().len(), 1, "an RFID identification is illegal in a remote start");

        // …but the same object is perfectly fine in the authorization process.
        let mut v = Validator::new();
        rfid.validate_in_process(&mut v, IdentificationProcess::Authorization);
        assert!(v.finish().is_empty());
    }

    #[test]
    fn rfid_identification_is_rejected_in_the_authorization_process() {
        let id = Identification::Rfid(RfidIdentification {
            uid: "7568290FFF765F".parse().unwrap(),
            evco_id: None,
            rfid: RfidType::MifareCls,
            printed_number: None,
            expiry_date: None,
        });
        let mut v = Validator::new();
        id.validate_in_process(&mut v, IdentificationProcess::Authorization);
        assert_eq!(v.finish().len(), 1);

        // Legal where the spec allows it.
        let mut v = Validator::new();
        id.validate_in_process(&mut v, IdentificationProcess::AuthenticationData);
        assert!(v.finish().is_empty());
    }

    #[test]
    fn a_plaintext_pin_alongside_a_hashed_one_is_reported() {
        let json = r#"{"QRCodeIdentification":{"EvcoID":"DE-DCB-C12345678-X","HashedPIN":{"Value":"a5ghdhf73h","Function":"Bcrypt"},"PIN":"1234"}}"#;
        let id: Identification = serde_json::from_str(json).unwrap();
        assert_eq!(id.validate().unwrap_err().len(), 1);
    }
}
