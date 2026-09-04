//! `MockHubjectServer` — the broker behind a real socket, at the real OICP paths.
//!
//! [`MockHubject`] alone tests your *logic*. This tests your *integration*: the URL the client
//! builds, the query string it appends, the way it reads a page, what it does with a `200` that
//! says `Result: false`, whether the crawl follows `last` correctly. Those live in
//! [`client`](crate::client) and cannot be exercised in-process.

use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::post;

use super::mock::MockHubject;
use crate::cpo::{
    AuthorizeStartRequest, AuthorizeStopRequest, ChargeDetailRecord, ChargingNotification,
    PushEvseDataRequest, PushEvseStatusRequest,
};
use crate::emp::{PullEvseDataRequest, PullEvseStatusByIdRequest, PullEvseStatusRequest};
use crate::transport::{Operation, PageQuery};
use crate::types::{Acknowledgement, Code};

/// A [`MockHubject`] served over HTTP at the paths OICP defines.
///
/// # What this is for
///
/// Point a real [`CpoClient`](crate::client::CpoClient) or
/// [`EmpClient`](crate::client::EmpClient) at it — no certificates, no contract, no QA
/// environment — and the whole client path runs: URL construction from the endpoint table, the
/// `?page=&size=` query, decoding a page, turning a `Result: false` into an error, following a
/// crawl to the last page.
///
/// ```no_run
/// # use oicp_kit::client::EmpClient;
/// # use oicp_kit::testkit::{MockHubject, MockEmp, MockHubjectServer, samples};
/// # use oicp_kit::transport::{HubjectEnv, PageQuery};
/// # use oicp_kit::emp::PullEvseDataRequest;
/// # use oicp_kit::types::GeoCoordinatesFormat;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mut hubject = MockHubject::new();
/// hubject.register_emp(MockEmp::permissive("DE-DCB".parse()?));
/// hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())?;
///
/// let server = MockHubjectServer::start(hubject).await?;
///
/// let client = EmpClient::builder()
///     .environment(HubjectEnv::Custom(server.base_url().to_owned()))
///     .provider_id("DE-DCB".parse()?)
///     .build()?;
///
/// let request = PullEvseDataRequest::full("DE-DCB".parse()?, GeoCoordinatesFormat::Google);
/// let page = client.pull_evse_data_page(&request, PageQuery::new()).await?;
/// assert_eq!(page.content.len(), 1);
/// # Ok(())
/// # }
/// ```
///
/// # It speaks plain HTTP
///
/// The real brokering system requires mutual TLS. This does not: terminating TLS in a test would
/// mean shipping a certificate authority, and what is being tested is the OICP layer above it.
/// [`ClientIdentity`](crate::client::ClientIdentity) has its own tests for the certificate work.
pub struct MockHubjectServer {
    hubject: MockHubject,
    base_url: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl MockHubjectServer {
    /// Starts the broker on an ephemeral port.
    ///
    /// # Errors
    ///
    /// Returns an IO error when the listener cannot be bound.
    pub async fn start(hubject: MockHubject) -> std::io::Result<Self> {
        Self::bind(hubject, "127.0.0.1:0").await
    }

    /// Starts the broker on `address`.
    ///
    /// # Errors
    ///
    /// Returns an IO error when the listener cannot be bound.
    pub async fn bind(hubject: MockHubject, address: &str) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind(address).await?;
        let local = listener.local_addr()?;
        let router = router(hubject.clone());
        let (shutdown, signal) = tokio::sync::oneshot::channel();

        let handle = tokio::spawn(async move {
            let served = axum::serve(listener, router).with_graceful_shutdown(async {
                let _ = signal.await;
            });
            if let Err(error) = served.await {
                tracing::error!(target: "oicp_kit::testkit", %error, "the mock broker stopped");
            }
        });

        Ok(Self {
            hubject,
            base_url: format!("http://{local}/api/oicp"),
            shutdown: Some(shutdown),
            handle: Some(handle),
        })
    }

    /// The base URL to point a client at.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The environment to configure a client with.
    #[must_use]
    pub fn environment(&self) -> crate::transport::HubjectEnv {
        crate::transport::HubjectEnv::Custom(self.base_url.clone())
    }

    /// The broker behind the socket, for asserting on what happened.
    #[must_use]
    pub const fn hubject(&self) -> &MockHubject {
        &self.hubject
    }

