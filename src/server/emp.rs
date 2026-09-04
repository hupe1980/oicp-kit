//! `EmpService` — the four things Hubject asks an e-Mobility Provider to do.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;

use crate::cpo::{
    AuthorizationStartResponse, AuthorizationStopResponse, AuthorizeStartRequest, AuthorizeStopRequest,
    ChargeDetailRecord, ChargingNotification,
};
use crate::transport::Operation;
use crate::types::{Acknowledgement, Code, Validate};

/// What an EMP must be able to do when Hubject calls it.
///
/// # This is where the money is decided
///
/// [`authorize_start`](Self::authorize_start) is the method that says whether a driver may charge,
/// and it is answered in the seconds a driver is standing at a charging point. Two rules:
///
/// * **Answer quickly.** Hubject reports a timeout to the CPO as
///   [`Code::PartnerDidNotRespond`], and the driver is refused.
/// * **Say why.** A refusal with `000 Success` is unreadable; a refusal with
///   [`Code::NoValidContract`] or [`Code::EvcoIdLocked`] tells the CPO's support desk what
///   happened. [`AuthorizationStartResponse::not_authorized`] fills both fields consistently.
pub trait EmpService: Send + Sync + 'static {
    /// A driver has presented a card or an app at a charging point. May they charge?
    ///
    /// Return [`AuthorizationStartResponse::authorized`] with a session id you can settle against
    /// later, or [`AuthorizationStartResponse::not_authorized`] with the reason.
    fn authorize_start(
        &self,
        request: AuthorizeStartRequest,
    ) -> impl core::future::Future<Output = AuthorizationStartResponse> + Send;

    /// A driver wants to stop a session. May they?
    fn authorize_stop(
        &self,
        request: AuthorizeStopRequest,
    ) -> impl core::future::Future<Output = AuthorizationStopResponse> + Send;

    /// A completed session, for billing.
    ///
    /// Acknowledge only once it is durably stored: an acknowledged CDR is not sent again.
    fn charge_detail_record(
        &self,
        cdr: ChargeDetailRecord,
    ) -> impl core::future::Future<Output = Acknowledgement> + Send;

    /// Something happened during a session in progress.
    ///
    /// Optional in the specification, and the one that makes a driver's app show live progress.
    fn charging_notification(
        &self,
        notification: ChargingNotification,
    ) -> impl core::future::Future<Output = Acknowledgement> + Send;

    /// What to do with a request that does not satisfy the specification.
    ///
    /// The default reports [`Code::DataError`]. Overriding it to return `None` accepts the request
    /// anyway — which for a CDR is often right: refusing to be billed because a CPO overran a text
    /// field is not a commercial position anyone wants to defend.
    fn on_invalid_request(&self, violations: &crate::types::Violations) -> Option<Acknowledgement> {
        Some(Acknowledgement::failure_with(Code::DataError, violations.to_string()))
    }
}

struct AckResponse(Acknowledgement);

impl IntoResponse for AckResponse {
    fn into_response(self) -> Response {
        (axum::http::StatusCode::OK, Json(self.0)).into_response()
    }
}

/// Builds the router Hubject calls for an EMP.
///
/// The authorization paths are the CPO-facing ones — Hubject forwards a CPO's `AuthorizeStart` to
/// the EMP at the same path shape — so this router serves the `operators/{operatorID}` paths.
pub fn emp_router<S: EmpService>(service: Arc<S>) -> axum::Router {
    // Each route awaits its trait method directly; see the note in the CPO router for why the
    // methods are not passed as values.
    axum::Router::new()
        .route(
            Operation::AuthorizeStart.path_template(),
            post(|State(s): State<Arc<S>>, Json(body): Json<serde_json::Value>| async move {
                match accept(s.as_ref(), body) {
                    Err(refusal) => {
                        json_ok(&AuthorizationStartResponse::not_authorized(refusal.status_code.code))
                    }
                    Ok(request) => json_ok(&s.authorize_start(request).await),
                }
            }),
        )
        .route(
            Operation::AuthorizeStop.path_template(),
            post(|State(s): State<Arc<S>>, Json(body): Json<serde_json::Value>| async move {
                match accept(s.as_ref(), body) {
                    Err(refusal) => json_ok(&not_authorized_stop(refusal.status_code.code)),
                    Ok(request) => json_ok(&s.authorize_stop(request).await),
                }
            }),
        )
        .route(
            Operation::ChargeDetailRecord.path_template(),
            post(|State(s): State<Arc<S>>, Json(body): Json<serde_json::Value>| async move {
                match accept(s.as_ref(), body) {
                    Err(refusal) => AckResponse(refusal).into_response(),
                    Ok(cdr) => AckResponse(s.charge_detail_record(cdr).await).into_response(),
                }
            }),
        )
        .route(
            Operation::ChargingNotifications.path_template(),
            post(|State(s): State<Arc<S>>, Json(body): Json<serde_json::Value>| async move {
                match accept(s.as_ref(), body) {
                    Err(refusal) => AckResponse(refusal).into_response(),
                    Ok(notification) => {
                        AckResponse(s.charging_notification(notification).await).into_response()
                    }
                }
            }),
        )
        .with_state(service)
}

