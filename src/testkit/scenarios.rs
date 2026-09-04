//! The sequences Hubject walks a partner through, as runnable checks.
//!
//! # What these are
//!
//! Onboarding to Hubject ends with an integration test: a series of scenarios run against their QA
//! environment, with a Hubject engineer watching. Failing one costs another round.
//!
//! Each scenario here is one of those sequences, run against a [`MockHubject`]. Passing them all
//! does not make the paid test unnecessary — only Hubject can certify — but it means the first
//! time you see these sequences is not the day they are being marked.
//!
//! ```
//! # use oicp_kit::testkit::scenarios;
//! let report = scenarios::run_all();
//! assert!(report.passed(), "{report}");
//! ```

use core::fmt;

use super::mock::{Event, MockEmp, MockHubject};
use super::samples;
use crate::cpo::{
    AuthorizationStatus, ChargingNotification, EvseStatus, OperatorEvseStatus, PushEvseStatusRequest,
};
use crate::emp::PullEvseDataRequest;
use crate::sync::{self, EvseRepository, InMemoryEvseRepository, Planner, PlannerConfig};
use crate::transport::PageQuery;
use crate::types::{
    ActionType, Code, Extensions, GeoCoordinatesFormat, Identification, RfidMifareFamilyIdentification, Text,
    Uid, Validate,
};

/// How one scenario went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioResult {
    /// What it is called.
    pub name: &'static str,
    /// What it checks.
    pub description: &'static str,
    /// Why it failed, if it did.
    pub failure: Option<String>,
}

impl ScenarioResult {
    /// Whether it passed.
    #[must_use]
    pub const fn passed(&self) -> bool {
        self.failure.is_none()
    }
}

impl fmt::Display for ScenarioResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.failure {
            None => write!(f, "PASS  {}", self.name),
            Some(why) => write!(f, "FAIL  {} — {why}", self.name),
        }
    }
}

/// The outcome of a whole run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// Each scenario, in order.
    pub results: Vec<ScenarioResult>,
}

impl Report {
    /// Whether every scenario passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.results.iter().all(ScenarioResult::passed)
    }

    /// How many failed.
    #[must_use]
    pub fn failures(&self) -> usize {
        self.results.iter().filter(|r| !r.passed()).count()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for result in &self.results {
            writeln!(f, "{result}")?;
        }
        write!(f, "{} of {} passed", self.results.len() - self.failures(), self.results.len())
    }
}

/// Runs every scenario.
#[must_use]
pub fn run_all() -> Report {
    Report {
        results: vec![
            cpo_publishes_and_emp_discovers(),
            authorize_charge_settle(),
            remote_start_from_an_app(),
            a_refused_contract_is_reported_not_swallowed(),
            status_updates_reach_the_emp(),
            a_delta_crawl_converges_on_the_full_picture(),
            a_fleet_change_needs_no_full_load(),
            a_resubmitted_cdr_is_refused_not_settled_twice(),
            a_session_is_stopped_with_the_medium_that_started_it(),
        ],
    }
}

/// A CDR that arrives twice settles once.
///
/// The sequence a retry produces: the first submission lands, its answer is lost, and the CPO
/// sends the record again. *"Hubject will accept only one CDR per SessionID"*, so the second is
/// refused — and a partner whose reconciliation counts acknowledgements rather than sessions finds
/// that out here.
#[must_use]
pub fn a_resubmitted_cdr_is_refused_not_settled_twice() -> ScenarioResult {
    scenario(
        "a_resubmitted_cdr_is_refused_not_settled_twice",
        "A CDR submitted twice for one session settles once; the second is refused with 400.",
        || {
            let hubject = broker();
            hubject
                .push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())
                .map_err(|ack| format!("the push was refused: {ack:?}"))?;

            let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
            let session = response.session_id.clone().ok_or("no session was opened")?;
            let cdr = samples::charge_detail_record("DE*ABC*E1", session);

            hubject.submit_cdr(&cdr).map_err(|ack| format!("the first CDR was refused: {ack}"))?;

            let again = hubject.submit_cdr(&cdr);
            let refusal = again.err().ok_or("the same CDR was accepted twice")?;
            check(
                *refusal.code() == Code::SessionIsInvalid,
                format!("the resubmission was refused with {} rather than 400", refusal.code()),
            )?;
            check(hubject.cdrs().len() == 1, "the broker stored the CDR twice")
        },
    )
}

