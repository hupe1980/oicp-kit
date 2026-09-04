//! The endpoint table: which service, at which version, on which path, in which direction.

use core::fmt;

use crate::types::{OperatorId, ProviderId};

/// Which side of the roaming relationship a party is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    /// A Charge Point Operator.
    Cpo,
    /// An e-Mobility Provider.
    Emp,
}

impl Role {
    /// A short, stable slug.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cpo => "CPO",
            Self::Emp => "EMP",
        }
    }

    /// The other side.
    #[must_use]
    pub const fn counterpart(self) -> Self {
        match self {
            Self::Cpo => Self::Emp,
            Self::Emp => Self::Cpo,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What one operation means for one role.
///
/// # Why this is per role and not per operation
///
/// OICP proxies: `AuthorizeRemoteStart` is a request an **EMP sends** to Hubject *and* a request
/// **a CPO receives** from Hubject, at the same path. A single "direction" on the operation can
/// only be true for one of the two, and the answer a partner needs — *do I call this, or do I
/// implement it?* — depends entirely on which role they are.
///
/// Each operation is therefore asked [`Operation::involvement`] for a role, and the answer is
/// `None` for the ones that role takes no part in: a CPO has no `ProviderID` and never pulls EVSE
/// data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Involvement {
    /// You call Hubject. You are the client.
    YouCall,
    /// Hubject calls you: you implement this endpoint and register it in the Hubject portal. The
    /// half most implementations forget — see [`server`](crate::server).
    YouServe,
}

impl Involvement {
    /// A short phrase for a listing.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::YouCall => "you -> Hubject",
            Self::YouServe => "Hubject -> you",
        }
    }
}

impl fmt::Display for Involvement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The identifier that goes in an endpoint's path.
///
/// Hubject matches this against the partner's TLS client certificate, and answers a mismatch with
/// [`Code::UnauthorizedAccess`](crate::types::Code::UnauthorizedAccess). It is therefore never
/// normalised on the way out — see [`types::ids`](crate::types::EvseId).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PathId {
    /// An `{operatorID}` path segment.
    Operator(OperatorId),
    /// A `{providerID}` path segment.
    Provider(ProviderId),
}

impl PathId {
    /// The identifier as it goes into the URL — byte for byte as it arrived.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Operator(id) => id.as_str(),
            Self::Provider(id) => id.as_str(),
        }
    }
}

impl fmt::Display for PathId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<OperatorId> for PathId {
    fn from(id: OperatorId) -> Self {
        Self::Operator(id)
    }
}

impl From<ProviderId> for PathId {
    fn from(id: ProviderId) -> Self {
        Self::Provider(id)
    }
}

/// Every OICP 2.3 operation, with the path and service version it lives at.
///
/// # Why services carry their own versions
///
/// OICP versions **services**, not the protocol: a "2.3 implementation" is `evsepush/v23` *and*
/// `charging/v21` *and* `cdrmgmt/v22` *and* `reservation/v11` *and* `dynamicpricing/v10` *and*
/// `notificationmgmt/v11`. Three of those did not change between OICP 2.2 and 2.3 at all.
///
/// This table is the one place in the crate that knows which is which. `cargo run -p xtask --
/// endpoints --check` diffs it against the vendored OpenAPI documents, so a Hubject revision shows
/// up as a failing CI job rather than as a silent 404 in production.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Operation {
    /// `eRoamingAuthorizeStart_V2.1` — CPO asks whether a driver may charge.
    AuthorizeStart,
    /// `eRoamingAuthorizeStop_V2.1` — CPO asks whether a session may stop.
    AuthorizeStop,
    /// `eRoamingAuthorizeRemoteStart_V2.1` — EMP starts a session; Hubject calls the CPO.
    AuthorizeRemoteStart,
    /// `eRoamingAuthorizeRemoteStop_V2.1` — EMP stops a session; Hubject calls the CPO.
    AuthorizeRemoteStop,
    /// `eRoamingChargeDetailRecord_V2.2` — CPO submits a CDR.
    ChargeDetailRecord,
    /// `eRoamingGetChargeDetailRecords_V2.2` — EMP retrieves CDRs.
    GetChargeDetailRecords,
    /// `eRoamingPushAuthenticationData_V2.1` — offline EMP uploads its card base.
    PushAuthenticationData,
    /// `eRoamingAuthorizeRemoteReservationStart_V1.1` — EMP reserves a spot.
    AuthorizeRemoteReservationStart,
    /// `eRoamingAuthorizeRemoteReservationStop_V1.1` — EMP releases a reservation.
    AuthorizeRemoteReservationStop,
    /// `eRoamingPushEvseData_V2.3` — CPO uploads static EVSE data.
    PushEvseData,
    /// `eRoamingPullEvseData_V2.3` — EMP downloads static EVSE data.
    PullEvseData,
    /// `eRoamingPushEvseStatus_V2.1` — CPO uploads dynamic EVSE status.
    PushEvseStatus,
    /// `eRoamingPullEvseStatus_V2.1` — EMP downloads dynamic EVSE status, in all three shapes.
    PullEvseStatus,
    /// `eRoamingPushPricingProductData_V1.0` — CPO uploads tariffs.
    PushPricingProductData,
    /// `eRoamingPullPricingProductData_V1.0` — EMP downloads tariffs.
    PullPricingProductData,
    /// `eRoamingPushEVSEPricing_V1.0` — CPO uploads per-EVSE pricing.
    PushEvsePricing,
    /// `eRoamingPullEVSEPricing_V1.0` — EMP downloads per-EVSE pricing.
    PullEvsePricing,
    /// `eRoamingChargingNotifications V1.1` — CPO reports session progress.
    ChargingNotifications,
}

