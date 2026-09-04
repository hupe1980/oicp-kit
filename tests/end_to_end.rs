//! A CPO and an EMP, both built on this crate, talking through a broker.
//!
//! The point of these tests is the *sequence*, not any single message: the failures that cost
//! partners weeks are a CDR for a session nobody opened, a remote start at a charge point
//! published as incompatible, a refusal that carries no reason.

use std::sync::{Arc, Mutex};

use oicp_kit::cpo::{
    AuthorizationStartResponse, AuthorizationStopResponse, AuthorizeRemoteReservationStartRequest,
    AuthorizeRemoteReservationStopRequest, AuthorizeRemoteStartRequest, AuthorizeRemoteStopRequest,
    AuthorizeStartRequest, AuthorizeStopRequest, ChargeDetailRecord, ChargingNotification,
};
use oicp_kit::eichrecht::{CdrCheck, Severity};
use oicp_kit::server::{CpoService, EmpService};
use oicp_kit::testkit::{Event, MockEmp, MockHubject, samples, scenarios};
use oicp_kit::types::{Acknowledgement, CalibrationLawDataAvailability, Code, SessionId, Validate};

/// A CPO that starts whatever it is told to, and records what it was told.
#[derive(Default)]
struct Cpo {
    started: Mutex<Vec<SessionId>>,
    stopped: Mutex<Vec<SessionId>>,
}

impl CpoService for Cpo {
    async fn authorize_remote_start(&self, request: AuthorizeRemoteStartRequest) -> Acknowledgement {
        self.started.lock().unwrap().push(request.session_id.clone());
        Acknowledgement::success().with_session(request.session_id)
    }
    async fn authorize_remote_stop(&self, request: AuthorizeRemoteStopRequest) -> Acknowledgement {
        self.stopped.lock().unwrap().push(request.session_id.clone());
        Acknowledgement::success().with_session(request.session_id)
    }
    async fn reservation_start(&self, _: AuthorizeRemoteReservationStartRequest) -> Acknowledgement {
        // Honest: this CPO does not offer reservations.
        Acknowledgement::failure(Code::ServiceNotAvailable)
    }
    async fn reservation_stop(&self, _: AuthorizeRemoteReservationStopRequest) -> Acknowledgement {
        Acknowledgement::failure(Code::ServiceNotAvailable)
    }
}

/// An EMP that authorizes a known card and refuses everything else.
struct Emp {
    known_uid: String,
    cdrs: Mutex<Vec<ChargeDetailRecord>>,
    notifications: Mutex<Vec<ChargingNotification>>,
}

impl EmpService for Emp {
    async fn authorize_start(&self, request: AuthorizeStartRequest) -> AuthorizationStartResponse {
        let known = request.identification.uid().is_some_and(|uid| uid.canonical() == self.known_uid);
        if known {
            AuthorizationStartResponse::authorized(samples::session_id())
        } else {
            AuthorizationStartResponse::not_authorized(Code::NoValidContract)
        }
    }
    async fn authorize_stop(&self, _: AuthorizeStopRequest) -> AuthorizationStopResponse {
        AuthorizationStopResponse {
            session_id: None,
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            provider_id: None,
            authorization_status: oicp_kit::cpo::AuthorizationStatus::Authorized,
            status_code: Code::Success.into(),
            extensions: oicp_kit::types::Extensions::new(),
        }
    }
    async fn charge_detail_record(&self, cdr: ChargeDetailRecord) -> Acknowledgement {
        self.cdrs.lock().unwrap().push(cdr);
        Acknowledgement::success()
    }
    async fn charging_notification(&self, notification: ChargingNotification) -> Acknowledgement {
        self.notifications.lock().unwrap().push(notification);
        Acknowledgement::success()
    }
}

fn emp(known_uid: &str) -> Arc<Emp> {
    Arc::new(Emp {
        known_uid: known_uid.to_owned(),
        cdrs: Mutex::new(vec![]),
        notifications: Mutex::new(vec![]),
    })
}

fn broker() -> MockHubject {
    let mut hubject = MockHubject::new();
    hubject.register_emp(MockEmp::permissive("DE-DCB".parse().unwrap()));
    hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E1").into()).expect("push");
    hubject
}

#[tokio::test]
async fn a_session_runs_from_authorization_to_settlement() {
    let hubject = broker();
    let emp = emp("7568290FFF765F");

    // 1. A driver presents a card. Hubject routes it to the EMP.
    let request = samples::authorize_start_request("DE*ABC*E1");
    let answer = emp.authorize_start(request.clone()).await;
    assert!(answer.is_authorized());
    answer.validate().expect("the answer is conformant");
    let session_id = answer.session_id.clone().unwrap();

    // 2. The broker opens its own session for the CPO's request.
    let brokered = hubject.authorize_start(&request);
    assert!(brokered.is_authorized());
    let brokered_session = brokered.session_id.clone().unwrap();

    // 3. The CPO reports that energy is flowing.
    let notification = ChargingNotification::Start(samples::charging_notification_start(
        "DE*ABC*E1",
        brokered_session.clone(),
    ));
    hubject.notify(&notification).expect("the notification is accepted");
    emp.charging_notification(notification).await;
    assert_eq!(emp.notifications.lock().unwrap().len(), 1);

    // 4. The session ends and the CPO submits a CDR.
    let cdr = samples::charge_detail_record("DE*ABC*E1", brokered_session);
    hubject.submit_cdr(&cdr).expect("the CDR settles");
    emp.charge_detail_record(cdr).await;

    assert_eq!(emp.cdrs.lock().unwrap().len(), 1);
    assert!(hubject.sessions().iter().all(|s| s.settled));
    let _ = session_id;
}

