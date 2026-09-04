//! `CpoClient` — everything a Charge Point Operator sends to Hubject.

use super::http::{ClientConfig, Transport, warn_on_identity_mismatch};
use super::identity::{ClientIdentity, IdentityWarning};
use crate::cpo::{
    AuthorizationStartResponse, AuthorizationStopResponse, AuthorizeStartRequest, AuthorizeStopRequest,
    ChargeDetailRecord, ChargingNotification, EvseDataRecord, EvseStatusRecord, OperatorEvseData,
    OperatorEvseStatus, PushEvseDataRequest, PushEvsePricingRequest, PushEvseStatusRequest,
    PushPricingProductDataRequest,
};
use crate::sync::{PushPlan, PushPlanner};
use crate::transport::{HubjectEnv, OicpError, Operation, PathId, Result};
use crate::types::{Acknowledgement, ActionType, Extensions, OperatorId, Text};

/// The CPO's client: pushes, authorization requests, CDRs and notifications.
///
/// Build it with [`CpoClient::builder`]. The builder checks the operator id against the client
/// certificate, so the commonest OICP misconfiguration is caught before the first request rather
/// than as a `017` on every one.
#[derive(Debug, Clone)]
pub struct CpoClient {
    transport: Transport,
    operator_id: OperatorId,
    identity_warning: Option<IdentityWarning>,
}

/// Builds a [`CpoClient`].
#[derive(Default)]
pub struct CpoClientBuilder {
    environment: Option<HubjectEnv>,
    operator_id: Option<OperatorId>,
    identity: Option<ClientIdentity>,
    config: Option<ClientConfig>,
}

impl CpoClientBuilder {
    /// Which brokering system to talk to. Defaults to [`HubjectEnv::Qa`].
    #[must_use]
    pub fn environment(mut self, environment: HubjectEnv) -> Self {
        self.environment = Some(environment);
        self
    }

    /// The operator this client acts as. Goes in every URL path.
    #[must_use]
    pub fn operator_id(mut self, operator_id: OperatorId) -> Self {
        self.operator_id = Some(operator_id);
        self
    }

    /// The client certificate and key Hubject issued.
    #[must_use]
    pub fn identity(mut self, identity: ClientIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    /// The full configuration, for timeouts and retry policy.
    #[must_use]
    pub fn config(mut self, config: ClientConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// Returns [`OicpError::Transport`] when no operator id was given, or the TLS identity cannot
    /// be used.
    pub fn build(self) -> Result<CpoClient> {
        let operator_id = self.operator_id.ok_or_else(|| {
            OicpError::transport("a CpoClient needs an OperatorID: it goes in every URL path")
        })?;
        let mut config = self.config.unwrap_or_default();
        if let Some(environment) = self.environment {
            config.environment = environment;
        }
        let identity_warning = warn_on_identity_mismatch(self.identity.as_ref(), operator_id.as_str());
        let transport = Transport::new(config, self.identity.as_ref())?;
        Ok(CpoClient { transport, operator_id, identity_warning })
    }
}

impl CpoClient {
    /// A builder.
    #[must_use]
    pub fn builder() -> CpoClientBuilder {
        CpoClientBuilder::default()
    }

    /// The operator this client acts as.
    #[must_use]
    pub const fn operator_id(&self) -> &OperatorId {
        &self.operator_id
    }

    /// The underlying transport, for an operation this crate has not modelled.
    #[must_use]
    pub const fn transport(&self) -> &Transport {
        &self.transport
    }

    /// The certificate mismatch found at construction, if there was one.
    ///
    /// Already logged at `WARN`. Read it if you want to refuse to start.
    #[must_use]
    pub const fn identity_warning(&self) -> Option<&IdentityWarning> {
        self.identity_warning.as_ref()
    }

    fn path_id(&self) -> PathId {
        PathId::Operator(self.operator_id.clone())
    }

    // --- EVSE data ------------------------------------------------------------------------

    /// Sends a `PushEvseData` exactly as given.
    ///
    /// Prefer [`push_evse_data_plan`](Self::push_evse_data_plan) for routine synchronisation: it
    /// cannot accidentally carry [`ActionType::FullLoad`].
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn push_evse_data(&self, request: &PushEvseDataRequest) -> Result<Acknowledgement> {
        self.transport.post(Operation::PushEvseData, &self.path_id(), request).await
    }

    /// Adds charging points to Hubject's copy of this operator's fleet.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn push_evse_data_insert(
        &self,
        records: Vec<EvseDataRecord>,
        operator_name: impl Into<String>,
    ) -> Result<Acknowledgement> {
        self.push_with_action(ActionType::Insert, records, operator_name).await
    }

    /// Updates charging points in Hubject's copy.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn push_evse_data_update(
        &self,
        records: Vec<EvseDataRecord>,
        operator_name: impl Into<String>,
    ) -> Result<Acknowledgement> {
        self.push_with_action(ActionType::Update, records, operator_name).await
    }