/// What the table knows about one operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct EndpointInfo {
    /// The service family, e.g. `charging`.
    pub service: &'static str,
    /// The service's own version, e.g. `v21`.
    pub version: &'static str,
    /// The path, with `{operatorID}` or `{providerID}` still in it.
    pub path_template: &'static str,
    /// What this operation is for a CPO, or `None` if a CPO takes no part.
    pub cpo: Option<Involvement>,
    /// What this operation is for an EMP, or `None` if an EMP takes no part.
    pub emp: Option<Involvement>,
    /// Whether the response is a page rather than an acknowledgement.
    pub paginated: bool,
}

/// `Calls` / `Serves` / `NoPart`, for the table's two role columns.
macro_rules! involvement {
    (Calls) => {
        Some(Involvement::YouCall)
    };
    (Serves) => {
        Some(Involvement::YouServe)
    };
    (NoPart) => {
        None
    };
}

macro_rules! endpoint_table {
    ($( $op:ident => $service:literal, $version:literal, $path:literal, $cpo:ident, $emp:ident, $paginated:literal );* $(;)?) => {
        impl Operation {
            /// Every operation in OICP 2.3.
            pub const ALL: &'static [Self] = &[ $( Self::$op ),* ];

            /// The table entry for this operation.
            #[must_use]
            pub const fn info(self) -> EndpointInfo {
                match self {
                    $( Self::$op => EndpointInfo {
                        service: $service,
                        version: $version,
                        path_template: $path,
                        cpo: involvement!($cpo),
                        emp: involvement!($emp),
                        paginated: $paginated,
                    }, )*
                }
            }
        }
    };
}

// The order matches the OpenAPI documents, so a diff against them reads naturally.
endpoint_table! {
    AuthorizeStart => "charging", "v21", "/charging/v21/operators/{operatorID}/authorize/start", Calls, Serves, false;
    AuthorizeStop => "charging", "v21", "/charging/v21/operators/{operatorID}/authorize/stop", Calls, Serves, false;
    AuthorizeRemoteStart => "charging", "v21", "/charging/v21/providers/{providerID}/authorize-remote/start", Serves, Calls, false;
    AuthorizeRemoteStop => "charging", "v21", "/charging/v21/providers/{providerID}/authorize-remote/stop", Serves, Calls, false;
    ChargeDetailRecord => "cdrmgmt", "v22", "/cdrmgmt/v22/operators/{operatorID}/charge-detail-record", Calls, Serves, false;
    GetChargeDetailRecords => "cdrmgmt", "v22", "/cdrmgmt/v22/providers/{providerID}/get-charge-detail-records-request", NoPart, Calls, true;
    PushAuthenticationData => "authdata", "v21", "/authdata/v21/providers/{providerID}/push-request", NoPart, Calls, false;
    AuthorizeRemoteReservationStart => "reservation", "v11", "/reservation/v11/providers/{providerID}/reservation-start-request", Serves, Calls, false;
    AuthorizeRemoteReservationStop => "reservation", "v11", "/reservation/v11/providers/{providerID}/reservation-stop-request", Serves, Calls, false;
    PushEvseData => "evsepush", "v23", "/evsepush/v23/operators/{operatorID}/data-records", Calls, NoPart, false;
    PullEvseData => "evsepull", "v23", "/evsepull/v23/providers/{providerID}/data-records", NoPart, Calls, true;
    PushEvseStatus => "evsepush", "v21", "/evsepush/v21/operators/{operatorID}/status-records", Calls, NoPart, false;
    PullEvseStatus => "evsepull", "v21", "/evsepull/v21/providers/{providerID}/status-records", NoPart, Calls, false;
    PushPricingProductData => "dynamicpricing", "v10", "/dynamicpricing/v10/operators/{operatorID}/pricing-products", Calls, NoPart, false;
    PullPricingProductData => "dynamicpricing", "v10", "/dynamicpricing/v10/providers/{providerID}/pricing-products", NoPart, Calls, false;
    PushEvsePricing => "dynamicpricing", "v10", "/dynamicpricing/v10/operators/{operatorID}/evse-pricing", Calls, NoPart, false;
    PullEvsePricing => "dynamicpricing", "v10", "/dynamicpricing/v10/providers/{providerID}/evse-pricing", NoPart, Calls, false;
    ChargingNotifications => "notificationmgmt", "v11", "/notificationmgmt/v11/charging-notifications", Calls, Serves, false;
}

