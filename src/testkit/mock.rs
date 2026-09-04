//! `MockHubject` — a brokering system in a process, so integration can start before onboarding does.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::cpo::DeltaType;
use crate::cpo::{
    AuthorizationStartResponse, AuthorizationStatus, AuthorizationStopResponse, AuthorizeRemoteStartRequest,
    AuthorizeRemoteStopRequest, AuthorizeStartRequest, AuthorizeStopRequest, ChargeDetailRecord,
    ChargingNotification, EvseDataRecord, EvseStatusRecord, PushEvseDataRequest, PushEvseStatusRequest,
};
use crate::emp::{
    EvseStatusByIdResponse, EvseStatusRecords, EvseStatusResponse, EvseStatuses, OperatorEvseStatusRecords,
    Page, PullEvseDataRecord, PullEvseDataRequest, PullEvseStatusByIdRequest, PullEvseStatusRequest,
};
use crate::sync::{EvseRepository, InMemoryEvseRepository};
use crate::transport::PageQuery;
use crate::types::{
    Acknowledgement, ActionType, Code, DateTime, EvcoId, EvseId, Extensions, Identification, OperatorId,
    ProviderId, SessionId, StatusCode, Validate,
};

/// What a mock EMP decided about an authorization request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthorizationDecision {
    /// The driver may charge.
    Authorized,
    /// The driver may not, for this reason.
    Refused(Code),
}

/// An EMP registered with a [`MockHubject`].
///
/// The decision function is what makes a scenario a scenario: return
/// [`AuthorizationDecision::Refused`] for a particular card and you have an expired-contract test.
#[derive(Clone)]
pub struct MockEmp {
    /// The EMP's id. Contracts whose `EvcoID` names it route here.
    pub provider_id: ProviderId,
    /// How this EMP answers authorization requests.
    pub decide: Arc<dyn Fn(&Identification) -> AuthorizationDecision + Send + Sync>,
}

impl MockEmp {
    /// An EMP that authorizes everything — the usual starting point.
    #[must_use]
    pub fn permissive(provider_id: ProviderId) -> Self {
        Self { provider_id, decide: Arc::new(|_| AuthorizationDecision::Authorized) }
    }

    /// An EMP that refuses everything with `code`.
    #[must_use]
    pub fn refusing(provider_id: ProviderId, code: Code) -> Self {
        Self { provider_id, decide: Arc::new(move |_| AuthorizationDecision::Refused(code.clone())) }
    }

    /// An EMP that decides per identification.
    #[must_use]
    pub fn with(
        provider_id: ProviderId,
        decide: impl Fn(&Identification) -> AuthorizationDecision + Send + Sync + 'static,
    ) -> Self {
        Self { provider_id, decide: Arc::new(decide) }
    }
}

impl core::fmt::Debug for MockEmp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MockEmp").field("provider_id", &self.provider_id).finish_non_exhaustive()
    }
}

/// What the broker did with one request, for asserting on afterwards.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A CPO pushed EVSE data.
    EvseDataPushed {
        /// The operator.
        operator_id: OperatorId,
        /// What it asked for.
        action: ActionType,
        /// How many records.
        records: usize,
    },
    /// A CPO pushed EVSE status.
    EvseStatusPushed {
        /// The operator.
        operator_id: OperatorId,
        /// How many records.
        records: usize,
    },
    /// A driver was authorized, or not.
    Authorized {
        /// The session the broker opened, if it did.
        session_id: Option<SessionId>,
        /// Which EMP decided.
        provider_id: Option<ProviderId>,
        /// What it decided.
        authorized: bool,
    },
    /// A session was stopped.
    Stopped {
        /// The session.
        session_id: SessionId,
    },
    /// An EMP asked to start a session remotely.
    RemoteStartRequested {
        /// The session the broker opened.
        session_id: SessionId,
        /// Where.
        evse_id: EvseId,
    },
    /// A CDR was submitted.
    CdrSubmitted {
        /// The session.
        session_id: SessionId,
        /// Which EMP it was routed to, if the broker could tell.
        provider_id: Option<ProviderId>,
    },
    /// A charging notification arrived.
    NotificationReceived {
        /// The session.
        session_id: SessionId,
        /// Which kind.
        notification_type: crate::cpo::ChargingNotificationType,
    },
}

/// Why a stop request's medium does not match the one that opened the session, if it does not.
///
/// Compares only what both sides actually name. A bare RFID UID names no contract and a QR-code
/// identification names no card, so the broker — which can resolve one to the other and the mock
/// cannot — is given the benefit of the doubt rather than refusing a legitimate stop.
fn medium_mismatch(started_with: &Identification, stopping_with: &Identification) -> Option<String> {
    if let (Some(opened), Some(closing)) = (started_with.evco_id(), stopping_with.evco_id())
        && opened != closing
    {
        return Some(format!(
            "the session was started with contract {opened} and this stop presents {closing}; \
                 a session may only be stopped with the medium that started it"
        ));
    }
    if let (Some(opened), Some(closing)) = (started_with.uid(), stopping_with.uid())
        && opened != closing
    {
        return Some(format!(
            "the session was started with card {opened} and this stop presents {closing}; \
                 a session may only be stopped with the medium that started it"
        ));
    }
    None
}

