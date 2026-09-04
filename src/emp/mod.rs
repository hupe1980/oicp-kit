//! The EMP half of OICP 2.3: what an e-Mobility Provider asks for, and what Hubject sends it.
//!
//! # The two directions
//!
//! | Direction | Messages | Endpoint |
//! |---|---|---|
//! | EMP → Hubject | [`PullEvseDataRequest`], [`PullEvseStatusRequest`] and friends | `/evsepull/v23`, `/evsepull/v21` |
//! | EMP → Hubject | [`AuthorizeRemoteStartRequest`](crate::cpo::AuthorizeRemoteStartRequest) | `/charging/v21` |
//! | EMP → Hubject | [`GetChargeDetailRecordsRequest`] | `/cdrmgmt/v22` |
//! | EMP → Hubject | [`PushAuthenticationDataRequest`] | `/authdata/v21` |
//! | EMP → Hubject | [`PullPricingProductDataRequest`], [`PullEvsePricingRequest`] | `/dynamicpricing/v10` |
//! | **Hubject → EMP** | [`AuthorizeStartRequest`](crate::cpo::AuthorizeStartRequest), forwarded from a CPO | the EMP's own endpoint |
//! | **Hubject → EMP** | [`ChargeDetailRecord`](crate::cpo::ChargeDetailRecord) | the EMP's own endpoint |
//! | **Hubject → EMP** | [`ChargingNotification`](crate::cpo::ChargingNotification) | the EMP's own endpoint |
//!
//! The messages an EMP *receives* are the same objects a CPO *sends*, so they live in
//! [`cpo`](crate::cpo) and are re-exported here. There is one CDR type in this crate, not two:
//! the record a CPO submits and the record an EMP is billed from are the same document, and
//! modelling them separately is how the two sides drift apart.
//!
//! # Pagination
//!
//! An EMP pulls everyone's data, so the big pulls are paginated. See [`Page`], and prefer the
//! streaming crawl in [`client`](crate::client) over assembling pages by hand.

mod cdr;
mod page;
mod pricing;
mod pull;

pub use cdr::{
    AuthenticationDataRecord, GetChargeDetailRecordsRequest, ProviderAuthenticationData,
    PushAuthenticationDataRequest,
};
pub use page::{Page, Pageable, Sort};
pub use pricing::{
    EvsePricingResponse, PricingProductDataResponse, PullEvsePricingRequest, PullPricingProductDataRequest,
};
pub use pull::{
    EvseStatusByIdResponse, EvseStatusRecords, EvseStatusResponse, EvseStatuses,
    MAX_EVSE_IDS_PER_STATUS_REQUEST, OperatorEvseStatusRecords, PullEvseDataRecord, PullEvseDataRequest,
    PullEvseStatusByIdRequest, PullEvseStatusByOperatorIdRequest, PullEvseStatusRequest, SearchCenter,
};

/// One page of `PullEvseData`.
pub type EvseDataResponse = Page<PullEvseDataRecord>;

/// One page of `GetChargeDetailRecords`.
pub type ChargeDetailRecordsResponse = Page<crate::cpo::ChargeDetailRecord>;
