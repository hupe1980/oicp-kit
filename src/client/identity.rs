//! Mutual TLS, and the local check that turns a production `017` into a startup error.

use crate::transport::OicpError;
use crate::types::{OperatorId, ProviderId};

/// The client certificate and key Hubject issued for this partner.
///
/// # OICP has no tokens
///
/// There is no API key, no OAuth, no bearer. Authentication *is* the TLS client certificate, and
/// authorisation is Hubject comparing the `OperatorID`/`ProviderID` in the URL path against that
/// certificate:
///
/// > *Hubject compares the given Provider- or OperatorID to the partner's SSL client certificate
/// > information with every web service request. […] If Hubject detects a mismatch […] Hubject
/// > will not perform the operation and will respond with the status code 017 "Unauthorized
/// > Access".*
///
/// That failure arrives as a `017` on every request, with no indication of which of the two sides
/// is wrong. [`ClientIdentity::check_against`] does the comparison locally, at startup, and says
/// which — see [`IdentityWarning`].
#[derive(Clone)]
pub struct ClientIdentity {
    pem: Vec<u8>,
    subject: Option<String>,
    sans: Vec<String>,
}

impl core::fmt::Debug for ClientIdentity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Never print the key material.
        f.debug_struct("ClientIdentity")
            .field("subject", &self.subject)
            .field("sans", &self.sans)
            .finish_non_exhaustive()
    }
}

impl ClientIdentity {
    /// Loads a certificate chain and private key from one PEM buffer.
    ///
    /// # Errors
    ///
    /// Returns [`OicpError::Transport`] when the buffer holds no certificate, or no private key.
    pub fn from_pem(pem: impl Into<Vec<u8>>) -> Result<Self, OicpError> {
        use rustls_pki_types::pem::PemObject as _;
        use rustls_pki_types::{CertificateDer, PrivateKeyDer};

        let pem = pem.into();
        let certs: Vec<CertificateDer<'_>> =
            CertificateDer::pem_slice_iter(&pem).collect::<Result<_, _>>().map_err(|e| {
                OicpError::transport_from("the client certificate PEM could not be parsed", e)
            })?;
        if certs.is_empty() {
            return Err(OicpError::transport("the PEM buffer holds no certificate"));
        }
        // Hubject issues both halves and mutual TLS needs both. A PEM with only the certificate is
        // the commonest onboarding mistake, and `reqwest` reports it much later and less clearly.
        PrivateKeyDer::from_pem_slice(&pem).map_err(|_| {
            OicpError::transport(
                "the PEM buffer holds a certificate but no private key; OICP needs both for mutual TLS",
            )
        })?;
        let (subject, sans) = extract_names(certs.first().map_or(&[][..], |c| c.as_ref()));
        Ok(Self { pem, subject, sans })
    }

    /// Loads a certificate and key from a PEM file.
    ///
    /// # Errors
    ///
    /// Returns [`OicpError::Transport`] when the file cannot be read or does not hold both parts.
    pub fn from_pem_file(path: impl AsRef<std::path::Path>) -> Result<Self, OicpError> {
        let path = path.as_ref();
        let pem = std::fs::read(path)
            .map_err(|e| OicpError::transport_from(format!("{} could not be read", path.display()), e))?;
        Self::from_pem(pem)
    }

    /// The PEM bytes, for handing to the HTTP client.
    #[must_use]
    pub fn pem(&self) -> &[u8] {
        &self.pem
    }

    /// The certificate's subject, as far as it could be read.
    #[must_use]
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    /// The subject alternative names, as far as they could be read.
    #[must_use]
    pub fn subject_alternative_names(&self) -> &[String] {
        &self.sans
    }

    /// Checks that `id` appears somewhere in the certificate's names.
    ///
    /// Returns `None` when it does, or when the certificate's names could not be read at all —
    /// this is a helpful local check, not a security boundary, and refusing to start because a
    /// certificate encoding was unfamiliar would be worse than useless.
    #[must_use]
    pub fn check_against(&self, id: &str) -> Option<IdentityWarning> {
        if self.subject.is_none() && self.sans.is_empty() {
            return None;
        }
        let needle = canonical(id);
        let found = core::iter::once(self.subject.as_deref())
            .flatten()
            .chain(self.sans.iter().map(String::as_str))
            .any(|name| canonical(name).contains(&needle));
        if found {
            None
        } else {
            Some(IdentityWarning {
                id: id.to_owned(),
                subject: self.subject.clone(),
                sans: self.sans.clone(),
            })
        }
    }
}

/// Strips the separators that make `DE*ABC` and `DEABC` the same operator, and folds case.
fn canonical(s: &str) -> String {
    s.bytes().filter(|c| !matches!(c, b'*' | b'-' | b' ')).map(|c| c.to_ascii_uppercase() as char).collect()
}