/// A session the broker is tracking.
#[derive(Clone, Debug)]
pub struct MockSession {
    /// The session id.
    pub session_id: SessionId,
    /// Where it is happening.
    pub evse_id: EvseId,
    /// Who is charging.
    pub identification: Identification,
    /// Which EMP owns the contract, if the broker could route it.
    pub provider_id: Option<ProviderId>,
    /// Whether it has been stopped.
    pub stopped: bool,
    /// Whether a CDR has arrived for it.
    pub settled: bool,
}

/// A Hubject brokering system, in a process.
///
/// # Why this exists
///
/// Testing an OICP implementation against the real thing needs a signed contract, certificates
/// issued by Hubject's CA, and access to their QA environment — weeks of work before the first
/// request. Meanwhile the interesting failures are all in the *sequences*: authorize, charge,
/// notify, settle; remote start from an app; a CDR for a session nobody opened.
///
/// `MockHubject` is those sequences, offline. It routes like the real broker does — deriving the
/// operator from the `EvseID` and the provider from the `EvcoID` — tracks sessions, answers with
/// spec-accurate status codes, and records what happened.
///
/// ```
/// # use oicp_kit::testkit::{MockHubject, MockEmp, samples};
/// # use oicp_kit::types::Validate;
/// let mut hubject = MockHubject::new();
/// hubject.register_emp(MockEmp::permissive("DE-DCB".parse().unwrap()));
/// hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E1").into()).unwrap();
///
/// // A driver swipes a card.
/// let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
/// assert!(response.is_authorized());
/// # assert!(response.validate().is_ok());
/// ```
#[derive(Clone)]
pub struct MockHubject {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    evse_data: BTreeMap<String, InMemoryEvseRepository>,
    /// When each charging point last changed, and how. Without this the broker cannot answer a
    /// `LastCall` delta faithfully, and the delta engine's whole code path goes untested.
    history: BTreeMap<String, (DateTime, DeltaType)>,
    statuses: BTreeMap<String, EvseStatusRecord>,
    emps: Vec<MockEmp>,
    sessions: BTreeMap<String, MockSession>,
    cdrs: Vec<ChargeDetailRecord>,
    notifications: Vec<ChargingNotification>,
    events: Vec<Event>,
    next_session: u64,
}

impl Default for MockHubject {
    fn default() -> Self {
        Self::new()
    }
}

impl MockHubject {
    /// An empty brokering system.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                evse_data: BTreeMap::new(),
                history: BTreeMap::new(),
                statuses: BTreeMap::new(),
                emps: vec![],
                sessions: BTreeMap::new(),
                cdrs: vec![],
                notifications: vec![],
                events: vec![],
                next_session: 1,
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Registers an EMP. Contracts whose `EvcoID` names it route to it.
    pub fn register_emp(&mut self, emp: MockEmp) {
        self.lock().emps.push(emp);
    }

    /// Applies a `PushEvseData` the way the broker would.
    ///
    /// # Errors
    ///
    /// Returns the acknowledgement the broker would answer with, when the push is refused —
    /// a record whose `EvseID` names a different operator, for instance.
    pub fn push_evse_data(&self, request: &PushEvseDataRequest) -> Result<Acknowledgement, Acknowledgement> {
        if let Err(violations) = request.validate() {
            return Err(Acknowledgement::failure_with(Code::DataError, violations.to_string()));
        }
        let operator = request.operator_evse_data.operator_id.clone();
        let mut inner = self.lock();
        let repo = inner.evse_data.entry(operator.canonical()).or_default();

        if request.action_type.is_destructive_replace() {
            repo.clear().ok();
        }
        let records = &request.operator_evse_data.evse_data_record;
        let mut changes: Vec<(String, DeltaType)> = vec![];
        for record in records {
            let pull = to_pull_record(record, &operator, request.operator_evse_data.operator_name.as_str());
            let key = record.evse_id.canonical();
            if request.action_type == ActionType::Delete {
                repo.delete(&record.evse_id).ok();
                changes.push((key, DeltaType::Delete));
            } else {
                let is_new = repo.upsert(pull).unwrap_or(true);
                changes.push((key, if is_new { DeltaType::Insert } else { DeltaType::Update }));
            }
        }
        let now = DateTime::now();
        for (key, delta) in changes {
            inner.history.insert(key, (now.clone(), delta));
        }
        inner.events.push(Event::EvseDataPushed {
            operator_id: operator,
            action: request.action_type,
            records: records.len(),
        });
        Ok(Acknowledgement::success())
    }

    /// Applies a `PushEvseStatus`.
    ///
    /// # Errors
    ///
    /// Returns the refusal acknowledgement when the push is not conformant.
    pub fn push_evse_status(
        &self,
        request: &PushEvseStatusRequest,
    ) -> Result<Acknowledgement, Acknowledgement> {
        if let Err(violations) = request.validate() {
            return Err(Acknowledgement::failure_with(Code::DataError, violations.to_string()));
        }
        let mut inner = self.lock();
        let records = &request.operator_evse_status.evse_status_record;
        for record in records {
            inner.statuses.insert(record.evse_id.canonical(), record.clone());
        }
        inner.events.push(Event::EvseStatusPushed {
            operator_id: request.operator_evse_status.operator_id.clone(),
            records: records.len(),
        });
        Ok(Acknowledgement::success())
    }