impl Operation {
    /// The service family, e.g. `charging`.
    #[must_use]
    pub const fn service(self) -> &'static str {
        self.info().service
    }

    /// The service's own version, e.g. `v21`.
    #[must_use]
    pub const fn version(self) -> &'static str {
        self.info().version
    }

    /// The path with `{operatorID}` / `{providerID}` still in it.
    #[must_use]
    pub const fn path_template(self) -> &'static str {
        self.info().path_template
    }

    /// Whether the answer is a page rather than an acknowledgement.
    #[must_use]
    pub const fn is_paginated(self) -> bool {
        self.info().paginated
    }

    /// Whether `role` implements this operation rather than calling it.
    ///
    /// There is no role-free answer: `AuthorizeRemoteStart` is served by a CPO and called by an
    /// EMP, at the same path. Asking without saying which side you are on is asking the wrong
    /// question, which is why this takes a [`Role`].
    #[must_use]
    pub const fn is_served_by(self, role: Role) -> bool {
        matches!(self.involvement(role), Some(Involvement::YouServe))
    }

    /// Whether this path takes an `{operatorID}` rather than a `{providerID}`.
    #[must_use]
    pub fn takes_operator_id(self) -> bool {
        self.info().path_template.contains("{operatorID}")
    }

    /// What this operation is for `role`, or `None` when that role takes no part in it.
    ///
    /// This is the question a partner actually has — *do I call this, or do I implement it?* — and
    /// it has different answers for the two roles at the same path. See [`Involvement`].
    #[must_use]
    pub const fn involvement(self, role: Role) -> Option<Involvement> {
        match role {
            Role::Cpo => self.info().cpo,
            Role::Emp => self.info().emp,
        }
    }

    /// Every operation `role` calls or serves, in table order.
    #[must_use]
    pub fn for_role(role: Role) -> Vec<Self> {
        Self::ALL.iter().copied().filter(|op| op.involvement(role).is_some()).collect()
    }

    /// The identifier `role` puts in this path when it calls the operation.
    ///
    /// `None` when the operation has no identifier in its path, or when `role` does not call it.
    #[must_use]
    pub fn caller_path_id(self, role: Role, id: &str) -> Option<PathId> {
        if self.involvement(role) != Some(Involvement::YouCall) || !self.path_template().contains('{') {
            return None;
        }
        Some(if self.takes_operator_id() {
            PathId::Operator(OperatorId::new_unchecked(id))
        } else {
            PathId::Provider(ProviderId::new_unchecked(id))
        })
    }

    /// The path with `id` substituted in.
    ///
    /// The identifier goes in verbatim: Hubject compares it to the TLS client certificate as text.
    ///
    /// # Errors
    ///
    /// Returns [`EndpointError`] when `id` is the wrong kind for this path — a provider id in an
    /// `{operatorID}` slot, or the reverse. That mismatch is `017 Unauthorized Access` in
    /// production, so it is caught here instead.
    pub fn path(self, id: &PathId) -> Result<String, EndpointError> {
        let template = self.path_template();
        match (id, self.takes_operator_id()) {
            (PathId::Operator(_), true) => Ok(template.replace("{operatorID}", id.as_str())),
            (PathId::Provider(_), false) if template.contains("{providerID}") => {
                Ok(template.replace("{providerID}", id.as_str()))
            }
            // The notification endpoint has no id at all.
            (_, false) if !template.contains('{') => Ok(template.to_owned()),
            (PathId::Operator(_), false) => {
                Err(EndpointError::WrongIdKind { operation: self, expected: "providerID" })
            }
            (PathId::Provider(_), true) => {
                Err(EndpointError::WrongIdKind { operation: self, expected: "operatorID" })
            }
            _ => Err(EndpointError::WrongIdKind { operation: self, expected: "no id" }),
        }
    }

    /// The full URL for this operation against `base`.
    ///
    /// # Errors
    ///
    /// As [`Operation::path`].
    pub fn url(self, base: &str, id: &PathId) -> Result<String, EndpointError> {
        Ok(format!("{}{}", base.trim_end_matches('/'), self.path(id)?))
    }

    /// Looks an operation up by its concrete path, for a server router.
    #[must_use]
    pub fn from_path(path: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|op| path_matches(op.path_template(), path))
    }
}

