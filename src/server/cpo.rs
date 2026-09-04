//! `CpoService` — the four things Hubject asks a Charge Point Operator to do.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;

use crate::cpo::{
    AuthorizeRemoteReservationStartRequest, AuthorizeRemoteReservationStopRequest,
    AuthorizeRemoteStartRequest, AuthorizeRemoteStopRequest,
};
use crate::transport::Operation;
use crate::types::{Acknowledgement, Code, Validate};

/// What a CPO must be able to do when Hubject calls it.
///
/// # All four are required
///
/// The trait has no default methods, deliberately. A CPO that has not decided what to do about
/// reservations should say [`Code::ServiceNotAvailable`] out loud, in code someone can read,
/// rather than inherit a default that quietly answers something.
///
/// Only [`authorize_remote_start`](Self::authorize_remote_start) and
/// [`authorize_remote_stop`](Self::authorize_remote_stop) are mandatory in the specification; the
/// two reservation methods implement an optional service. Refusing them honestly with
/// `Code::ServiceNotAvailable` is conformant. Answering them with success when nothing happened is
/// not.
pub trait CpoService: Send + Sync + 'static {
    /// An EMP wants to start a charging session at one of your charging points.
    ///
    /// *"Implementation: MANDATORY."* Start the session and acknowledge, or refuse with a code
    /// that says why — [`Code::EvseAlreadyInUse`], [`Code::CommunicationToEvseFailed`],
    /// [`Code::NoEvConnectedToEvse`].
    fn authorize_remote_start(
        &self,
        request: AuthorizeRemoteStartRequest,
    ) -> impl core::future::Future<Output = Acknowledgement> + Send;

    /// An EMP wants to stop a session it started.
    fn authorize_remote_stop(
        &self,
        request: AuthorizeRemoteStopRequest,
    ) -> impl core::future::Future<Output = Acknowledgement> + Send;

    /// An EMP wants to reserve a charging point.
    ///
    /// Optional in the specification. Refuse with [`Code::ServiceNotAvailable`] if you do not
    /// offer reservations, and publish your charging points without
    /// [`ValueAddedService::Reservation`](crate::types::ValueAddedService::Reservation) so no EMP
    /// offers it to a driver.
    fn reservation_start(
        &self,
        request: AuthorizeRemoteReservationStartRequest,
    ) -> impl core::future::Future<Output = Acknowledgement> + Send;

    /// An EMP releases a reservation.
    fn reservation_stop(
        &self,
        request: AuthorizeRemoteReservationStopRequest,
    ) -> impl core::future::Future<Output = Acknowledgement> + Send;

    /// What to do with a request that does not satisfy the specification.
    ///
    /// The default reports [`Code::DataError`] with the violations, which is what Hubject itself
    /// does. Override it to accept them anyway — some partners send non-conformant requests that
    /// are perfectly actionable, and refusing a driver over a `string(250)` overrun helps nobody.
    fn on_invalid_request(&self, violations: &crate::types::Violations) -> Option<Acknowledgement> {
        Some(Acknowledgement::failure_with(Code::DataError, violations.to_string()))
    }
}

/// An acknowledgement, as OICP sends it: `HTTP 200`, whatever it says.
struct AckResponse(Acknowledgement);

impl IntoResponse for AckResponse {
    fn into_response(self) -> Response {
        // OICP answers a refusal with 200 and `Result: false`. A 4xx here would be a protocol
        // error that Hubject reports as a failed delivery rather than a refusal.
        (axum::http::StatusCode::OK, Json(self.0)).into_response()
    }
}