/// Pulls the subject CN and the DNS SANs out of a DER certificate.
///
/// A deliberately shallow scan rather than a full X.509 parser: this exists to produce a helpful
/// message, so a certificate it cannot read costs nothing. Bringing an ASN.1 dependency into the
/// crate for a diagnostic would be the wrong trade.
fn extract_names(der: &[u8]) -> (Option<String>, Vec<String>) {
    let mut subject = None;
    let mut sans = vec![];
    // Printable/UTF8 strings inside the certificate that look like an OICP identifier.
    let mut i = 0;
    while i + 2 < der.len() {
        let tag = der[i];
        let len = der[i + 1] as usize;
        // 0x0c UTF8String, 0x13 PrintableString, 0x16 IA5String — the ones names live in.
        if matches!(tag, 0x0c | 0x13 | 0x16) && len > 0 && len < 128 && i + 2 + len <= der.len() {
            if let Ok(text) = core::str::from_utf8(&der[i + 2..i + 2 + len])
                && text.chars().all(|c| c.is_ascii_graphic() || c == ' ')
                && text.len() >= 3
            {
                if subject.is_none() {
                    subject = Some(text.to_owned());
                }
                sans.push(text.to_owned());
            }
            i += 2 + len;
        } else {
            i += 1;
        }
    }
    sans.dedup();
    (subject, sans)
}

/// The identifier configured does not appear in the client certificate.
///
/// Almost certainly [`Code::UnauthorizedAccess`](crate::types::Code::UnauthorizedAccess) waiting to
/// happen on every request. Reported rather than refused, because a **hub operator** legitimately
/// acts for bundled sub-partners whose ids are not in its certificate — the spec's hub-partner
/// chapter — and because a certificate this crate could not fully parse must not stop a partner
/// from working.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityWarning {
    /// The identifier that was configured.
    pub id: String,
    /// The certificate's subject, as far as it was read.
    pub subject: Option<String>,
    /// The names found in the certificate.
    pub sans: Vec<String>,
}

impl core::fmt::Display for IdentityWarning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "the configured identifier {:?} does not appear in the client certificate (names found: {}). \
             Hubject compares the two on every request and answers a mismatch with 017 Unauthorized \
             Access. This is legitimate only if you are a hub partner acting for a bundled sub-partner.",
            self.id,
            if self.sans.is_empty() { "none".to_owned() } else { self.sans.join(", ") }
        )
    }
}

/// Which party this client acts as, and with which identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartyId {
    /// A Charge Point Operator.
    Cpo(OperatorId),
    /// An e-Mobility Provider.
    Emp(ProviderId),
}

impl PartyId {
    /// The identifier as it goes into a URL path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Cpo(id) => id.as_str(),
            Self::Emp(id) => id.as_str(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pem_without_a_key_is_refused_with_a_useful_message() {
        let cert_only = b"-----BEGIN CERTIFICATE-----\nMIIBkTCB+w==\n-----END CERTIFICATE-----\n";
        let err = ClientIdentity::from_pem(cert_only.to_vec()).unwrap_err();
        assert!(err.to_string().contains("private key"), "{err}");
    }

    #[test]
    fn an_empty_pem_is_refused() {
        let err = ClientIdentity::from_pem(b"not a pem".to_vec()).unwrap_err();
        assert!(err.to_string().contains("no certificate"), "{err}");
    }

    #[test]
    fn identifiers_are_matched_ignoring_separators_and_case() {
        let identity = ClientIdentity {
            pem: vec![],
            subject: Some("DEABC Operator GmbH".into()),
            sans: vec!["de-abc.example.com".into()],
        };
        // The same operator, three spellings.
        assert!(identity.check_against("DE*ABC").is_none());
        assert!(identity.check_against("DEABC").is_none());
        assert!(identity.check_against("de*abc").is_none());
        // A different one.
        assert!(identity.check_against("DE*XYZ").is_some());
    }

    #[test]
    fn a_mismatch_explains_what_will_happen_and_when_it_is_legitimate() {
        let identity =
            ClientIdentity { pem: vec![], subject: Some("DEABC".into()), sans: vec!["DEABC".into()] };
        let warning = identity.check_against("DE*XYZ").unwrap();
        let message = warning.to_string();
        assert!(message.contains("017"));
        assert!(message.contains("hub partner"));
    }

    #[test]
    fn an_unreadable_certificate_produces_no_warning_rather_than_a_wrong_one() {
        let identity = ClientIdentity { pem: vec![], subject: None, sans: vec![] };
        assert!(identity.check_against("DE*ABC").is_none());
    }

    #[test]
    fn the_debug_output_never_contains_key_material() {
        let identity = ClientIdentity {
            pem: b"-----BEGIN PRIVATE KEY-----secret-----END PRIVATE KEY-----".to_vec(),
            subject: Some("DEABC".into()),
            sans: vec![],
        };
        let debug = format!("{identity:?}");
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("PRIVATE"));
    }
}
