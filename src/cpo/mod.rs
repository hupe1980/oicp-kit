//! The CPO half of OICP 2.3: what a Charge Point Operator sends, and what Hubject sends it back.
//!
//! # The two directions
//!
//! | Direction | Messages | Endpoint |
//! |---|---|---|
//! | CPO → Hubject | [`PushEvseDataRequest`], [`PushEvseStatusRequest`] | `/evsepush/v23`, `/evsepush/v21` |
//! | CPO → Hubject | [`AuthorizeStartRequest`], [`AuthorizeStopRequest`] | `/charging/v21` |
//! | CPO → Hubject | [`ChargeDetailRecord`] | `/cdrmgmt/v22` |
//! | CPO → Hubject | [`ChargingNotification`] | `/notificationmgmt/v11` |
//! | CPO → Hubject | [`PushPricingProductDataRequest`], [`PushEvsePricingRequest`] | `/dynamicpricing/v10` |
//! | **Hubject → CPO** | [`AuthorizeRemoteStartRequest`], [`AuthorizeRemoteStopRequest`] | the CPO's own endpoint |
//! | **Hubject → CPO** | [`AuthorizeRemoteReservationStartRequest`], [`AuthorizeRemoteReservationStopRequest`] | the CPO's own endpoint |
//!
//! The second half is not optional. A CPO that only implements the first cannot be started from a
//! driver's phone app, which is most of what roaming is for. [`CpoService`](crate::server::CpoService)
//! is the trait that makes sure you have implemented all of it.
//!
//! Everything here uses the shared [`types`](crate::types); the EMP's view of the same protocol is
//! in [`emp`](crate::emp).

mod authorization;
mod cdr;
mod evse;
mod notification;
mod pricing;
mod remote;

pub use authorization::{
    AuthorizationStartResponse, AuthorizationStatus, AuthorizationStopResponse, AuthorizeStartRequest,
    AuthorizeStopRequest,
};
pub use cdr::{
    CalibrationLawVerificationInfo, ChargeDetailRecord, MeterValuesInBetween, MeteringStatus,
    SignedMeteringValue,
};
pub use evse::{
    DeltaType, EvseDataRecord, EvseStatus, EvseStatusRecord, OperatorEvseData, OperatorEvseStatus,
    PushEvseDataRequest, PushEvseStatusRequest,
};
pub use notification::{
    ChargingNotification, ChargingNotificationEnd, ChargingNotificationError, ChargingNotificationProgress,
    ChargingNotificationStart, ChargingNotificationType, ErrorType,
};
pub use pricing::{
    AdditionalReference, AdditionalReferenceType, EvsePricing, OperatorEvsePricing, PricingProductData,
    PricingProductDataRecord, ProductAvailabilityTime, PushEvsePricingRequest, PushPricingProductDataRequest,
};
pub use remote::{
    AuthorizeRemoteReservationStartRequest, AuthorizeRemoteReservationStopRequest,
    AuthorizeRemoteStartRequest, AuthorizeRemoteStopRequest,
};

pub(crate) use evse::validate_evse_common;