#[tokio::test]
async fn a_remote_start_reaches_the_cpo_and_comes_back_conformant() {
    let hubject = broker();
    let cpo = Arc::new(Cpo::default());

    // The EMP asks Hubject; Hubject produces the request the CPO will receive.
    let request = hubject
        .remote_start(
            &"DE-DCB".parse().unwrap(),
            &"DE*ABC*E1".parse().unwrap(),
            &"DE-DCB-C12345678-X".parse().unwrap(),
        )
        .expect("the broker routes it");
    request.validate().expect("the request the CPO receives is conformant");

    let session_id = request.session_id.clone();
    let ack = cpo.authorize_remote_start(request).await;
    assert!(ack.is_success());
    assert_eq!(cpo.started.lock().unwrap().as_slice(), [session_id.clone()].as_slice());

    // …and the stop finds its way back to the same session.
    let stop = hubject.remote_stop(&"DE-DCB".parse().unwrap(), &session_id).expect("the session is known");
    stop.validate().expect("conformant");
    assert!(cpo.authorize_remote_stop(stop).await.is_success());
    assert_eq!(cpo.stopped.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_unknown_card_is_refused_with_a_reason_the_driver_can_be_told() {
    let emp = emp("AAAAAAAAAAAAAA");
    let answer = emp.authorize_start(samples::authorize_start_request("DE*ABC*E1")).await;

    assert!(!answer.is_authorized());
    assert_eq!(answer.status_code.code, Code::NoValidContract);
    assert!(answer.status_code.code.is_authorization_failure());
    // The consistency rule: a refusal never claims 000, and never carries a session id.
    answer.validate().expect("a refusal is conformant");
    assert!(answer.session_id.is_none());
}

#[tokio::test]
async fn a_cdr_for_a_session_nobody_opened_is_refused_by_the_broker() {
    let hubject = broker();
    // The commonest integration failure: a CPO that invents its own session ids.
    let cdr = samples::charge_detail_record("DE*ABC*E1", samples::session_id());
    let refusal = hubject.submit_cdr(&cdr).expect_err("the broker refuses it");

    assert_eq!(*refusal.code(), Code::SessionIsInvalid);
    refusal.validate().expect("even the refusal is conformant");
    assert!(refusal.status_code.additional_info.is_some(), "and it says which session");
}

#[tokio::test]
async fn a_cdr_is_checked_before_it_is_sent_not_after_it_is_rejected() {
    let hubject = broker();
    let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
    let session_id = response.session_id.clone().unwrap();

    // The charging point publishes calibration data externally, so every CDR from it must carry
    // signed metering values. The CDR does not.
    let mut evse = samples::evse_data_record("DE*ABC*E1");
    evse.calibration_law_data_availability = CalibrationLawDataAvailability::External;
    let cdr = samples::charge_detail_record("DE*ABC*E1", session_id);

    let check = CdrCheck::new().against_evse(&evse);
    assert!(!check.is_submittable(&cdr), "the pre-flight catches it");
    let findings = check.run(&cdr);
    assert_eq!(findings[0].severity, Severity::Error);
    assert!(findings[0].message.contains("German calibration law"));

    // The CDR is still structurally valid, which is why the pre-flight is needed at all: the
    // broker would accept it, and the dispute would arrive weeks later.
    cdr.validate().expect("the CDR is self-consistent");
    assert!(hubject.submit_cdr(&cdr).is_ok());
}

#[tokio::test]
async fn a_reservation_a_cpo_does_not_offer_is_refused_honestly() {
    let cpo = Arc::new(Cpo::default());
    let request = AuthorizeRemoteReservationStartRequest {
        session_id: None,
        cpo_partner_session_id: None,
        emp_partner_session_id: None,
        provider_id: "DE-DCB".parse().unwrap(),
        evse_id: "DE*ABC*E1".parse().unwrap(),
        identification: oicp_kit::types::Identification::Remote(oicp_kit::types::RemoteIdentification {
            evco_id: "DE-DCB-C12345678-X".parse().unwrap(),
        }),
        partner_product_id: None,
        duration: Some(15),
        extensions: oicp_kit::types::Extensions::new(),
    };
    request.validate().expect("conformant");

    let ack = cpo.reservation_start(request).await;
    assert!(!ack.is_success(), "answering success for something that did not happen is worse than refusing");
    assert_eq!(*ack.code(), Code::ServiceNotAvailable);
    ack.validate().expect("the refusal is conformant");
}

#[test]
fn every_onboarding_scenario_passes() {
    let report = scenarios::run_all();
    assert!(report.passed(), "\n{report}");
}

#[test]
fn the_broker_records_the_sequence_that_happened() {
    let hubject = broker();
    let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
    let session_id = response.session_id.clone().unwrap();
    hubject
        .notify(&ChargingNotification::Start(samples::charging_notification_start(
            "DE*ABC*E1",
            session_id.clone(),
        )))
        .unwrap();
    hubject.submit_cdr(&samples::charge_detail_record("DE*ABC*E1", session_id)).unwrap();

    let events = hubject.events();
    assert!(matches!(events[0], Event::EvseDataPushed { .. }));
    assert!(matches!(events[1], Event::Authorized { authorized: true, .. }));
    assert!(matches!(events[2], Event::NotificationReceived { .. }));
    assert!(matches!(events[3], Event::CdrSubmitted { .. }));
}