/// Whether a concrete path matches a template with `{…}` placeholders.
fn path_matches(template: &str, path: &str) -> bool {
    let mut t = template.split('/');
    let mut p = path.trim_end_matches('/').split('/');
    loop {
        match (t.next(), p.next()) {
            (None, None) => return true,
            (Some(ts), Some(ps)) if ts.starts_with('{') && ts.ends_with('}') => {
                if ps.is_empty() {
                    return false;
                }
            }
            (Some(ts), Some(ps)) if ts == ps => {}
            _ => return false,
        }
    }
}

/// Why a URL could not be built.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EndpointError {
    /// The identifier is the wrong kind for this path.
    #[error("{operation:?} takes a {expected} in its path, but the other kind of identifier was given")]
    WrongIdKind {
        /// The operation.
        operation: Operation,
        /// What the path wanted.
        expected: &'static str,
    },
}

/// Which Hubject environment to talk to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubjectEnv {
    /// The production brokering system, `https://service.hubject.com/api/oicp`.
    Prod,
    /// The QA system, `https://service-qa.hubject.com/api/oicp`. Where integration starts.
    Qa,
    /// A base URL of your own — a [`MockHubject`](crate::testkit::MockHubject), or a proxy.
    Custom(String),
}

impl HubjectEnv {
    /// The base URL, without a trailing slash.
    #[must_use]
    pub fn base_url(&self) -> &str {
        match self {
            Self::Prod => "https://service.hubject.com/api/oicp",
            Self::Qa => "https://service-qa.hubject.com/api/oicp",
            Self::Custom(url) => url.trim_end_matches('/'),
        }
    }

    /// Whether this is the production system.
    ///
    /// Worth branching on before anything destructive: a `fullLoad` against `Prod` is not a
    /// rehearsal.
    #[must_use]
    pub const fn is_production(&self) -> bool {
        matches!(self, Self::Prod)
    }
}