    /// Answers a `PullEvseData` with one page.
    ///
    /// Records are ordered by `EvseID`, so a crawl is stable across pages.
    #[must_use]
    pub fn pull_evse_data(
        &self,
        request: &PullEvseDataRequest,
        query: PageQuery,
    ) -> Page<PullEvseDataRecord> {
        let inner = self.lock();

        // A `LastCall` pull returns *changes*, tagged, and omits everything unchanged — which is
        // the behaviour [`sync`](crate::sync) is built on, so the mock has to reproduce it or the
        // whole delta path goes untested. It also carries tombstones for withdrawn charging
        // points, which a scan of the live records alone could never produce.
        if let Some(since) = &request.last_call {
            let mut changed: Vec<PullEvseDataRecord> = vec![];
            for (key, (at, delta)) in &inner.history {
                if at <= since {
                    continue;
                }
                let live = inner
                    .evse_data
                    .values()
                    .find_map(|repo| repo.iter().find(|r| r.evse_id.canonical() == *key).cloned());
                match (delta, live) {
                    // A withdrawal is a tombstone: the EvseID, and nothing worth keeping.
                    (DeltaType::Delete, _) | (_, None) => {
                        let mut tombstone = crate::testkit::samples::pull_evse_data_record(key);
                        tombstone.delta_type = Some(DeltaType::Delete);
                        changed.push(tombstone);
                    }
                    (delta, Some(mut record)) => {
                        record.delta_type = Some(delta.clone());
                        changed.push(record);
                    }
                }
            }
            changed.sort_by_key(|r| r.evse_id.canonical());
            for record in &mut changed {
                if let Some(converted) =
                    record.geo_coordinates.to_format(request.geo_coordinates_response_format)
                {
                    record.geo_coordinates = converted;
                }
            }
            return paginate(changed, query);
        }

        let mut all: Vec<PullEvseDataRecord> = inner
            .evse_data
            .values()
            .flat_map(|repo| repo.iter().cloned().collect::<Vec<_>>())
            .filter(|record| {
                request.is_hubject_compatible.is_none_or(|want| want == record.is_hubject_compatible)
                    && request.is_open_24_hours.is_none_or(|want| want == record.is_open_24_hours)
                    && request.renewable_energy.is_none_or(|want| want == record.renewable_energy)
                    && request.operator_ids.as_ref().is_none_or(|wanted| wanted.contains(&record.operator_id))
            })
            .collect();
        all.sort_by_key(|r| r.evse_id.canonical());

        // The coordinates come back in the notation the EMP asked for — which is a real behaviour
        // of the broker, and one a client that assumes otherwise gets wrong.
        for record in &mut all {
            if let Some(converted) = record.geo_coordinates.to_format(request.geo_coordinates_response_format)
            {
                record.geo_coordinates = converted;
            }
        }

        paginate(all, query)
    }

    /// Answers a `PullEvseStatus` with everything the broker holds, grouped by operator.
    ///
    /// A charging point whose status no CPO has pushed is reported as
    /// [`EvseStatus::Unknown`](crate::cpo::EvseStatus::Unknown) rather than omitted — which is what
    /// the real broker does, and the difference matters to an EMP deciding whether to route a
    /// driver to it.
    #[must_use]
    pub fn pull_evse_status(&self, request: &PullEvseStatusRequest) -> EvseStatusResponse {
        let inner = self.lock();
        let mut by_operator: BTreeMap<String, (OperatorId, Vec<EvseStatusRecord>)> = BTreeMap::new();

        for repo in inner.evse_data.values() {
            for record in repo.iter() {
                let status = inner.statuses.get(&record.evse_id.canonical()).map_or_else(
                    || EvseStatusRecord {
                        evse_id: record.evse_id.clone(),
                        evse_status: crate::cpo::EvseStatus::Unknown,
                        extensions: Extensions::new(),
                    },
                    Clone::clone,
                );
                if let Some(wanted) = &request.evse_status
                    && status.evse_status != *wanted
                {
                    continue;
                }
                by_operator
                    .entry(record.operator_id.canonical())
                    .or_insert_with(|| (record.operator_id.clone(), vec![]))
                    .1
                    .push(status);
            }
        }

        EvseStatusResponse {
            evse_statuses: EvseStatuses {
                operator_evse_status: by_operator
                    .into_values()
                    .map(|(operator_id, records)| OperatorEvseStatusRecords {
                        operator_id,
                        operator_name: None,
                        evse_status_record: records,
                        extensions: Extensions::new(),
                    })
                    .collect(),
            },
            status_code: Some(Code::Success.into()),
            extensions: Extensions::new(),
        }
    }

    /// Answers a `PullEvseStatusByID`.
    ///
    /// An id the broker does not know comes back as
    /// [`EvseStatus::EvseNotFound`](crate::cpo::EvseStatus::EvseNotFound) — the code the
    /// specification defines for exactly this — rather than being dropped from the answer, so an
    /// EMP can tell "not found" from "not returned".
    #[must_use]
    pub fn pull_evse_status_by_id(&self, request: &PullEvseStatusByIdRequest) -> EvseStatusByIdResponse {
        let inner = self.lock();
        let records = request
            .evse_id
            .iter()
            .map(|evse_id| {
                inner.statuses.get(&evse_id.canonical()).cloned().unwrap_or_else(|| {
                    let known = inner.evse_data.values().any(|r| r.get(evse_id).ok().flatten().is_some());
                    EvseStatusRecord {
                        evse_id: evse_id.clone(),
                        evse_status: if known {
                            crate::cpo::EvseStatus::Unknown
                        } else {
                            crate::cpo::EvseStatus::EvseNotFound
                        },
                        extensions: Extensions::new(),
                    }
                })
            })
            .collect();

        EvseStatusByIdResponse {
            evse_status_records: EvseStatusRecords { evse_status_record: records },
            status_code: Some(Code::Success.into()),
            extensions: Extensions::new(),
        }
    }