    /// Withdraws charging points from Hubject's copy.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn push_evse_data_delete(
        &self,
        records: Vec<EvseDataRecord>,
        operator_name: impl Into<String>,
    ) -> Result<Acknowledgement> {
        self.push_with_action(ActionType::Delete, records, operator_name).await
    }

    /// Sends the minimal set of pushes that brings Hubject's copy up to date.
    ///
    /// The safe way to synchronise a fleet: [`PushPlanner`] works out what actually changed, and
    /// the requests it produces are never `fullLoad`.
    ///
    /// ```no_run
    /// # use oicp_kit::client::CpoClient;
    /// # use oicp_kit::sync::PushPlanner;
    /// # async fn example(client: &CpoClient, previous: Vec<oicp_kit::cpo::EvseDataRecord>, current: Vec<oicp_kit::cpo::EvseDataRecord>)
    /// # -> Result<(), Box<dyn std::error::Error>> {
    /// let plan = PushPlanner::plan(&previous, &current);
    /// let acknowledgements = client.push_evse_data_plan(plan, "ABC technologies").await?;
    /// # let _ = acknowledgements;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Stops at the first failure and returns it, having sent everything before it.
    pub async fn push_evse_data_plan(
        &self,
        plan: PushPlan,
        operator_name: impl Into<String>,
    ) -> Result<Vec<Acknowledgement>> {
        let name = Text::new_unchecked(operator_name.into());
        let mut acknowledgements = vec![];
        for request in plan.into_requests(&self.operator_id, &name) {
            acknowledgements.push(self.push_evse_data(&request).await?);
        }
        Ok(acknowledgements)
    }

    /// **Replaces** Hubject's copy of this operator's entire fleet.
    ///
    /// # Every charging point not in `records` is withdrawn
    ///
    /// Including from every EMP's map, until the next push. Use
    /// [`push_evse_data_plan`](Self::push_evse_data_plan) for routine synchronisation. This is for
    /// a deliberate re-baseline, with a fleet list you are sure is complete.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn push_evse_data_full_load(
        &self,
        records: Vec<EvseDataRecord>,
        operator_name: impl Into<String>,
    ) -> Result<Acknowledgement> {
        if self.transport.is_production() {
            tracing::warn!(
                target: "oicp_kit::client",
                operator = %self.operator_id,
                records = records.len(),
                "sending a fullLoad to the production brokering system: every charging point of this \
                 operator that is not in this push will be withdrawn from the roaming network"
            );
        }
        let request = PushPlanner::full_load(
            self.operator_id.clone(),
            Text::new_unchecked(operator_name.into()),
            records,
        );
        self.push_evse_data(&request).await
    }

    async fn push_with_action(
        &self,
        action_type: ActionType,
        records: Vec<EvseDataRecord>,
        operator_name: impl Into<String>,
    ) -> Result<Acknowledgement> {
        let request = PushEvseDataRequest {
            action_type,
            operator_evse_data: OperatorEvseData {
                operator_id: self.operator_id.clone(),
                operator_name: Text::new_unchecked(operator_name.into()),
                evse_data_record: records,
                extensions: Extensions::new(),
            },
        };
        self.push_evse_data(&request).await
    }

    // --- EVSE status ----------------------------------------------------------------------

    /// Sends a `PushEvseStatus` exactly as given.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn push_evse_status(&self, request: &PushEvseStatusRequest) -> Result<Acknowledgement> {
        self.transport.post(Operation::PushEvseStatus, &self.path_id(), request).await
    }

    /// Updates the status of some charging points.
    ///
    /// The spec recommends sending status every one to five minutes.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn update_evse_status(&self, records: Vec<EvseStatusRecord>) -> Result<Acknowledgement> {
        let request = PushEvseStatusRequest {
            action_type: ActionType::Update,
            operator_evse_status: OperatorEvseStatus {
                operator_id: self.operator_id.clone(),
                operator_name: None,
                evse_status_record: records,
                extensions: Extensions::new(),
            },
        };
        self.push_evse_status(&request).await
    }

    // --- authorization --------------------------------------------------------------------

    /// Asks whether a driver may start charging.
    ///
    /// # Errors
    ///
    /// See [`OicpError`]. Note that a *refusal* comes back as `Ok` with
    /// [`AuthorizationStatus::NotAuthorized`](crate::cpo::AuthorizationStatus::NotAuthorized) —
    /// the request succeeded, the answer was no.
    pub async fn authorize_start(
        &self,
        request: &AuthorizeStartRequest,
    ) -> Result<AuthorizationStartResponse> {
        self.transport.post(Operation::AuthorizeStart, &self.path_id(), request).await
    }

    /// Asks whether a session may stop.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn authorize_stop(&self, request: &AuthorizeStopRequest) -> Result<AuthorizationStopResponse> {
        self.transport.post(Operation::AuthorizeStop, &self.path_id(), request).await
    }

    // --- records --------------------------------------------------------------------------

    /// Submits a charge detail record.
    ///
    /// Consider running [`CdrCheck`](crate::eichrecht::CdrCheck) first: several of the reasons
    /// Hubject rejects a CDR need the EVSE's data record to detect, and by the time the rejection
    /// arrives the session is long over.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn send_charge_detail_record(&self, cdr: &ChargeDetailRecord) -> Result<Acknowledgement> {
        self.transport.post(Operation::ChargeDetailRecord, &self.path_id(), cdr).await
    }

    /// Sends a charging notification.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn send_charging_notification(
        &self,
        notification: &ChargingNotification,
    ) -> Result<Acknowledgement> {
        self.transport.post(Operation::ChargingNotifications, &self.path_id(), notification).await
    }

    // --- pricing --------------------------------------------------------------------------

    /// Uploads tariffs.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn push_pricing_product_data(
        &self,
        request: &PushPricingProductDataRequest,
    ) -> Result<Acknowledgement> {
        self.transport.post(Operation::PushPricingProductData, &self.path_id(), request).await
    }

    /// Uploads per-EVSE pricing.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn push_evse_pricing(&self, request: &PushEvsePricingRequest) -> Result<Acknowledgement> {
        self.transport.post(Operation::PushEvsePricing, &self.path_id(), request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_without_an_operator_id_says_why_it_cannot_be_built() {
        let err = CpoClient::builder().build().unwrap_err();
        assert!(err.to_string().contains("OperatorID"), "{err}");
    }

    #[test]
    fn a_client_builds_and_keeps_its_identifier_verbatim() {
        let client = CpoClient::builder()
            .operator_id("DE*ABC".parse().unwrap())
            .environment(HubjectEnv::Qa)
            .build()
            .unwrap();
        assert_eq!(client.operator_id().as_str(), "DE*ABC");
        assert!(client.identity_warning().is_none(), "no certificate was configured to check against");
        assert!(!client.transport().is_production());
    }
}