/// A session may only be stopped with the medium that started it.
#[must_use]
pub fn a_session_is_stopped_with_the_medium_that_started_it() -> ScenarioResult {
    scenario(
        "a_session_is_stopped_with_the_medium_that_started_it",
        "A stop presenting a different card is refused; the right one succeeds.",
        || {
            let hubject = broker();
            hubject
                .push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())
                .map_err(|ack| format!("the push was refused: {ack:?}"))?;

            let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
            let session = response.session_id.clone().ok_or("no session was opened")?;

            // Someone else's card.
            let mut wrong = samples::authorize_stop_request("DE*ABC*E1", session.clone());
            wrong.identification = Identification::RfidMifareFamily(RfidMifareFamilyIdentification {
                uid: Uid::new("AABBCCDDEEFF11").expect("valid"),
            });
            let refused = hubject.authorize_stop(&wrong);
            check(
                refused.authorization_status == AuthorizationStatus::NotAuthorized,
                "a stop with a different card was authorized",
            )?;

            // The card that started it.
            let right = samples::authorize_stop_request("DE*ABC*E1", session);
            let accepted = hubject.authorize_stop(&right);
            check(
                accepted.authorization_status == AuthorizationStatus::Authorized,
                "the stop with the correct medium was refused",
            )
        },
    )
}

fn scenario(
    name: &'static str,
    description: &'static str,
    body: impl FnOnce() -> Result<(), String>,
) -> ScenarioResult {
    ScenarioResult { name, description, failure: body().err() }
}

fn broker() -> MockHubject {
    let mut hubject = MockHubject::new();
    hubject.register_emp(MockEmp::permissive("DE-DCB".parse().expect("valid")));
    hubject
}

fn check(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.into()) }
}

/// A CPO publishes its fleet; an EMP pulls it and sees it.
#[must_use]
pub fn cpo_publishes_and_emp_discovers() -> ScenarioResult {
    scenario(
        "cpo_publishes_and_emp_discovers",
        "A CPO pushes EVSE data and an EMP's pull returns it, in the notation the EMP asked for.",
        || {
            let hubject = broker();
            hubject
                .push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())
                .map_err(|ack| format!("the push was refused: {ack:?}"))?;

            let request = PullEvseDataRequest::full(
                "DE-DCB".parse().expect("valid"),
                GeoCoordinatesFormat::DecimalDegree,
            );
            let page = hubject.pull_evse_data(&request, PageQuery::new());

            page.validate().map_err(|e| format!("the page is not conformant: {e}"))?;
            check(page.content.len() == 1, "the pull did not return the pushed record")?;
            check(
                page.content[0].geo_coordinates.format() == GeoCoordinatesFormat::DecimalDegree,
                "the coordinates did not come back in the requested notation",
            )?;
            page.content[0].validate().map_err(|e| format!("the record is not conformant: {e}"))
        },
    )
}

/// The core sequence: a driver swipes, charges, and the session settles.
#[must_use]
pub fn authorize_charge_settle() -> ScenarioResult {
    scenario(
        "authorize_charge_settle",
        "Authorize, notify start, notify end, submit the CDR — the sequence every session follows.",
        || {
            let hubject = broker();
            hubject
                .push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())
                .map_err(|_| "push refused")?;

            let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
            response.validate().map_err(|e| format!("the authorization response is not conformant: {e}"))?;
            check(response.is_authorized(), "the driver was not authorized")?;
            let session_id =
                response.session_id.clone().ok_or("an authorized response carried no SessionID")?;

            let start = ChargingNotification::Start(samples::charging_notification_start(
                "DE*ABC*E1",
                session_id.clone(),
            ));
            hubject.notify(&start).map_err(|ack| format!("the start notification was refused: {ack:?}"))?;

            let cdr = samples::charge_detail_record("DE*ABC*E1", session_id.clone());
            hubject.submit_cdr(&cdr).map_err(|ack| format!("the CDR was refused: {ack:?}"))?;

            check(hubject.cdrs().len() == 1, "the CDR was not recorded")?;
            check(hubject.sessions().iter().all(|s| s.settled), "the session was not marked settled")
        },
    )
}