    /// Answers a CPO's `AuthorizeStart` by routing it to the EMP that owns the contract.
    #[must_use]
    pub fn authorize_start(&self, request: &AuthorizeStartRequest) -> AuthorizationStartResponse {
        let Some(evse_id) = request.evse_id.clone() else {
            // Legal per the spec, but the mock cannot route without knowing where the driver is.
            return self.record_refusal(AuthorizationStartResponse::not_authorized(Code::DataError));
        };
        if !self.knows_evse(&evse_id) {
            return self.record_refusal(AuthorizationStartResponse::not_authorized(Code::UnknownEvseId));
        }
        if !self.evse_is_hubject_compatible(&evse_id) {
            return self.record_refusal(AuthorizationStartResponse::not_authorized(
                Code::EvseIdNotHubjectCompatible,
            ));
        }

        // Hubject routes on the contract in the identification. A bare RFID UID names no contract,
        // so the real broker asks every EMP that has the card; the mock asks the first registered
        // EMP, which is the same shape of answer.
        let emp = self.route(request.identification.evco_id());
        let Some(emp) = emp else {
            return self.record_refusal(AuthorizationStartResponse::not_authorized(Code::PartnerNotFound));
        };

        match (emp.decide)(&request.identification) {
            AuthorizationDecision::Refused(code) => {
                let mut response = AuthorizationStartResponse::not_authorized(code);
                response.provider_id = Some(emp.provider_id.clone());
                self.record_refusal(response)
            }
            AuthorizationDecision::Authorized => {
                let session_id = self.new_session_id();
                let mut inner = self.lock();
                inner.sessions.insert(
                    session_id.as_str().to_owned(),
                    MockSession {
                        session_id: session_id.clone(),
                        evse_id,
                        identification: request.identification.clone(),
                        provider_id: Some(emp.provider_id.clone()),
                        stopped: false,
                        settled: false,
                    },
                );
                inner.events.push(Event::Authorized {
                    session_id: Some(session_id.clone()),
                    provider_id: Some(emp.provider_id.clone()),
                    authorized: true,
                });
                let mut response = AuthorizationStartResponse::authorized(session_id);
                response.provider_id = Some(emp.provider_id.clone());
                response
            }
        }
    }

    /// Answers a CPO's `AuthorizeStop`.
    #[must_use]
    pub fn authorize_stop(&self, request: &AuthorizeStopRequest) -> AuthorizationStopResponse {
        let mut inner = self.lock();
        let Some(session) = inner.sessions.get_mut(request.session_id.as_str()) else {
            return AuthorizationStopResponse {
                session_id: Some(request.session_id.clone()),
                cpo_partner_session_id: None,
                emp_partner_session_id: None,
                provider_id: None,
                authorization_status: AuthorizationStatus::NotAuthorized,
                status_code: Code::SessionIsInvalid.into(),
                extensions: Extensions::new(),
            };
        };

        // "the session `MUST` only be stopped with the same medium, which was used for starting
        // the session" — CPO 2.3, eRoamingAuthorizeStop. The broker knows which medium opened the
        // session because it opened it; a CPO that lets one driver stop another's session finds
        // out here rather than in Hubject's integration test.
        if let Some(reason) = medium_mismatch(&session.identification, &request.identification) {
            let provider_id = session.provider_id.clone();
            drop(inner);
            return AuthorizationStopResponse {
                session_id: Some(request.session_id.clone()),
                cpo_partner_session_id: request.cpo_partner_session_id.clone(),
                emp_partner_session_id: request.emp_partner_session_id.clone(),
                provider_id,
                authorization_status: AuthorizationStatus::NotAuthorized,
                status_code: StatusCode::with_info(Code::SessionIsInvalid, reason),
                extensions: Extensions::new(),
            };
        }
        session.stopped = true;
        let provider_id = session.provider_id.clone();
        inner.events.push(Event::Stopped { session_id: request.session_id.clone() });
        AuthorizationStopResponse {
            session_id: Some(request.session_id.clone()),
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            provider_id,
            authorization_status: AuthorizationStatus::Authorized,
            status_code: Code::Success.into(),
            extensions: Extensions::new(),
        }
    }

