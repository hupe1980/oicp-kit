//! The HTTP plumbing: one POST, with retries, and the error mapping that makes 200 mean success.

use core::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::identity::{ClientIdentity, IdentityWarning};
use super::retry::RetryPolicy;
use crate::transport::{HubjectEnv, OicpError, Operation, PathId, Result};
use crate::types::{Acknowledgement, Validate};

/// How a client talks to Hubject.
#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// Which brokering system.
    pub environment: HubjectEnv,
    /// How long to wait for one request.
    pub timeout: Duration,
    /// When to try again.
    pub retry: RetryPolicy,
    /// Check every outgoing request against the spec before sending it.
    ///
    /// On by default. A non-conformant request is refused locally — with a JSON Pointer to the
    /// offending field — rather than by Hubject with `022 Data error` and no detail.
    pub validate_requests: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            environment: HubjectEnv::Qa,
            timeout: Duration::from_secs(30),
            retry: RetryPolicy::default(),
            validate_requests: true,
        }
    }
}

/// The shared HTTP machinery behind [`CpoClient`](super::CpoClient) and
/// [`EmpClient`](super::EmpClient).
///
/// Public so a partner can add an operation this crate has not modelled, or drive one against a
/// [`MockHubject`](crate::testkit::MockHubject) over HTTP.
#[derive(Clone)]
pub struct Transport {
    http: reqwest::Client,
    config: ClientConfig,
}

impl core::fmt::Debug for Transport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Transport").field("config", &self.config).finish_non_exhaustive()
    }
}

impl Transport {
    /// Builds a transport with mutual TLS.
    ///
    /// # Errors
    ///
    /// Returns [`OicpError::Transport`] when the identity cannot be turned into a TLS client
    /// certificate, or the HTTP client cannot be built.
    pub fn new(config: ClientConfig, identity: Option<&ClientIdentity>) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .timeout(config.timeout)
            .use_rustls_tls()
            .tls_built_in_root_certs(true)
            .user_agent(concat!("oicp-kit/", env!("CARGO_PKG_VERSION")));

        if let Some(identity) = identity {
            let pem = reqwest::Identity::from_pem(identity.pem()).map_err(|e| {
                OicpError::transport_from(
                    "the client certificate and key could not be used for TLS; OICP requires both \
                     in one PEM",
                    e,
                )
            })?;
            builder = builder.identity(pem);
        }

        let http = builder
            .build()
            .map_err(|e| OicpError::transport_from("the HTTP client could not be built", e))?;
        Ok(Self { http, config })
    }

    /// The configuration.
    #[must_use]
    pub const fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Whether this transport points at the production brokering system.
    #[must_use]
    pub const fn is_production(&self) -> bool {
        self.config.environment.is_production()
    }

    /// POSTs `body` to `operation` and decodes the answer as `T`.
    ///
    /// # Errors
    ///
    /// See [`OicpError`]. In particular, a `200` carrying `Result: false` is
    /// [`OicpError::Rejected`], not a success.
    pub async fn post<B, T>(&self, operation: Operation, id: &PathId, body: &B) -> Result<T>
    where
        B: Serialize + Validate,
        T: DeserializeOwned,
    {
        if self.config.validate_requests {
            body.validate()?;
        }
        let url = operation.url(self.config.environment.base_url(), id)?;
        self.post_raw(operation, &url, body).await
    }

    /// POSTs `body` to `operation` at `url` — for a paginated call, whose URL carries a query.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn post_raw<B, T>(&self, operation: Operation, url: &str, body: &B) -> Result<T>
    where
        B: Serialize,
        T: DeserializeOwned,
    {
        let mut attempt = 0;
        loop {
            match self.attempt(url, body).await {
                Ok(value) => return Ok(value),
                Err(error) => {
                    // `should_retry` weighs the failure *and* the operation: a lost remote start is
                    // retryable at the transport layer but must not be repeated, because the first
                    // one may have started a session.
                    if self.config.retry.should_retry(operation, &error, attempt).is_none() {
                        return Err(error);
                    }
                    let wait = self.config.retry.jittered_backoff(attempt, super::retry::random_permille());
                    tracing::debug!(
                        target: "oicp_kit::client",
                        ?operation, attempt, ?wait, %error,
                        "retrying after a retryable failure"
                    );
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn attempt<B, T>(&self, url: &str, body: &B) -> Result<T>
    where
        B: Serialize,
        T: DeserializeOwned,
    {
        let response = self
            .http
            .post(url)
            .json(body)
            .send()
            .await
            .map_err(|e| OicpError::transport_from(format!("POST {url} failed"), e))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| OicpError::transport_from(format!("the body of {url} could not be read"), e))?;

        if !status.is_success() {
            return Err(OicpError::Http { status: status.as_u16(), url: url.to_owned(), body: text });
        }

        // Hubject answers a refused operation with 200 and Result: false. Check that before
        // decoding into `T`, so a caller can never mistake a refusal for a success.
        if let Ok(ack) = serde_json::from_str::<Acknowledgement>(&text)
            && !ack.is_success()
        {
            return Err(OicpError::rejected(ack));
        }

        decode(&text)
    }
}

/// Decodes `text` as `T`, reporting where in the JSON it went wrong.
fn decode<T: DeserializeOwned>(text: &str) -> Result<T> {
    let deserializer = &mut serde_json::Deserializer::from_str(text);
    serde_path_to_error::deserialize(deserializer).map_err(|e| OicpError::Decode {
        pointer: Some(e.path().to_string()),
        message: e.into_inner().to_string(),
    })
}

/// Checks a party identifier against a client certificate, for the client builders.
pub(crate) fn warn_on_identity_mismatch(
    identity: Option<&ClientIdentity>,
    id: &str,
) -> Option<IdentityWarning> {
    let warning = identity?.check_against(id);
    if let Some(warning) = &warning {
        tracing::warn!(target: "oicp_kit::client", "{warning}");
    }
    warning
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decode_failure_says_where_in_the_json_it_happened() {
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Outer {
            inner: Inner,
        }
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Inner {
            value: u32,
        }
        let Err(err) = decode::<Outer>(r#"{"inner":{"value":"not a number"}}"#) else {
            panic!("a string is not a u32")
        };
        let OicpError::Decode { pointer, .. } = &err else { panic!("expected a decode error: {err}") };
        assert_eq!(pointer.as_deref(), Some("inner.value"));
    }

    #[test]
    fn the_default_configuration_points_at_qa_not_production() {
        let config = ClientConfig::default();
        assert!(!config.environment.is_production(), "a default that pushes to production is a trap");
        assert!(config.validate_requests);
    }
}