fn not_authorized_stop(code: Code) -> AuthorizationStopResponse {
    AuthorizationStopResponse {
        session_id: None,
        cpo_partner_session_id: None,
        emp_partner_session_id: None,
        provider_id: None,
        authorization_status: crate::cpo::AuthorizationStatus::NotAuthorized,
        status_code: code.into(),
        extensions: crate::types::Extensions::new(),
    }
}

/// Decodes and validates one request, or the refusal to answer with.
///
/// The refusal keeps the reason — the decode error, or the violations with their JSON Pointers —
/// so a partner debugging an integration is told what was wrong rather than just that something
/// was.
///
/// # Errors
///
/// Returns the acknowledgement to answer with when the body does not decode, or when it decodes
/// but the service chooses to refuse it.
fn accept<S: EmpService, T: serde::de::DeserializeOwned + Validate>(
    service: &S,
    body: serde_json::Value,
) -> Result<T, Acknowledgement> {
    let request: T = deserialize(body).map_err(|message| {
        tracing::warn!(target: "oicp_kit::server", %message, "a request could not be decoded");
        Acknowledgement::failure_with(Code::DataError, message)
    })?;

    // Parse permissively, validate explicitly: the request decoded, and the service decides what
    // to do about any violations.
    if let Err(violations) = request.validate() {
        tracing::warn!(target: "oicp_kit::server", %violations, "a request is not conformant");
        if let Some(refusal) = service.on_invalid_request(&violations) {
            return Err(refusal);
        }
    }
    Ok(request)
}

fn json_ok<T: serde::Serialize>(value: &T) -> Response {
    (axum::http::StatusCode::OK, Json(value)).into_response()
}

fn deserialize<T: serde::de::DeserializeOwned>(body: serde_json::Value) -> Result<T, String> {
    serde_path_to_error::deserialize(body).map_err(|e| format!("{} at {}", e.inner(), e.path()))
}

// Fixtures come from `testkit::samples`, so these tests compile when that feature is on.
#[cfg(all(test, feature = "testkit"))]
mod tests {
    use super::*;
    use crate::testkit::samples;

    struct Emp {
        refuse: Option<Code>,
        cdrs: std::sync::Mutex<Vec<ChargeDetailRecord>>,
    }

    impl EmpService for Emp {
        async fn authorize_start(&self, _: AuthorizeStartRequest) -> AuthorizationStartResponse {
            match &self.refuse {
                None => AuthorizationStartResponse::authorized(samples::session_id()),
                Some(code) => AuthorizationStartResponse::not_authorized(code.clone()),
            }
        }
        async fn authorize_stop(&self, _: AuthorizeStopRequest) -> AuthorizationStopResponse {
            AuthorizationStopResponse {
                session_id: None,
                cpo_partner_session_id: None,
                emp_partner_session_id: None,
                provider_id: None,
                authorization_status: crate::cpo::AuthorizationStatus::Authorized,
                status_code: Code::Success.into(),
                extensions: crate::types::Extensions::new(),
            }
        }
        async fn charge_detail_record(&self, cdr: ChargeDetailRecord) -> Acknowledgement {
            self.cdrs.lock().unwrap().push(cdr);
            Acknowledgement::success()
        }
        async fn charging_notification(&self, _: ChargingNotification) -> Acknowledgement {
            Acknowledgement::success()
        }
    }

    fn emp(refuse: Option<Code>) -> Emp {
        Emp { refuse, cdrs: std::sync::Mutex::new(vec![]) }
    }

    #[tokio::test]
    async fn an_authorization_reaches_the_service_and_comes_back_conformant() {
        let service = emp(None);
        let body = serde_json::to_value(samples::authorize_start_request("DE*ABC*E1")).unwrap();
        let request = accept::<_, AuthorizeStartRequest>(&service, body).expect("a conformant request");
        let answer = service.authorize_start(request).await;
        assert!(answer.is_authorized());
        assert!(answer.validate().is_ok());
    }