    /// Turns an EMP's remote-start request into the one the broker would send the CPO.
    ///
    /// This is the direction most implementations forget: the answer is what *your*
    /// [`CpoService`](crate::server::CpoService) will be asked to handle.
    ///
    /// # Errors
    ///
    /// Returns the acknowledgement the broker would answer the EMP with, when it will not route
    /// the request at all — an unknown `EvseID`, or one that is not Hubject compatible.
    pub fn remote_start(
        &self,
        provider_id: &ProviderId,
        evse_id: &EvseId,
        evco_id: &EvcoId,
    ) -> Result<AuthorizeRemoteStartRequest, Acknowledgement> {
        if !self.knows_evse(evse_id) {
            return Err(Acknowledgement::failure(Code::UnknownEvseId));
        }
        if !self.evse_is_hubject_compatible(evse_id) {
            return Err(Acknowledgement::failure(Code::EvseIdNotHubjectCompatible));
        }
        let session_id = self.new_session_id();
        let identification =
            Identification::Remote(crate::types::RemoteIdentification { evco_id: evco_id.clone() });
        let mut inner = self.lock();
        inner.sessions.insert(
            session_id.as_str().to_owned(),
            MockSession {
                session_id: session_id.clone(),
                evse_id: evse_id.clone(),
                identification: identification.clone(),
                provider_id: Some(provider_id.clone()),
                stopped: false,
                settled: false,
            },
        );
        inner
            .events
            .push(Event::RemoteStartRequested { session_id: session_id.clone(), evse_id: evse_id.clone() });
        Ok(AuthorizeRemoteStartRequest {
            session_id,
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            provider_id: provider_id.clone(),
            evse_id: evse_id.clone(),
            identification,
            partner_product_id: None,
            extensions: Extensions::new(),
        })
    }

    /// Turns an EMP's remote-stop request into the one the broker would send the CPO.
    ///
    /// # Errors
    ///
    /// Returns the refusal when the session is not one the broker opened.
    pub fn remote_stop(
        &self,
        provider_id: &ProviderId,
        session_id: &SessionId,
    ) -> Result<AuthorizeRemoteStopRequest, Acknowledgement> {
        let inner = self.lock();
        let Some(session) = inner.sessions.get(session_id.as_str()) else {
            return Err(Acknowledgement::failure(Code::SessionIsInvalid));
        };
        Ok(AuthorizeRemoteStopRequest {
            session_id: session_id.clone(),
            cpo_partner_session_id: None,
            emp_partner_session_id: None,
            provider_id: provider_id.clone(),
            evse_id: session.evse_id.clone(),
            extensions: Extensions::new(),
        })
    }

    /// Accepts a CDR, checking it against the session it claims to settle.
    ///
    /// # Errors
    ///
    /// Returns the refusal acknowledgement the broker would answer with: `400 Session is invalid`
    /// for a session it never opened, `022 Data error` for a CDR that is not conformant.
    pub fn submit_cdr(&self, cdr: &ChargeDetailRecord) -> Result<Acknowledgement, Acknowledgement> {
        if let Err(violations) = cdr.validate() {
            return Err(Acknowledgement::failure_with(Code::DataError, violations.to_string()));
        }
        let mut inner = self.lock();
        let Some(session) = inner.sessions.get_mut(cdr.session_id.as_str()) else {
            // The real broker does exactly this, and it is the most common integration failure:
            // a CPO that invents its own session ids finds every CDR refused.
            return Err(Acknowledgement::failure_with(
                Code::SessionIsInvalid,
                format!("no session {} was opened through this broker", cdr.session_id),
            ));
        };
        if session.evse_id != cdr.evse_id {
            return Err(Acknowledgement::failure_with(
                Code::DataError,
                format!("session {} was opened at {}, not {}", cdr.session_id, session.evse_id, cdr.evse_id),
            ));
        }
        // "Hubject will accept only one CDR per SessionID." — CPO 2.3, eRoamingChargeDetailRecord.
        // This is the rule a retry meets: a CPO whose first submission timed out sends the record
        // again, and the second is refused because the first landed. A broker that accepts both
        // teaches a partner's reconciliation to expect two acknowledgements for one session, and
        // the real one gives it one and a refusal.
        if session.settled {
            return Err(Acknowledgement::failure_with(
                Code::SessionIsInvalid,
                format!(
                    "a CDR for session {} has already been accepted; Hubject accepts only one per \
                     SessionID, so a resubmission is refused rather than settled twice",
                    cdr.session_id
                ),
            ));
        }
        session.settled = true;
        let provider_id = session.provider_id.clone();
        inner.cdrs.push(cdr.clone());
        inner.events.push(Event::CdrSubmitted { session_id: cdr.session_id.clone(), provider_id });
        Ok(Acknowledgement::success().with_session(cdr.session_id.clone()))
    }

    /// Accepts a charging notification.
    ///
    /// # Errors
    ///
    /// Returns the refusal when the notification is not conformant, or names a session the broker
    /// did not open.
    pub fn notify(&self, notification: &ChargingNotification) -> Result<Acknowledgement, Acknowledgement> {
        if let Err(violations) = notification.validate() {
            return Err(Acknowledgement::failure_with(Code::DataError, violations.to_string()));
        }
        let mut inner = self.lock();
        if !inner.sessions.contains_key(notification.session_id().as_str()) {
            return Err(Acknowledgement::failure(Code::SessionIsInvalid));
        }
        inner.events.push(Event::NotificationReceived {
            session_id: notification.session_id().clone(),
            notification_type: notification.notification_type(),
        });
        inner.notifications.push(notification.clone());
        Ok(Acknowledgement::success())
    }