    /// Stops the server and waits for it to finish.
    pub async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

impl Drop for MockHubjectServer {
    fn drop(&mut self) {
        // A test that forgets `stop()` must not leak the task.
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

impl core::fmt::Debug for MockHubjectServer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MockHubjectServer").field("base_url", &self.base_url).finish_non_exhaustive()
    }
}

/// The routes the real brokering system exposes, mounted under `/api/oicp`.
fn router(hubject: MockHubject) -> axum::Router {
    let paths = axum::Router::new()
        .route(
            Operation::PushEvseData.path_template(),
            post(|State(h): State<MockHubject>, Json(body): Json<serde_json::Value>| async move {
                acknowledge(decode::<PushEvseDataRequest>(body).map(|r| h.push_evse_data(&r)))
            }),
        )
        .route(
            Operation::PushEvseStatus.path_template(),
            post(|State(h): State<MockHubject>, Json(body): Json<serde_json::Value>| async move {
                acknowledge(decode::<PushEvseStatusRequest>(body).map(|r| h.push_evse_status(&r)))
            }),
        )
        .route(
            Operation::PullEvseData.path_template(),
            post(
                |State(h): State<MockHubject>,
                 Query(query): Query<PageQuery>,
                 Json(body): Json<serde_json::Value>| async move {
                    match decode::<PullEvseDataRequest>(body) {
                        Err(refusal) => json(&refusal),
                        Ok(request) => json(&h.pull_evse_data(&request, query)),
                    }
                },
            ),
        )
        .route(
            Operation::PullEvseStatus.path_template(),
            post(|State(h): State<MockHubject>, Json(body): Json<serde_json::Value>| async move {
                // The three status pulls share one endpoint and are told apart by their body.
                if let Ok(request) = serde_json::from_value::<PullEvseStatusByIdRequest>(body.clone()) {
                    return json(&h.pull_evse_status_by_id(&request));
                }
                match decode::<PullEvseStatusRequest>(body) {
                    Err(refusal) => json(&refusal),
                    Ok(request) => json(&h.pull_evse_status(&request)),
                }
            }),
        )
        .route(
            Operation::AuthorizeStart.path_template(),
            post(|State(h): State<MockHubject>, Json(body): Json<serde_json::Value>| async move {
                match decode::<AuthorizeStartRequest>(body) {
                    Err(refusal) => json(&refusal),
                    Ok(request) => json(&h.authorize_start(&request)),
                }
            }),
        )
        .route(
            Operation::AuthorizeStop.path_template(),
            post(|State(h): State<MockHubject>, Json(body): Json<serde_json::Value>| async move {
                match decode::<AuthorizeStopRequest>(body) {
                    Err(refusal) => json(&refusal),
                    Ok(request) => json(&h.authorize_stop(&request)),
                }
            }),
        )
        .route(
            Operation::ChargeDetailRecord.path_template(),
            post(|State(h): State<MockHubject>, Json(body): Json<serde_json::Value>| async move {
                acknowledge(decode::<ChargeDetailRecord>(body).map(|cdr| h.submit_cdr(&cdr)))
            }),
        )
        .route(
            Operation::ChargingNotifications.path_template(),
            post(|State(h): State<MockHubject>, Json(body): Json<serde_json::Value>| async move {
                acknowledge(decode::<ChargingNotification>(body).map(|n| h.notify(&n)))
            }),
        )
        .with_state(hubject);

    axum::Router::new().nest("/api/oicp", paths)
}

/// Decodes a request body, or the acknowledgement to refuse it with.
///
/// Hubject answers a malformed push with `HTTP 400` and a description, not with `Result: false` —
/// so the mock does the same, and a client that only handles the acknowledgement path is caught.
fn decode<T: serde::de::DeserializeOwned>(body: serde_json::Value) -> Result<T, Acknowledgement> {
    serde_path_to_error::deserialize(body)
        .map_err(|e| Acknowledgement::failure_with(Code::DataError, format!("{} at {}", e.inner(), e.path())))
}

fn acknowledge(outcome: Result<Result<Acknowledgement, Acknowledgement>, Acknowledgement>) -> Response {
    match outcome {
        // A body that is not the message it should be: the real broker answers 400.
        Err(refusal) => (axum::http::StatusCode::BAD_REQUEST, Json(refusal)).into_response(),
        // An operation the broker refused: 200, with `Result: false`.
        Ok(Err(refusal)) => json(&refusal),
        Ok(Ok(ack)) => json(&ack),
    }
}

fn json<T: serde::Serialize>(value: &T) -> Response {
    (axum::http::StatusCode::OK, Json(value)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{MockEmp, samples};

    async fn started() -> MockHubjectServer {
        let mut hubject = MockHubject::new();
        hubject.register_emp(MockEmp::permissive("DE-DCB".parse().unwrap()));
        hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E1").into()).unwrap();
        MockHubjectServer::start(hubject).await.expect("the mock binds an ephemeral port")
    }

    #[tokio::test]
    async fn the_base_url_is_the_one_the_endpoint_table_expects() {
        let server = started().await;
        assert!(server.base_url().ends_with("/api/oicp"));
        // The path the real client will build against it.
        let url = Operation::PullEvseData
            .url(server.base_url(), &crate::transport::PathId::Provider("DE-DCB".parse().unwrap()))
            .unwrap();
        assert!(url.contains("/api/oicp/evsepull/v23/providers/DE-DCB/data-records"));
        server.stop().await;
    }

    #[tokio::test]
    async fn the_broker_behind_the_socket_is_the_one_that_was_given() {
        let server = started().await;
        assert_eq!(server.hubject().evse_count(), 1);
        server.stop().await;
    }
}
