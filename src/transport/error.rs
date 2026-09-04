//! `OicpError` — the four different ways an OICP call fails, kept apart.

use crate::types::{Acknowledgement, Code, StatusCode, Violations};

/// Why an OICP call did not produce a usable answer.
///
/// # Four failures, not one
///
/// OICP layers its failures, and collapsing them loses the information the caller needs to decide
/// what to do:
///
/// | Variant | What happened | Retry? |
/// |---|---|---|
/// | [`Transport`](Self::Transport) | the request never got an answer | yes, with backoff |
/// | [`Http`](Self::Http) | the answer was a non-2xx status | on 5xx |
/// | [`Rejected`](Self::Rejected) | `HTTP 200`, `Result: false` | ask the [`Code`] |
/// | [`Decode`](Self::Decode) | the answer was not the message it should be | no |
///
/// The third is the one that catches people out: Hubject answers a refused push with `HTTP 200`
/// and a body saying it did not happen. A client that only checks the status code believes the
/// push landed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OicpError {
    /// The request never reached Hubject, or its answer never came back.
    #[error("the request to Hubject failed: {message}")]
    Transport {
        /// What went wrong.
        message: String,
        /// The underlying error.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Hubject answered with a status outside 2xx.
    ///
    /// The URL is part of the error because the commonest cause of a `404` is a base URL without
    /// its `/api/oicp` path, and "Hubject answered 404" does not say that. An error a human reads
    /// should carry what they need to fix it.
    #[error("{url} answered {status}{}{}", body_excerpt(.body), path_hint(*.status, .url))]
    Http {
        /// The HTTP status.
        status: u16,
        /// The URL that was called.
        url: String,
        /// As much of the body as was read, for the log.
        body: String,
    },

    /// Hubject answered `HTTP 200` and said the operation did not happen.
    ///
    /// The most important variant: this is a **failure** that looks like a success at the HTTP
    /// layer. [`status_code`](Self::Rejected::status_code) says why.
    #[error("Hubject rejected the operation: {status_code}")]
    Rejected {
        /// Why it was rejected.
        status_code: StatusCode,
        /// The whole acknowledgement, so nothing is lost.
        acknowledgement: Box<Acknowledgement>,
    },

    /// The answer was not the message it was supposed to be.
    #[error("could not decode Hubject's answer{}: {message}", pointer_suffix(.pointer.as_deref()))]
    Decode {
        /// What went wrong.
        message: String,
        /// Where in the JSON, when `serde_path_to_error` could say.
        pointer: Option<String>,
    },

    /// A request this crate was asked to send is not conformant.
    ///
    /// Raised before anything goes on the wire — the "construct strictly" rule, enforced at the
    /// last possible moment for callers who built their request by hand.
    #[error("the request is not conformant: {0}")]
    Invalid(#[from] Violations),

    /// The request could not be addressed.
    #[error(transparent)]
    Endpoint(#[from] super::endpoint::EndpointError),
}

fn body_excerpt(body: &str) -> String {
    if body.is_empty() {
        String::new()
    } else {
        let trimmed: String = body.chars().take(200).collect();
        format!(": {trimmed}")
    }
}

fn pointer_suffix(pointer: Option<&str>) -> String {
    pointer.map_or_else(String::new, |p| format!(" at {p}"))
}

/// The one 404 worth guessing at: OICP's endpoints all hang off `…/api/oicp`, and a base URL
/// given without it produces a 404 on every call with nothing to say why.
fn path_hint(status: u16, url: &str) -> &'static str {
    if status == 404 && !url.contains("/api/oicp") {
        ". Every OICP endpoint hangs off `…/api/oicp`; a base URL without that path 404s on every call"
    } else {
        ""
    }
}

impl OicpError {
    /// Builds a [`Transport`](Self::Transport) error.
    pub fn transport(message: impl Into<String>) -> Self {
        Self::Transport { message: message.into(), source: None }
    }

    /// Builds a [`Transport`](Self::Transport) error with a cause.
    pub fn transport_from(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Transport { message: message.into(), source: Some(Box::new(source)) }
    }

    /// Builds a [`Rejected`](Self::Rejected) error from an acknowledgement that failed.
    #[must_use]
    pub fn rejected(acknowledgement: Acknowledgement) -> Self {
        Self::Rejected {
            status_code: acknowledgement.status_code.clone(),
            acknowledgement: Box::new(acknowledgement),
        }
    }

    /// The OICP status code, when the failure carried one.
    #[must_use]
    pub fn code(&self) -> Option<&Code> {
        match self {
            Self::Rejected { status_code, .. } => Some(&status_code.code),
            _ => None,
        }
    }

    /// Whether re-sending the identical request could plausibly succeed.
    ///
    /// * Transport failures: yes — the request may never have arrived.
    /// * HTTP 5xx and 429: yes. Other HTTP statuses: no.
    /// * A rejection: only for the transient codes — [`Code::is_retryable`].
    /// * Decode failures and invalid requests: no. The next attempt fails identically.
    ///
    /// [`RetryPolicy`](crate::client::RetryPolicy) is built on this.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport { .. } => true,
            Self::Http { status, .. } => *status >= 500 || *status == 429,
            Self::Rejected { status_code, .. } => status_code.code.is_retryable(),
            Self::Decode { .. } | Self::Invalid(_) | Self::Endpoint(_) => false,
        }
    }

    /// Whether this failure means the driver may not charge, as opposed to something breaking.
    #[must_use]
    pub fn is_authorization_failure(&self) -> bool {
        self.code().is_some_and(Code::is_authorization_failure)
    }
}