impl fmt::Display for HubjectEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.base_url())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_covers_every_operation_exactly_once() {
        assert_eq!(Operation::ALL.len(), 18);
        let mut paths: Vec<_> = Operation::ALL.iter().map(|op| (op.path_template(), *op)).collect();
        paths.sort_unstable();
        let mut unique = paths.clone();
        unique.dedup_by_key(|(p, _)| *p);
        assert_eq!(paths.len(), unique.len(), "two operations share a path template");
    }

    #[test]
    fn paths_carry_the_identifier_exactly_as_given() {
        let op = Operation::PushEvseData;
        // Hubject string-matches this against the certificate: `DE*ABC` must not become `DEABC`.
        let id = PathId::Operator("DE*ABC".parse().unwrap());
        assert_eq!(op.path(&id).unwrap(), "/evsepush/v23/operators/DE*ABC/data-records");

        let packed = PathId::Operator("DEABC".parse().unwrap());
        assert_eq!(op.path(&packed).unwrap(), "/evsepush/v23/operators/DEABC/data-records");
    }

    #[test]
    fn the_wrong_kind_of_identifier_is_caught_locally() {
        let provider = PathId::Provider("DE-DCB".parse().unwrap());
        // PushEvseData wants an operatorID; this would be 017 Unauthorized Access in production.
        let err = Operation::PushEvseData.path(&provider).unwrap_err();
        assert!(matches!(err, EndpointError::WrongIdKind { expected: "operatorID", .. }));

        let operator = PathId::Operator("DE*ABC".parse().unwrap());
        assert!(Operation::PullEvseData.path(&operator).is_err());
    }

    #[test]
    fn the_notification_endpoint_takes_no_identifier() {
        let op = Operation::ChargingNotifications;
        let id = PathId::Operator("DE*ABC".parse().unwrap());
        assert_eq!(op.path(&id).unwrap(), "/notificationmgmt/v11/charging-notifications");
    }

    #[test]
    fn routing_recovers_the_operation_from_a_concrete_path() {
        assert_eq!(
            Operation::from_path("/evsepush/v23/operators/DE*ABC/data-records"),
            Some(Operation::PushEvseData)
        );
        assert_eq!(
            Operation::from_path("/charging/v21/providers/DE-DCB/authorize-remote/start"),
            Some(Operation::AuthorizeRemoteStart)
        );
        assert_eq!(Operation::from_path("/nope"), None);
        // The two authorize paths differ only in their last segment.
        assert_eq!(
            Operation::from_path("/charging/v21/operators/DE*ABC/authorize/stop"),
            Some(Operation::AuthorizeStop)
        );
    }

    #[test]
    fn what_a_partner_serves_depends_on_which_partner_they_are() {
        // The same four paths, read from the two sides. `AuthorizeRemoteStart` is the pair that
        // makes the point: an EMP calls it and a CPO implements it, at one path — so an operation
        // cannot carry a single direction and be right about both.
        assert_eq!(
            Operation::ALL.iter().copied().filter(|op| op.is_served_by(Role::Cpo)).collect::<Vec<_>>(),
            vec![
                Operation::AuthorizeRemoteStart,
                Operation::AuthorizeRemoteStop,
                Operation::AuthorizeRemoteReservationStart,
                Operation::AuthorizeRemoteReservationStop,
            ]
        );
        assert_eq!(
            Operation::ALL.iter().copied().filter(|op| op.is_served_by(Role::Emp)).collect::<Vec<_>>(),
            vec![
                Operation::AuthorizeStart,
                Operation::AuthorizeStop,
                Operation::ChargeDetailRecord,
                Operation::ChargingNotifications,
            ]
        );
    }

    #[test]
    fn the_role_that_calls_an_operation_is_the_role_whose_identifier_the_path_carries() {
        // Not a coincidence worth relying on silently: a CPO only ever calls `{operatorID}` paths
        // and an EMP only ever calls `{providerID}` ones, which is what makes `caller_path_id`
        // able to pick the kind without being told.
        for operation in Operation::ALL.iter().copied() {
            if !operation.path_template().contains('{') {
                continue;
            }
            let caller = if operation.involvement(Role::Cpo) == Some(Involvement::YouCall) {
                Role::Cpo
            } else {
                Role::Emp
            };
            assert_eq!(
                operation.involvement(caller),
                Some(Involvement::YouCall),
                "{operation:?} is called by neither role"
            );
            assert_eq!(
                operation.takes_operator_id(),
                caller == Role::Cpo,
                "{operation:?} is called by {caller} but its path carries the other identifier"
            );
        }
    }

    #[test]
    fn every_operation_involves_at_least_one_role() {
        for operation in Operation::ALL.iter().copied() {
            assert!(
                operation.involvement(Role::Cpo).is_some() || operation.involvement(Role::Emp).is_some(),
                "{operation:?} is in the table and belongs to nobody"
            );
        }
        assert_eq!(Operation::for_role(Role::Cpo).len(), 12, "the CPO document's twelve endpoints");
        assert_eq!(Operation::for_role(Role::Emp).len(), 14, "the EMP document's fourteen");
    }

    #[test]
    fn only_the_two_big_pulls_are_paginated() {
        let paginated: Vec<_> = Operation::ALL.iter().copied().filter(|op| op.is_paginated()).collect();
        assert_eq!(paginated, vec![Operation::GetChargeDetailRecords, Operation::PullEvseData]);
    }

    #[test]
    fn the_environments_have_the_urls_the_spec_gives() {
        assert_eq!(HubjectEnv::Prod.base_url(), "https://service.hubject.com/api/oicp");
        assert_eq!(HubjectEnv::Qa.base_url(), "https://service-qa.hubject.com/api/oicp");
        assert!(HubjectEnv::Prod.is_production());
        assert!(!HubjectEnv::Qa.is_production());
        assert_eq!(HubjectEnv::Custom("http://localhost:8080/".into()).base_url(), "http://localhost:8080");
    }

    #[test]
    fn urls_join_base_and_path_without_doubling_the_slash() {
        let url = Operation::PullEvseData
            .url(HubjectEnv::Qa.base_url(), &PathId::Provider("DE-DCB".parse().unwrap()))
            .unwrap();
        assert_eq!(url, "https://service-qa.hubject.com/api/oicp/evsepull/v23/providers/DE-DCB/data-records");
    }
}