    /// Everything the broker did, in order.
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        self.lock().events.clone()
    }

    /// The sessions the broker has opened.
    #[must_use]
    pub fn sessions(&self) -> Vec<MockSession> {
        self.lock().sessions.values().cloned().collect()
    }

    /// The CDRs it has accepted.
    #[must_use]
    pub fn cdrs(&self) -> Vec<ChargeDetailRecord> {
        self.lock().cdrs.clone()
    }

    /// The notifications it has accepted.
    #[must_use]
    pub fn notifications(&self) -> Vec<ChargingNotification> {
        self.lock().notifications.clone()
    }

    /// How many charging points it holds.
    #[must_use]
    pub fn evse_count(&self) -> usize {
        self.lock().evse_data.values().filter_map(|r| usize::try_from(r.len().ok()?).ok()).sum()
    }

    /// The status it last saw for `evse_id`.
    #[must_use]
    pub fn status_of(&self, evse_id: &EvseId) -> Option<EvseStatusRecord> {
        self.lock().statuses.get(&evse_id.canonical()).cloned()
    }

    fn knows_evse(&self, evse_id: &EvseId) -> bool {
        self.lock().evse_data.values().any(|repo| repo.get(evse_id).ok().flatten().is_some())
    }

    fn evse_is_hubject_compatible(&self, evse_id: &EvseId) -> bool {
        self.lock()
            .evse_data
            .values()
            .find_map(|repo| repo.get(evse_id).ok().flatten())
            .is_some_and(|r| r.is_hubject_compatible)
    }

    fn route(&self, evco_id: Option<&EvcoId>) -> Option<MockEmp> {
        let inner = self.lock();
        match evco_id {
            Some(evco) => {
                let provider = evco.provider_id();
                inner.emps.iter().find(|e| e.provider_id == provider).cloned()
            }
            // A bare RFID UID names no contract; the broker falls back to asking whoever is there.
            None => inner.emps.first().cloned(),
        }
    }

    fn new_session_id(&self) -> SessionId {
        let mut inner = self.lock();
        let n = inner.next_session;
        inner.next_session += 1;
        // Deterministic, and shaped like the GUID the spec requires.
        SessionId::new_unchecked(format!("{n:08x}-0000-4000-8000-{n:012x}"))
    }

    fn record_refusal(&self, response: AuthorizationStartResponse) -> AuthorizationStartResponse {
        self.lock().events.push(Event::Authorized {
            session_id: None,
            provider_id: response.provider_id.clone(),
            authorized: false,
        });
        response
    }
}

/// Splits `all` into the page `query` asks for, with spec-accurate metadata.
fn paginate<T>(all: Vec<T>, query: PageQuery) -> Page<T> {
    let size = query.size.max(1);
    let total_elements = all.len() as u64;
    let total_pages = u32::try_from(total_elements.div_ceil(u64::from(size))).unwrap_or(u32::MAX);
    let start = (query.page as usize).saturating_mul(size as usize);
    let content: Vec<T> = all.into_iter().skip(start).take(size as usize).collect();
    Page {
        number_of_elements: u32::try_from(content.len()).unwrap_or(u32::MAX),
        empty: Some(content.is_empty()),
        content,
        number: query.page,
        size,
        total_elements,
        total_pages,
        first: query.page == 0,
        last: total_pages == 0 || query.page + 1 >= total_pages,
        pageable: None,
        status_code: Some(Code::Success.into()),
        extensions: Extensions::new(),
    }
}

/// Turns a CPO's record into the EMP's view of it, stamping the fields Hubject owns.
fn to_pull_record(
    record: &EvseDataRecord,
    operator_id: &OperatorId,
    operator_name: &str,
) -> PullEvseDataRecord {
    let mut pull = PullEvseDataRecord::from_evse_data_record(
        record.clone(),
        operator_id.clone(),
        crate::types::Text::new_unchecked(operator_name),
    );
    // Hubject stamps this on every record it stores, whatever the CPO sent.
    pull.last_update = Some(crate::types::DateTime::now());
    pull
}

/// A one-record push, for seeding a test.
///
/// The action is [`ActionType::Insert`], never `fullLoad` — the same rule the rest of the crate
/// follows. A shorthand that quietly replaced the operator's whole fleet would make
/// `for r in fleet { hubject.push_evse_data(&r.into()) }` leave exactly one record behind, which
/// is a confusing way to learn what `fullLoad` does.
impl From<EvseDataRecord> for PushEvseDataRequest {
    fn from(record: EvseDataRecord) -> Self {
        let operator_id = record.evse_id.operator_id();
        Self {
            action_type: ActionType::Insert,
            operator_evse_data: crate::cpo::OperatorEvseData {
                operator_id,
                operator_name: crate::types::Text::new_unchecked("Mock Operator"),
                evse_data_record: vec![record],
                extensions: Extensions::new(),
            },
        }
    }
}

// Fixtures come from `testkit::samples`, so these tests compile when that feature is on.
#[cfg(all(test, feature = "testkit"))]
mod tests {
    use super::*;
    use crate::cpo::AuthorizationStatus;
    use crate::testkit::samples;
    use crate::types::{QrCodeIdentification, RfidMifareFamilyIdentification, Uid};

    fn broker() -> MockHubject {
        let mut hubject = MockHubject::new();
        hubject.register_emp(MockEmp::permissive("DE-DCB".parse().unwrap()));
        hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E1").into()).unwrap();
        hubject
    }