/// Builds the router Hubject calls.
///
/// The paths come from [`Operation`], so they cannot drift from the endpoint table — and the table
/// is checked against the vendored OpenAPI documents in CI.
pub fn cpo_router<S: CpoService>(service: Arc<S>) -> axum::Router {
    // Each route awaits its trait method directly. Passing the methods as function values would
    // read better, but a return-position `impl Future` in a trait cannot be unified across the
    // higher-ranked lifetime that would need — so the small repetition stays.
    axum::Router::new()
        .route(
            Operation::AuthorizeRemoteStart.path_template(),
            post(|State(s): State<Arc<S>>, Json(body): Json<serde_json::Value>| async move {
                match accept(s.as_ref(), body) {
                    Err(response) => response,
                    Ok(request) => AckResponse(s.authorize_remote_start(request).await).into_response(),
                }
            }),
        )
        .route(
            Operation::AuthorizeRemoteStop.path_template(),
            post(|State(s): State<Arc<S>>, Json(body): Json<serde_json::Value>| async move {
                match accept(s.as_ref(), body) {
                    Err(response) => response,
                    Ok(request) => AckResponse(s.authorize_remote_stop(request).await).into_response(),
                }
            }),
        )
        .route(
            Operation::AuthorizeRemoteReservationStart.path_template(),
            post(|State(s): State<Arc<S>>, Json(body): Json<serde_json::Value>| async move {
                match accept(s.as_ref(), body) {
                    Err(response) => response,
                    Ok(request) => AckResponse(s.reservation_start(request).await).into_response(),
                }
            }),
        )
        .route(
            Operation::AuthorizeRemoteReservationStop.path_template(),
            post(|State(s): State<Arc<S>>, Json(body): Json<serde_json::Value>| async move {
                match accept(s.as_ref(), body) {
                    Err(response) => response,
                    Ok(request) => AckResponse(s.reservation_stop(request).await).into_response(),
                }
            }),
        )
        .with_state(service)
}

/// Decodes and validates one request, or produces the refusal to send instead.
///
/// # Errors
///
/// Returns the response to send when the body does not decode, or when it decodes but the service
/// chooses to refuse it.
fn accept<S: CpoService, T: serde::de::DeserializeOwned + Validate>(
    service: &S,
    body: serde_json::Value,
) -> Result<T, Response> {
    let request: T = deserialize(body).map_err(|message| {
        tracing::warn!(target: "oicp_kit::server", %message, "a request could not be decoded");
        AckResponse(Acknowledgement::failure_with(Code::DataError, message)).into_response()
    })?;

    // Parse permissively, validate explicitly: the request decoded, and the service decides what
    // to do about any violations.
    if let Err(violations) = request.validate() {
        tracing::warn!(target: "oicp_kit::server", %violations, "a request is not conformant");
        if let Some(refusal) = service.on_invalid_request(&violations) {
            return Err(AckResponse(refusal).into_response());
        }
    }
    Ok(request)
}