/// The result of an OICP call.
pub type Result<T> = core::result::Result<T, OicpError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_two_hundred_with_result_false_is_an_error() {
        let ack = Acknowledgement::failure(Code::UnknownEvseId);
        let err = OicpError::rejected(ack);
        assert_eq!(err.code(), Some(&Code::UnknownEvseId));
        assert!(!err.is_retryable());
        assert!(err.to_string().contains("603"));
    }

    #[test]
    fn retryability_follows_the_layer_the_failure_came_from() {
        assert!(OicpError::transport("connection reset").is_retryable());
        let http = |status| OicpError::Http { status, url: "https://x/api/oicp".into(), body: String::new() };
        assert!(http(503).is_retryable());
        assert!(http(429).is_retryable());
        assert!(!http(400).is_retryable());
        assert!(!OicpError::Decode { message: "bad".into(), pointer: None }.is_retryable());

        // Transient rejections are worth another go; decisions are not.
        assert!(OicpError::rejected(Acknowledgement::failure(Code::ServiceNotAvailable)).is_retryable());
        assert!(!OicpError::rejected(Acknowledgement::failure(Code::NoValidContract)).is_retryable());
    }

    #[test]
    fn an_authorization_failure_is_distinguishable_from_a_breakage() {
        let refused = OicpError::rejected(Acknowledgement::failure(Code::EvcoIdLocked));
        assert!(refused.is_authorization_failure());
        let broken = OicpError::rejected(Acknowledgement::failure(Code::HubjectSystemError));
        assert!(!broken.is_authorization_failure());
    }

    #[test]
    fn a_long_error_body_is_truncated_in_the_message() {
        let err = OicpError::Http { status: 500, url: "https://x/api/oicp".into(), body: "x".repeat(1000) };
        assert!(err.to_string().len() < 320, "{err}");
    }

    #[test]
    fn a_404_on_a_base_url_without_its_path_says_so() {
        // The commonest way to get a 404 out of Hubject is a base URL missing `/api/oicp`, and
        // "answered 404" is exactly as much help as no message at all.
        let missing = OicpError::Http {
            status: 404,
            url: "http://127.0.0.1:8080/evsepull/v23/providers/DE-DCB/data-records".into(),
            body: String::new(),
        };
        assert!(missing.to_string().contains("/api/oicp"), "{missing}");

        let genuine = OicpError::Http {
            status: 404,
            url: "https://service.hubject.com/api/oicp/evsepull/v23/x".into(),
            body: String::new(),
        };
        assert!(!genuine.to_string().contains("hangs off"), "{genuine}");

        // The hint is for 404 only; a 500 on the same URL is not a path problem.
        let server = OicpError::Http { status: 500, url: "http://x/y".into(), body: String::new() };
        assert!(!server.to_string().contains("hangs off"), "{server}");
    }
}