    #[test]
    fn a_push_makes_the_charging_point_pullable() {
        let hubject = broker();
        assert_eq!(hubject.evse_count(), 1);

        let request =
            PullEvseDataRequest::full("DE-DCB".parse().unwrap(), crate::types::GeoCoordinatesFormat::Google);
        let page = hubject.pull_evse_data(&request, PageQuery::new());
        assert_eq!(page.content.len(), 1);
        assert_eq!(page.total_elements, 1);
        assert!(page.first && page.last);
        assert!(page.validate().is_ok(), "the mock's pages are spec-accurate");
    }

    #[test]
    fn the_full_sequence_runs_offline() {
        let hubject = broker();

        // Authorize.
        let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
        assert!(response.is_authorized());
        assert!(response.validate().is_ok());
        let session_id = response.session_id.clone().unwrap();

        // Notify.
        let notification = ChargingNotification::Start(samples::charging_notification_start(
            "DE*ABC*E1",
            session_id.clone(),
        ));
        assert!(hubject.notify(&notification).is_ok());

        // Settle.
        let cdr = samples::charge_detail_record("DE*ABC*E1", session_id.clone());
        assert!(hubject.submit_cdr(&cdr).is_ok());

        assert_eq!(hubject.cdrs().len(), 1);
        assert!(hubject.sessions()[0].settled);
        assert!(matches!(hubject.events().last(), Some(Event::CdrSubmitted { .. })));
    }

    #[test]
    fn a_cdr_for_a_session_nobody_opened_is_refused() {
        let hubject = broker();
        let cdr = samples::charge_detail_record("DE*ABC*E1", samples::session_id());
        let refusal = hubject.submit_cdr(&cdr).unwrap_err();
        assert_eq!(*refusal.code(), Code::SessionIsInvalid);
        assert!(refusal.validate().is_ok());
    }

    #[test]
    fn only_one_cdr_per_session_is_accepted() {
        // "Hubject will accept only one CDR per SessionID." A CPO whose first submission timed out
        // sends the record again; the second is refused, and a broker that accepts both teaches a
        // partner's reconciliation to expect two acknowledgements for one session.
        let hubject = broker();
        let session = hubject
            .authorize_start(&samples::authorize_start_request("DE*ABC*E1"))
            .session_id
            .expect("the session opened");
        let cdr = samples::charge_detail_record("DE*ABC*E1", session);

        assert!(hubject.submit_cdr(&cdr).is_ok(), "the first CDR settles");
        let refusal = hubject.submit_cdr(&cdr).unwrap_err();
        assert_eq!(*refusal.code(), Code::SessionIsInvalid);
        assert!(refusal.to_string().contains("already been accepted"), "{refusal}");
        assert_eq!(hubject.cdrs().len(), 1, "the record was stored twice");
        assert!(refusal.validate().is_ok());
    }

    #[test]
    fn a_session_can_only_be_stopped_with_the_medium_that_started_it() {
        // CPO 2.3, eRoamingAuthorizeStop: "the session `MUST` only be stopped with the same
        // medium, which was used for starting the session". The broker stored that medium and,
        // until this test, never read it.
        let hubject = broker();
        let session = hubject
            .authorize_start(&samples::authorize_start_request("DE*ABC*E1"))
            .session_id
            .expect("the session opened");

        let mut wrong = samples::authorize_stop_request("DE*ABC*E1", session.clone());
        wrong.identification = Identification::RfidMifareFamily(RfidMifareFamilyIdentification {
            uid: Uid::new("AABBCCDDEEFF11").expect("valid"),
        });
        let refused = hubject.authorize_stop(&wrong);
        assert_eq!(refused.authorization_status, AuthorizationStatus::NotAuthorized);
        assert_eq!(refused.status_code.code, Code::SessionIsInvalid);
        assert!(refused.status_code.additional_info.is_some(), "the refusal says which medium");

        // The card that opened it stops it, and the session is not left half-stopped by the refusal.
        let right = samples::authorize_stop_request("DE*ABC*E1", session);
        assert_eq!(hubject.authorize_stop(&right).authorization_status, AuthorizationStatus::Authorized);
    }

    #[test]
    fn a_stop_that_names_no_comparable_medium_is_not_refused() {
        // A bare RFID UID names no contract and a QR code names no card. Hubject can resolve one
        // to the other and this broker cannot, so it gives the benefit of the doubt rather than
        // refusing a legitimate stop.
        let hubject = broker();
        let session = hubject
            .authorize_start(&samples::authorize_start_request("DE*ABC*E1"))
            .session_id
            .expect("the session opened");

        let mut by_contract = samples::authorize_stop_request("DE*ABC*E1", session);
        by_contract.identification = Identification::QrCode(QrCodeIdentification {
            evco_id: "DE-DCB-C12345678-X".parse().expect("valid"),
            hashed_pin: None,
            pin: Some("1234".to_owned()),
        });
        assert_eq!(
            hubject.authorize_stop(&by_contract).authorization_status,
            AuthorizationStatus::Authorized
        );
    }

    #[test]
    fn an_unknown_charging_point_is_refused_with_the_specs_own_code() {
        let hubject = broker();
        let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E999"));
        assert!(!response.is_authorized());
        assert_eq!(response.status_code.code, Code::UnknownEvseId);
    }