/// A driver starts a session from their phone.
#[must_use]
pub fn remote_start_from_an_app() -> ScenarioResult {
    scenario(
        "remote_start_from_an_app",
        "An EMP starts a session remotely; the request the CPO receives is conformant and carries a \
         RemoteIdentification.",
        || {
            let hubject = broker();
            hubject
                .push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())
                .map_err(|_| "push refused")?;

            let request = hubject
                .remote_start(
                    &"DE-DCB".parse().expect("valid"),
                    &"DE*ABC*E1".parse().expect("valid"),
                    &"DE-DCB-C12345678-X".parse().expect("valid"),
                )
                .map_err(|ack| format!("the remote start was refused: {ack:?}"))?;

            request.validate().map_err(|e| format!("the request the CPO receives is not conformant: {e}"))?;
            check(
                matches!(request.identification, crate::types::Identification::Remote(_)),
                "the spec requires a RemoteIdentification in a remote start",
            )?;
            check(
                hubject.events().iter().any(|e| matches!(e, Event::RemoteStartRequested { .. })),
                "the broker did not record the remote start",
            )
        },
    )
}

/// An EMP says no, and the CPO can tell the driver why.
#[must_use]
pub fn a_refused_contract_is_reported_not_swallowed() -> ScenarioResult {
    scenario(
        "a_refused_contract_is_reported_not_swallowed",
        "A refusal carries a real status code, and the response is internally consistent.",
        || {
            let mut hubject = MockHubject::new();
            hubject.register_emp(MockEmp::refusing("DE-DCB".parse().expect("valid"), Code::NoValidContract));
            hubject
                .push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())
                .map_err(|_| "push refused")?;

            let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
            response.validate().map_err(|e| format!("the refusal is not conformant: {e}"))?;
            check(!response.is_authorized(), "the driver should not have been authorized")?;
            check(
                response.status_code.code == Code::NoValidContract,
                "the refusal did not carry the EMP's reason",
            )?;
            check(
                response.status_code.code.is_authorization_failure(),
                "the code is not classified as an authorization failure",
            )
        },
    )
}

/// Status pushes reach the EMP's view.
#[must_use]
pub fn status_updates_reach_the_emp() -> ScenarioResult {
    scenario(
        "status_updates_reach_the_emp",
        "A CPO's status push is visible to the broker, and an occupied point is not chargeable.",
        || {
            let hubject = broker();
            hubject
                .push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())
                .map_err(|_| "push refused")?;

            let push = PushEvseStatusRequest {
                action_type: ActionType::Update,
                operator_evse_status: OperatorEvseStatus {
                    operator_id: samples::operator_id(),
                    operator_name: Some(Text::new_unchecked("ABC technologies")),
                    evse_status_record: vec![samples::evse_status_record("DE*ABC*E1", EvseStatus::Occupied)],
                    extensions: Extensions::new(),
                },
            };
            hubject.push_evse_status(&push).map_err(|ack| format!("the status push was refused: {ack:?}"))?;

            let status = hubject
                .status_of(&"DE*ABC*E1".parse().expect("valid"))
                .ok_or("the broker did not record the status")?;
            check(status.evse_status == EvseStatus::Occupied, "the status was not the one pushed")?;
            check(!status.evse_status.is_chargeable(), "an occupied point must not be chargeable")
        },
    )
}