    #[tokio::test]
    async fn a_refusal_carries_the_reason_and_validates() {
        let service = emp(Some(Code::NoValidContract));
        let answer = service.authorize_start(samples::authorize_start_request("DE*ABC*E1")).await;
        assert!(!answer.is_authorized());
        assert_eq!(answer.status_code.code, Code::NoValidContract);
        // The consistency rule the wire type enforces: a refusal never claims 000.
        assert!(answer.validate().is_ok());
    }

    #[tokio::test]
    async fn a_cdr_reaches_the_service() {
        let service = emp(None);
        let cdr = samples::charge_detail_record("DE*ABC*E1", samples::session_id());
        let body = serde_json::to_value(&cdr).unwrap();
        let request = accept::<_, ChargeDetailRecord>(&service, body).expect("a conformant CDR");
        assert!(service.charge_detail_record(request).await.is_success());
        assert_eq!(service.cdrs.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_non_conformant_cdr_is_refused_by_default_but_can_be_accepted() {
        let mut cdr = samples::charge_detail_record("DE*ABC*E1", samples::session_id());
        cdr.consumed_energy = crate::types::Number::from(99); // contradicts the meter readings

        let service = emp(None);
        let body = serde_json::to_value(&cdr).unwrap();
        assert!(
            accept::<_, ChargeDetailRecord>(&service, body.clone()).is_err(),
            "the default refuses a CDR whose energy contradicts its meter readings"
        );
        assert!(service.cdrs.lock().unwrap().is_empty());

        let tolerant = Tolerant(std::sync::Mutex::new(vec![]));
        let request =
            accept::<_, ChargeDetailRecord>(&tolerant, body).expect("a tolerant service bills it anyway");
        assert!(tolerant.charge_detail_record(request).await.is_success());
        assert_eq!(tolerant.0.lock().unwrap().len(), 1);
    }

    /// An EMP that bills a CDR whatever the violations, and reconciles later.
    struct Tolerant(std::sync::Mutex<Vec<ChargeDetailRecord>>);

    impl EmpService for Tolerant {
        async fn authorize_start(&self, _: AuthorizeStartRequest) -> AuthorizationStartResponse {
            AuthorizationStartResponse::not_authorized(Code::NoValidContract)
        }
        async fn authorize_stop(&self, _: AuthorizeStopRequest) -> AuthorizationStopResponse {
            not_authorized_stop(Code::SessionIsInvalid)
        }
        async fn charge_detail_record(&self, cdr: ChargeDetailRecord) -> Acknowledgement {
            self.0.lock().unwrap().push(cdr);
            Acknowledgement::success()
        }
        async fn charging_notification(&self, _: ChargingNotification) -> Acknowledgement {
            Acknowledgement::success()
        }
        fn on_invalid_request(&self, _: &crate::types::Violations) -> Option<Acknowledgement> {
            None // bill it anyway, and reconcile later
        }
    }

    #[test]
    fn an_emp_serves_exactly_what_the_table_says_an_emp_serves() {
        // The mirror of the CPO check: an EMP implements the *other* four, at paths a CPO calls.
        // `AuthorizeRemoteStart` is the pair that makes the point — an EMP calls it and a CPO
        // serves it, at one path, so no single "direction" on the operation can be true for both.
        use crate::transport::Role;

        let served: Vec<Operation> =
            Operation::ALL.iter().copied().filter(|op| op.is_served_by(Role::Emp)).collect();
        assert_eq!(
            served,
            vec![
                Operation::AuthorizeStart,
                Operation::AuthorizeStop,
                Operation::ChargeDetailRecord,
                Operation::ChargingNotifications,
            ],
            "the four methods on EmpService, and nothing else"
        );

        for operation in [Operation::AuthorizeRemoteStart, Operation::PullEvseData] {
            assert_eq!(
                operation.involvement(Role::Emp),
                Some(crate::transport::Involvement::YouCall),
                "{operation:?} is one an EMP calls"
            );
        }
        assert!(Operation::AuthorizeRemoteStart.is_served_by(Role::Cpo), "and a CPO serves it");
        assert!(Operation::PushEvseData.involvement(Role::Emp).is_none(), "an EMP pushes no fleet");
    }

    #[test]
    fn the_router_builds() {
        let _router = emp_router(Arc::new(emp(None)));
    }
}