    #[test]
    fn a_refusing_emp_produces_a_refusal_with_its_reason() {
        let mut hubject = MockHubject::new();
        hubject.register_emp(MockEmp::refusing("DE-DCB".parse().unwrap(), Code::NoValidContract));
        hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E1").into()).unwrap();

        let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
        assert!(!response.is_authorized());
        assert_eq!(response.status_code.code, Code::NoValidContract);
        assert!(response.status_code.code.is_authorization_failure());
        assert!(response.validate().is_ok());
    }

    #[test]
    fn a_remote_start_produces_the_request_the_cpo_will_receive() {
        let hubject = broker();
        let request = hubject
            .remote_start(
                &"DE-DCB".parse().unwrap(),
                &"DE*ABC*E1".parse().unwrap(),
                &"DE-DCB-C12345678-X".parse().unwrap(),
            )
            .unwrap();
        // …and it is conformant, including the rule that only RemoteIdentification is allowed.
        assert!(request.validate().is_ok());
        assert!(matches!(request.identification, Identification::Remote(_)));
    }

    #[test]
    fn a_remote_start_at_an_incompatible_point_is_refused() {
        let mut hubject = MockHubject::new();
        hubject.register_emp(MockEmp::permissive("DE-DCB".parse().unwrap()));
        let mut record = samples::evse_data_record("DE*ABC*E1");
        record.is_hubject_compatible = false;
        hubject.push_evse_data(&record.into()).unwrap();

        let refusal = hubject
            .remote_start(
                &"DE-DCB".parse().unwrap(),
                &"DE*ABC*E1".parse().unwrap(),
                &"DE-DCB-C12345678-X".parse().unwrap(),
            )
            .unwrap_err();
        assert_eq!(*refusal.code(), Code::EvseIdNotHubjectCompatible);
    }

    #[test]
    fn a_delta_pull_returns_only_what_changed_since_the_watermark() {
        let hubject = broker();
        let request =
            PullEvseDataRequest::full("DE-DCB".parse().unwrap(), crate::types::GeoCoordinatesFormat::Google);
        assert_eq!(
            hubject.pull_evse_data(&request, PageQuery::new()).content.len(),
            1,
            "a full pull sees it"
        );

        // A delta from *after* the push sees nothing: there is nothing new.
        let after = crate::types::DateTime::now();
        let delta = PullEvseDataRequest::delta(
            "DE-DCB".parse().unwrap(),
            crate::types::GeoCoordinatesFormat::Google,
            after.clone(),
        );
        let page = hubject.pull_evse_data(&delta, PageQuery::new());
        assert!(page.content.is_empty(), "an unchanged world produces an empty delta");

        // A change after the watermark shows up, tagged.
        hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E2").into()).unwrap();
        let page = hubject.pull_evse_data(&delta, PageQuery::new());
        assert_eq!(page.content.len(), 1, "only the new record");
        assert_eq!(page.content[0].delta_type, Some(crate::cpo::DeltaType::Insert));
        assert_eq!(page.content[0].evse_id.canonical(), "DEABCE2");
    }

    #[test]
    fn a_withdrawal_comes_back_as_a_tombstone() {
        let hubject = broker();
        let after = crate::types::DateTime::now();
        let delta = PullEvseDataRequest::delta(
            "DE-DCB".parse().unwrap(),
            crate::types::GeoCoordinatesFormat::Google,
            after,
        );

        let mut withdrawal: PushEvseDataRequest = samples::evse_data_record("DE*ABC*E1").into();
        withdrawal.action_type = ActionType::Delete;
        hubject.push_evse_data(&withdrawal).unwrap();

        let page = hubject.pull_evse_data(&delta, PageQuery::new());
        assert_eq!(page.content.len(), 1);
        assert!(page.content[0].is_deletion(), "a withdrawn charging point is a delete, not an update");
        assert_eq!(hubject.evse_count(), 0);
    }

    #[test]
    fn pulls_come_back_in_the_notation_the_emp_asked_for() {
        let hubject = broker();
        let request = PullEvseDataRequest::full(
            "DE-DCB".parse().unwrap(),
            crate::types::GeoCoordinatesFormat::DecimalDegree,
        );
        let page = hubject.pull_evse_data(&request, PageQuery::new());
        assert_eq!(
            page.content[0].geo_coordinates.format(),
            crate::types::GeoCoordinatesFormat::DecimalDegree
        );
    }

    #[test]
    fn a_crawl_over_several_pages_sees_every_record_once() {
        let mut hubject = MockHubject::new();
        hubject.register_emp(MockEmp::permissive("DE-DCB".parse().unwrap()));
        for i in 0..25 {
            hubject.push_evse_data(&samples::evse_data_record(&format!("DE*ABC*E{i}")).into()).unwrap();
        }
        let request =
            PullEvseDataRequest::full("DE-DCB".parse().unwrap(), crate::types::GeoCoordinatesFormat::Google);

        let mut seen = vec![];
        let mut query = Some(PageQuery::with_size(10));
        while let Some(current) = query {
            let page = hubject.pull_evse_data(&request, current);
            assert!(page.validate().is_ok());
            seen.extend(page.content.iter().map(|r| r.evse_id.canonical()));
            query = page.next_page().map(|n| PageQuery::at(n, 10));
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 25, "every record exactly once");
    }
}