fn deserialize<T: serde::de::DeserializeOwned>(body: serde_json::Value) -> Result<T, String> {
    serde_path_to_error::deserialize(body).map_err(|e| format!("{} at {}", e.inner(), e.path()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Extensions, Identification, RemoteIdentification};

    struct Recording {
        started: std::sync::Mutex<Vec<String>>,
    }

    impl CpoService for Recording {
        async fn authorize_remote_start(&self, request: AuthorizeRemoteStartRequest) -> Acknowledgement {
            self.started.lock().unwrap().push(request.evse_id.to_string());
            Acknowledgement::success().with_session(request.session_id)
        }
        async fn authorize_remote_stop(&self, request: AuthorizeRemoteStopRequest) -> Acknowledgement {
            Acknowledgement::success().with_session(request.session_id)
        }
        async fn reservation_start(&self, _: AuthorizeRemoteReservationStartRequest) -> Acknowledgement {
            Acknowledgement::failure(Code::ServiceNotAvailable)
        }
        async fn reservation_stop(&self, _: AuthorizeRemoteReservationStopRequest) -> Acknowledgement {
            Acknowledgement::failure(Code::ServiceNotAvailable)
        }
    }

    fn remote_start() -> AuthorizeRemoteStartRequest {
        AuthorizeRemoteStartRequest {
            session_id: "f98efba4-02d8-4fa0-b810-9a9d50d2c527".parse().unwrap(),
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            provider_id: "DE-DCB".parse().unwrap(),
            evse_id: "DE*ABC*E1".parse().unwrap(),
            identification: Identification::Remote(RemoteIdentification {
                evco_id: "DE-DCB-C12345678-X".parse().unwrap(),
            }),
            partner_product_id: None,
            extensions: Extensions::new(),
        }
    }

    #[tokio::test]
    async fn a_conformant_request_reaches_the_service() {
        let service = Recording { started: std::sync::Mutex::new(vec![]) };
        let body = serde_json::to_value(remote_start()).unwrap();
        let request = accept::<_, AuthorizeRemoteStartRequest>(&service, body).expect("a conformant request");
        let ack = service.authorize_remote_start(request).await;

        assert!(ack.is_success());
        assert_eq!(service.started.lock().unwrap().as_slice(), ["DE*ABC*E1"]);
    }

    #[tokio::test]
    async fn a_malformed_request_is_refused_without_reaching_the_service() {
        let service = Recording { started: std::sync::Mutex::new(vec![]) };
        let body = serde_json::json!({"not": "a remote start"});
        let response = accept::<_, AuthorizeRemoteStartRequest>(&service, body)
            .expect_err("a malformed body is refused");

        // Still 200: OICP reports a refusal in the body, not the status line.
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert!(service.started.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_non_conformant_request_is_refused_by_default() {
        let service = Recording { started: std::sync::Mutex::new(vec![]) };
        // An RFID identification is illegal in a remote start.
        let mut request = remote_start();
        request.identification =
            Identification::RfidMifareFamily(crate::types::RfidMifareFamilyIdentification {
                uid: "7568290FFF765F".parse().unwrap(),
            });
        let body = serde_json::to_value(&request).unwrap();

        let response = accept::<_, AuthorizeRemoteStartRequest>(&service, body)
            .expect_err("an RFID identification is illegal in a remote start");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert!(service.started.lock().unwrap().is_empty(), "the default refuses it");
    }

    #[tokio::test]
    async fn a_service_may_choose_to_accept_non_conformant_requests() {
        struct Tolerant;
        impl CpoService for Tolerant {
            async fn authorize_remote_start(&self, request: AuthorizeRemoteStartRequest) -> Acknowledgement {
                Acknowledgement::success().with_session(request.session_id)
            }
            async fn authorize_remote_stop(&self, r: AuthorizeRemoteStopRequest) -> Acknowledgement {
                Acknowledgement::success().with_session(r.session_id)
            }
            async fn reservation_start(&self, _: AuthorizeRemoteReservationStartRequest) -> Acknowledgement {
                Acknowledgement::failure(Code::ServiceNotAvailable)
            }
            async fn reservation_stop(&self, _: AuthorizeRemoteReservationStopRequest) -> Acknowledgement {
                Acknowledgement::failure(Code::ServiceNotAvailable)
            }
            fn on_invalid_request(&self, _: &crate::types::Violations) -> Option<Acknowledgement> {
                None // start the session anyway
            }
        }

        let mut request = remote_start();
        request.identification =
            Identification::RfidMifareFamily(crate::types::RfidMifareFamilyIdentification {
                uid: "7568290FFF765F".parse().unwrap(),
            });
        let body = serde_json::to_value(&request).unwrap();
        let request = accept::<_, AuthorizeRemoteStartRequest>(&Tolerant, body)
            .expect("a tolerant service accepts it anyway");
        assert!(Tolerant.authorize_remote_start(request).await.is_success());
    }

    #[test]
    fn the_router_uses_the_paths_from_the_endpoint_table() {
        // A compile-and-construct check: the router cannot drift from `Operation`.
        let _router = cpo_router(Arc::new(Recording { started: std::sync::Mutex::new(vec![]) }));
        assert_eq!(
            Operation::AuthorizeRemoteStart.path_template(),
            "/charging/v21/providers/{providerID}/authorize-remote/start"
        );
    }

    #[test]
    fn a_cpo_serves_exactly_what_the_table_says_a_cpo_serves() {
        // "Both directions, one trait each" is a promise that the trait *is* the Hubject-facing
        // surface. That is only true if the set of operations the table marks `YouServe` for a CPO
        // and the set the router mounts are the same set — so the table is the specification and
        // this is the check. An operation added to one and not the other is a partner discovering
        // in production that Hubject calls something nobody implemented.
        use crate::transport::Role;

        let served: Vec<Operation> =
            Operation::ALL.iter().copied().filter(|op| op.is_served_by(Role::Cpo)).collect();
        assert_eq!(
            served,
            vec![
                Operation::AuthorizeRemoteStart,
                Operation::AuthorizeRemoteStop,
                Operation::AuthorizeRemoteReservationStart,
                Operation::AuthorizeRemoteReservationStop,
            ],
            "the four methods on CpoService, and nothing else"
        );

        // Everything else a CPO takes part in, it calls.
        for operation in Operation::for_role(Role::Cpo) {
            assert!(
                served.contains(&operation) || !operation.is_served_by(Role::Cpo),
                "{operation:?} is neither called nor served"
            );
        }
        // And a CPO takes no part in the pulls at all.
        assert!(Operation::PullEvseData.involvement(Role::Cpo).is_none());
        assert!(Operation::GetChargeDetailRecords.involvement(Role::Cpo).is_none());
    }
}
