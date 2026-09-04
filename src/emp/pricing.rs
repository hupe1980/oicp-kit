//! Pulling tariffs: what the CPOs an EMP has contracts with are charging.

use serde::{Deserialize, Serialize};

use crate::cpo::{OperatorEvsePricing, PricingProductData};
use crate::types::{
    DateTime, Extensions, OperatorId, ProviderId, StatusCode, Validate, Validator, ViolationCode,
    strict_builder, validate_fields,
};

/// Asks Hubject for the tariffs the named operators have published to this EMP.
///
/// Unlike [`PullEvseDataRequest`](super::PullEvseDataRequest), `LastCall` here is *not* exclusive
/// with the operator filter — the spec makes `OperatorIDs` mandatory, so a delta is always scoped
/// to operators the EMP named. There is nothing to omit, so there is nothing to go stale.
///
/// Spec: `eRoamingPullPricingProductData_V1.0`,
/// `POST /dynamicpricing/v10/providers/{providerID}/pricing-products`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PullPricingProductDataRequest {
    /// Ask for the changes since this instant, rather than everything.
    #[serde(rename = "LastCall", default, skip_serializing_if = "Option::is_none")]
    pub last_call: Option<DateTime>,
    /// Whose tariffs. Mandatory.
    #[serde(rename = "OperatorIDs")]
    pub operator_ids: Vec<OperatorId>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for PullPricingProductDataRequest {
    fn validate_in(&self, v: &mut Validator) {
        if self.operator_ids.is_empty() {
            v.report_at(
                "OperatorIDs",
                ViolationCode::EmptyRequiredList,
                "OperatorIDs is mandatory: name at least one operator whose tariffs you want",
            );
        }
        validate_fields!(self, v, last_call as "LastCall", operator_ids as "OperatorIDs");
    }
}

/// Asks Hubject which tariffs apply at which charging points.
///
/// Spec: `eRoamingPullEVSEPricing_V1.0`,
/// `POST /dynamicpricing/v10/providers/{providerID}/evse-pricing`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PullEvsePricingRequest {
    /// The EMP asking.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// Ask for the changes since this instant.
    #[serde(rename = "LastCall", default, skip_serializing_if = "Option::is_none")]
    pub last_call: Option<DateTime>,
    /// Whose pricing. Mandatory.
    #[serde(rename = "OperatorIDs")]
    pub operator_ids: Vec<OperatorId>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for PullEvsePricingRequest {
    fn validate_in(&self, v: &mut Validator) {
        if self.operator_ids.is_empty() {
            v.report_at(
                "OperatorIDs",
                ViolationCode::EmptyRequiredList,
                "OperatorIDs is mandatory: name at least one operator whose pricing you want",
            );
        }
        validate_fields!(self, v, provider_id as "ProviderID", last_call as "LastCall", operator_ids as "OperatorIDs");
    }
}

/// The answer to [`PullPricingProductDataRequest`].
///
/// Spec: `eRoamingPricingProductData`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PricingProductDataResponse {
    /// The tariffs, one block per operator.
    #[serde(rename = "PricingProductData")]
    pub pricing_product_data: Vec<PricingProductData>,
    /// Whether the query itself succeeded.
    #[serde(rename = "StatusCode", default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<StatusCode>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Validate for PricingProductDataResponse {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, pricing_product_data as "PricingProductData", status_code as "StatusCode");
    }
}

/// The answer to [`PullEvsePricingRequest`].
///
/// Spec: `eRoamingEVSEPricing`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvsePricingResponse {
    /// The pricing, one block per operator.
    #[serde(rename = "EVSEPricing")]
    pub evse_pricing: Vec<OperatorEvsePricing>,
    /// Whether the query itself succeeded.
    #[serde(rename = "StatusCode", default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<StatusCode>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Validate for EvsePricingResponse {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, evse_pricing as "EVSEPricing", status_code as "StatusCode");
    }
}

strict_builder!(
    PullPricingProductDataRequest,
    PullPricingProductDataRequestBuilder,
    pull_pricing_product_data_request_builder
);
strict_builder!(PullEvsePricingRequest, PullEvsePricingRequestBuilder, pull_evse_pricing_request_builder);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_ids_are_mandatory_on_a_pricing_pull() {
        let request = PullPricingProductDataRequest {
            last_call: None,
            operator_ids: vec![],
            extensions: Extensions::new(),
        };
        assert_eq!(request.validate().unwrap_err().as_slice()[0].pointer, "/OperatorIDs");
    }

    #[test]
    fn a_pricing_delta_may_name_operators_unlike_an_evse_delta() {
        let request = PullPricingProductDataRequest {
            last_call: Some("2020-09-23T14:33:42.246Z".parse().unwrap()),
            operator_ids: vec!["DE*ABC".parse().unwrap()],
            extensions: Extensions::new(),
        };
        assert!(request.validate().is_ok());
    }
}