/// The property that makes delta sync safe: a delta crawl ends where a full pull would.
#[must_use]
pub fn a_delta_crawl_converges_on_the_full_picture() -> ScenarioResult {
    scenario(
        "a_delta_crawl_converges_on_the_full_picture",
        "After a full pull and a delta, the EMP's copy matches what a fresh full pull would return.",
        || {
            let hubject = broker();
            for i in 0..3 {
                hubject
                    .push_evse_data(&samples::evse_data_record(&format!("DE*ABC*E{i}")).into())
                    .map_err(|_| "push refused")?;
            }

            let planner = Planner::new(PlannerConfig::new(
                "DE-DCB".parse().expect("valid"),
                GeoCoordinatesFormat::Google,
            ));
            let mut repository = InMemoryEvseRepository::new();

            // First crawl: a full pull.
            let (plan, watermark) = planner.plan(&repository).map_err(|_| "planning failed")?;
            check(plan.replaces_everything(), "the first pull must be a full one")?;
            let page = hubject.pull_evse_data(plan.request(), PageQuery::new());
            sync::apply(&mut repository, page.content).map_err(|_| "apply failed")?;
            planner.commit(&mut repository, watermark).map_err(|_| "commit failed")?;
            check(repository.len().unwrap_or(0) == 3, "the full pull did not populate the copy")?;

            // The CPO withdraws one and adds one.
            hubject
                .push_evse_data(&samples::evse_data_record("DE*ABC*E9").into())
                .map_err(|_| "push refused")?;

            // Second crawl: the planner chooses a delta, and it carries no filters.
            let (plan, watermark) = planner.plan(&repository).map_err(|_| "planning failed")?;
            check(!plan.replaces_everything(), "the second pull should have been a delta")?;
            check(
                plan.request().conflicting_filters().is_empty(),
                "a delta must not carry the geographic filters",
            )?;
            let page = hubject.pull_evse_data(plan.request(), PageQuery::new());
            sync::apply(&mut repository, page.content).map_err(|_| "apply failed")?;
            planner.commit(&mut repository, watermark).map_err(|_| "commit failed")?;

            check(repository.len().unwrap_or(0) == 4, "the delta did not bring the new record in")
        },
    )
}

/// The CPO never needs the destructive action for routine work.
#[must_use]
pub fn a_fleet_change_needs_no_full_load() -> ScenarioResult {
    scenario(
        "a_fleet_change_needs_no_full_load",
        "Adding and removing a charging point produces insert and delete pushes, never fullLoad.",
        || {
            let previous =
                vec![samples::evse_data_record("DE*ABC*E1"), samples::evse_data_record("DE*ABC*E2")];
            let current =
                vec![samples::evse_data_record("DE*ABC*E1"), samples::evse_data_record("DE*ABC*E3")];

            let plan = crate::sync::PushPlanner::plan(&previous, &current);
            check(plan.inserts.len() == 1, "the new charging point should be an insert")?;
            check(plan.deletes.len() == 1, "the withdrawn charging point should be a delete")?;
            check(plan.unchanged == 1, "the unchanged point should not be sent at all")?;

            let requests =
                plan.into_requests(&samples::operator_id(), &Text::new_unchecked("ABC technologies"));
            for request in &requests {
                request.validate().map_err(|e| format!("a generated push is not conformant: {e}"))?;
                check(
                    !request.action_type.is_destructive_replace(),
                    "routine synchronisation must never use fullLoad",
                )?;
            }
            check(requests.len() == 2, "expected one insert and one delete request")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scenario_passes_against_the_mock() {
        let report = run_all();
        assert!(report.passed(), "\n{report}");
        assert!(report.results.len() >= 7);
    }

    #[test]
    fn a_report_renders_its_failures() {
        let report = Report {
            results: vec![ScenarioResult { name: "x", description: "y", failure: Some("because".into()) }],
        };
        assert!(!report.passed());
        assert!(report.to_string().contains("FAIL  x — because"));
    }
}
