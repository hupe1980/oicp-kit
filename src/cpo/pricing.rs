//! Dynamic pricing: what a CPO charges, and which EMP it charges that to.

use serde::{Deserialize, Serialize};

use crate::oicp_open_enum;
use crate::types::{
    ActionType, DaySelection, EvseId, Extensions, Number, OperatorId, Period, ProviderId, ProviderIdOrAll,
    ReferenceUnit, Text, Validate, Validator, ViolationCode, strict_builder, validate_fields,
};

oicp_open_enum! {
    /// A fee charged in addition to the base price.
    pub enum AdditionalReferenceType {
        /// A fixed fee for starting the session, on top of the base price.
        StartFee = "START FEE",
        /// A single price regardless of duration or energy. The base price should then be zero.
        FixedFee = "FIXED FEE",
        /// A fee for occupying the bay, configured on the Hubject portal.
        ParkingFee = "PARKING FEE",
        /// A floor: the session cannot cost less than this.
        MinimumFee = "MINIMUM FEE",
        /// A ceiling: the session cannot cost more than this.
        MaximumFee = "MAXIMUM FEE",
    }
}

/// When a pricing product applies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct ProductAvailabilityTime {
    /// The windows within the day.
    #[serde(rename = "Periods")]
    pub periods: Vec<Period>,
    /// Which days.
    #[serde(rename = "on")]
    pub on: DaySelection,
}

impl Validate for ProductAvailabilityTime {
    fn validate_in(&self, v: &mut Validator) {
        if self.periods.is_empty() {
            v.report_at(
                "Periods",
                ViolationCode::EmptyRequiredList,
                "an availability time needs at least one period",
            );
        }
        validate_fields!(self, v, periods as "Periods", on as "on");
    }
}

/// An extra fee, and what it is charged per.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct AdditionalReference {
    /// Which kind of fee.
    #[serde(rename = "AdditionalReference")]
    pub additional_reference: AdditionalReferenceType,
    /// What it is charged per.
    #[serde(rename = "AdditionalReferenceUnit")]
    pub additional_reference_unit: ReferenceUnit,
    /// How much, in the product's currency.
    #[serde(rename = "PricePerAdditionalReferenceUnit")]
    pub price_per_additional_reference_unit: Number,
}

impl Validate for AdditionalReference {
    fn validate_in(&self, v: &mut Validator) {
        if self.price_per_additional_reference_unit.is_negative() {
            v.report_at(
                "PricePerAdditionalReferenceUnit",
                ViolationCode::OutOfRange,
                "a fee cannot be negative",
            );
        }
        validate_fields!(
            self,
            v,
            additional_reference as "AdditionalReference",
            additional_reference_unit as "AdditionalReferenceUnit",
            price_per_additional_reference_unit as "PricePerAdditionalReferenceUnit",
        );
    }
}

/// One tariff a CPO offers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PricingProductDataRecord {
    /// The product's identifier, e.g. `AC1` or a CPO's own name for it.
    #[serde(rename = "ProductID")]
    #[builder(into)]
    pub product_id: Text<50>,
    /// What the base price is charged per.
    #[serde(rename = "ReferenceUnit")]
    pub reference_unit: ReferenceUnit,
    /// The ISO 4217 currency.
    #[serde(rename = "ProductPriceCurrency")]
    #[builder(into)]
    pub product_price_currency: Text<3>,
    /// The base price, per reference unit.
    #[serde(rename = "PricePerReferenceUnit")]
    pub price_per_reference_unit: Number,
    /// The maximum power this product covers, in kW.
    #[serde(rename = "MaximumProductChargingPower")]
    pub maximum_product_charging_power: Number,
    /// Whether the product applies around the clock.
    #[serde(rename = "IsValid24hours")]
    pub is_valid_24hours: bool,
    /// When it applies, if not around the clock.
    #[serde(rename = "ProductAvailabilityTimes")]
    pub product_availability_times: Vec<ProductAvailabilityTime>,
    /// Fees charged on top of the base price.
    #[serde(rename = "AdditionalReferences", default, skip_serializing_if = "Option::is_none")]
    pub additional_references: Option<Vec<AdditionalReference>>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for PricingProductDataRecord {
    fn validate_in(&self, v: &mut Validator) {
        if self.price_per_reference_unit.is_negative() {
            v.report_at("PricePerReferenceUnit", ViolationCode::OutOfRange, "a price cannot be negative");
        }
        if self.product_price_currency.len() != 3 {
            v.report_at(
                "ProductPriceCurrency",
                ViolationCode::PatternMismatch,
                format!(
                    "{:?} is not a three-letter ISO 4217 currency code",
                    self.product_price_currency.as_str()
                ),
            );
        }
        match (self.is_valid_24hours, self.product_availability_times.is_empty()) {
            (false, true) => v.report_at(
                "ProductAvailabilityTimes",
                ViolationCode::MissingConditional,
                "IsValid24hours is false, so the times the product does apply must be given",
            ),
            (true, false) => v.report_at(
                "ProductAvailabilityTimes",
                ViolationCode::Inconsistent,
                "IsValid24hours is true, so ProductAvailabilityTimes contradicts it",
            ),
            _ => {}
        }
        // "When used, the value set in PricePerReferenceUnit […] SHOULD be set to zero."
        if let Some(references) = &self.additional_references {
            let has_fixed =
                references.iter().any(|r| r.additional_reference == AdditionalReferenceType::FixedFee);
            if has_fixed && !self.price_per_reference_unit.is_zero() {
                v.report_at(
                    "PricePerReferenceUnit",
                    ViolationCode::Inconsistent,
                    format!(
                        "a FIXED FEE product should price the reference unit at zero, but this one \
                         charges {} on top of the fixed fee",
                        self.price_per_reference_unit
                    ),
                );
            }
            check_fee_bounds(references, v);
        }
        validate_fields!(
            self,
            v,
            product_id as "ProductID",
            reference_unit as "ReferenceUnit",
            product_price_currency as "ProductPriceCurrency",
            price_per_reference_unit as "PricePerReferenceUnit",
            maximum_product_charging_power as "MaximumProductChargingPower",
            product_availability_times as "ProductAvailabilityTimes",
            additional_references as "AdditionalReferences",
        );
    }
}

/// Checks the two fees that bound a session's price against each other.
///
/// > *MINIMUM FEE: […] the eventual price to be paid cannot be less than this minimum fee.*
/// > *MAXIMUM FEE: […] the eventual price to be paid cannot be more than this maximum fee.*
///
/// Stated separately, each is a plain number and Hubject accepts either. Stated **together** they
/// are a range, and a floor above its ceiling is a product no session can be priced under — which
/// is discovered when the first invoice is disputed, not when the tariff is published.
///
/// Only fees charged per the *same* reference unit are compared: a minimum per session and a
/// maximum per kWh bound different quantities and say nothing about each other.
fn check_fee_bounds(references: &[AdditionalReference], v: &mut Validator) {
    // Two fees of the same kind and unit is its own problem: which one applies is undefined, and
    // the two sides of a settlement can read it differently.
    for kind in [AdditionalReferenceType::MinimumFee, AdditionalReferenceType::MaximumFee] {
        let mut seen: Vec<&ReferenceUnit> = vec![];
        for reference in references.iter().filter(|r| r.additional_reference == kind) {
            if seen.contains(&&reference.additional_reference_unit) {
                v.report_at(
                    "AdditionalReferences",
                    ViolationCode::Inconsistent,
                    format!(
                        "{} appears more than once per {}; which one applies is undefined",
                        kind.as_str(),
                        reference.additional_reference_unit.as_str()
                    ),
                );
            } else {
                seen.push(&reference.additional_reference_unit);
            }
        }
    }

    for floor in references.iter().filter(|r| r.additional_reference == AdditionalReferenceType::MinimumFee) {
        for ceiling in references
            .iter()
            .filter(|r| r.additional_reference == AdditionalReferenceType::MaximumFee)
            .filter(|r| r.additional_reference_unit == floor.additional_reference_unit)
        {
            if floor.price_per_additional_reference_unit > ceiling.price_per_additional_reference_unit {
                v.report_at(
                    "AdditionalReferences",
                    ViolationCode::Inconsistent,
                    format!(
                        "the MINIMUM FEE of {} is above the MAXIMUM FEE of {} per {}; no session can \
                         be priced within both",
                        floor.price_per_additional_reference_unit,
                        ceiling.price_per_additional_reference_unit,
                        floor.additional_reference_unit.as_str()
                    ),
                );
            }
        }
    }
}

/// A CPO's tariffs, offered to one EMP or to all of them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PricingProductData {
    /// The operator offering them.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// The operator's name.
    #[serde(rename = "OperatorName", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub operator_name: Option<Text<100>>,
    /// Which EMP these prices are for — or `*` for every subscribed EMP.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderIdOrAll,
    /// The price for sessions at EVSEs no product covers.
    #[serde(rename = "PricingDefaultPrice")]
    pub pricing_default_price: Number,
    /// The currency of the default price.
    #[serde(rename = "PricingDefaultPriceCurrency")]
    #[builder(into)]
    pub pricing_default_price_currency: Text<3>,
    /// What the default price is charged per.
    #[serde(rename = "PricingDefaultReferenceUnit")]
    pub pricing_default_reference_unit: ReferenceUnit,
    /// The tariffs.
    #[serde(rename = "PricingProductDataRecords")]
    pub pricing_product_data_records: Vec<PricingProductDataRecord>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for PricingProductData {
    fn validate_in(&self, v: &mut Validator) {
        if self.pricing_default_price.is_negative() {
            v.report_at("PricingDefaultPrice", ViolationCode::OutOfRange, "a price cannot be negative");
        }
        // Two products with the same id make the EMP's choice ambiguous.
        let mut seen = std::collections::HashSet::new();
        for (i, record) in self.pricing_product_data_records.iter().enumerate() {
            if !seen.insert(record.product_id.as_str()) {
                v.enter("PricingProductDataRecords");
                v.enter(&i.to_string());
                v.report_at(
                    "ProductID",
                    ViolationCode::Inconsistent,
                    format!("{:?} appears more than once in this push", record.product_id.as_str()),
                );
                v.leave();
                v.leave();
            }
        }
        validate_fields!(
            self,
            v,
            operator_id as "OperatorID",
            operator_name as "OperatorName",
            provider_id as "ProviderID",
            pricing_default_price as "PricingDefaultPrice",
            pricing_default_reference_unit as "PricingDefaultReferenceUnit",
            pricing_product_data_records as "PricingProductDataRecords",
        );
    }
}

/// Uploads tariffs to Hubject.
///
/// Spec: `eRoamingPushPricingProductData_V1.0`,
/// `POST /dynamicpricing/v10/operators/{operatorID}/pricing-products`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PushPricingProductDataRequest {
    /// What Hubject should do with the payload.
    #[serde(rename = "ActionType")]
    pub action_type: ActionType,
    /// The tariffs.
    #[serde(rename = "PricingProductData")]
    pub pricing_product_data: PricingProductData,
}

impl Validate for PushPricingProductDataRequest {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, action_type as "ActionType", pricing_product_data as "PricingProductData");
    }
}

/// Which tariffs apply at one charging spot, for one EMP.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct EvsePricing {
    /// The spot.
    #[serde(rename = "EvseID")]
    pub evse_id: EvseId,
    /// The EMP.
    #[serde(rename = "ProviderID")]
    pub provider_id: ProviderId,
    /// The products that apply there.
    #[serde(rename = "EvseIDProductList")]
    pub evse_id_product_list: Vec<Text<50>>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for EvsePricing {
    fn validate_in(&self, v: &mut Validator) {
        if self.evse_id_product_list.is_empty() {
            v.report_at(
                "EvseIDProductList",
                ViolationCode::EmptyRequiredList,
                "an EVSE pricing entry names at least one product",
            );
        }
        validate_fields!(
            self,
            v,
            evse_id as "EvseID",
            provider_id as "ProviderID",
            evse_id_product_list as "EvseIDProductList",
        );
    }
}

/// One operator's per-EVSE pricing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct OperatorEvsePricing {
    /// The operator.
    #[serde(rename = "OperatorID")]
    pub operator_id: OperatorId,
    /// The operator's name.
    #[serde(rename = "OperatorName", default, skip_serializing_if = "Option::is_none")]
    #[builder(into)]
    pub operator_name: Option<Text<100>>,
    /// The per-EVSE entries.
    #[serde(rename = "EVSEPricing")]
    pub evse_pricing: Vec<EvsePricing>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Validate for OperatorEvsePricing {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(
            self,
            v,
            operator_id as "OperatorID",
            operator_name as "OperatorName",
            evse_pricing as "EVSEPricing",
        );
    }
}

/// Uploads per-EVSE pricing to Hubject.
///
/// Spec: `eRoamingPushEVSEPricing_V1.0`,
/// `POST /dynamicpricing/v10/operators/{operatorID}/evse-pricing`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[builder(finish_fn = build_unchecked)]
pub struct PushEvsePricingRequest {
    /// What Hubject should do with the payload.
    #[serde(rename = "ActionType")]
    pub action_type: ActionType,
    /// The pricing.
    #[serde(rename = "EVSEPricing")]
    pub evse_pricing: Vec<OperatorEvsePricing>,
}

impl Validate for PushEvsePricingRequest {
    fn validate_in(&self, v: &mut Validator) {
        validate_fields!(self, v, action_type as "ActionType", evse_pricing as "EVSEPricing");
    }
}

strict_builder!(ProductAvailabilityTime, ProductAvailabilityTimeBuilder, product_availability_time_builder);
strict_builder!(AdditionalReference, AdditionalReferenceBuilder, additional_reference_builder);
strict_builder!(
    PricingProductDataRecord,
    PricingProductDataRecordBuilder,
    pricing_product_data_record_builder
);
strict_builder!(PricingProductData, PricingProductDataBuilder, pricing_product_data_builder);
strict_builder!(
    PushPricingProductDataRequest,
    PushPricingProductDataRequestBuilder,
    push_pricing_product_data_request_builder
);
strict_builder!(EvsePricing, EvsePricingBuilder, evse_pricing_builder);
strict_builder!(OperatorEvsePricing, OperatorEvsePricingBuilder, operator_evse_pricing_builder);
strict_builder!(PushEvsePricingRequest, PushEvsePricingRequestBuilder, push_evse_pricing_request_builder);

#[cfg(test)]
mod tests {
    use super::*;

    fn product() -> PricingProductDataRecord {
        PricingProductDataRecord {
            product_id: Text::new("AC1").unwrap(),
            reference_unit: ReferenceUnit::KilowattHour,
            product_price_currency: Text::new("EUR").unwrap(),
            price_per_reference_unit: "0.35".parse().unwrap(),
            maximum_product_charging_power: "22".parse().unwrap(),
            is_valid_24hours: true,
            product_availability_times: vec![],
            additional_references: None,
            extensions: Extensions::new(),
        }
    }

    #[test]
    fn prices_are_exact_decimals() {
        let p = product();
        assert_eq!(serde_json::to_string(&p.price_per_reference_unit).unwrap(), "0.35");
        assert!(p.validate().is_ok());
    }

    #[test]
    fn a_product_that_is_not_always_valid_must_say_when_it_is() {
        let p = PricingProductDataRecord { is_valid_24hours: false, ..product() };
        let err = p.validate().unwrap_err();
        assert_eq!(err.as_slice()[0].pointer, "/ProductAvailabilityTimes");
        assert_eq!(err.as_slice()[0].code, ViolationCode::MissingConditional);
    }

    #[test]
    fn a_fixed_fee_product_should_not_also_charge_per_unit() {
        let p = PricingProductDataRecord {
            additional_references: Some(vec![AdditionalReference {
                additional_reference: AdditionalReferenceType::FixedFee,
                additional_reference_unit: ReferenceUnit::Hour,
                price_per_additional_reference_unit: "5".parse().unwrap(),
            }]),
            ..product()
        };
        let err = p.validate().unwrap_err();
        assert_eq!(err.as_slice()[0].pointer, "/PricePerReferenceUnit");

        // Zeroed, as the spec asks.
        let fixed = PricingProductDataRecord { price_per_reference_unit: Number::ZERO, ..p };
        assert!(fixed.validate().is_ok());
    }

    #[test]
    fn the_offer_to_all_asterisk_is_a_distinct_value() {
        let data = PricingProductData {
            operator_id: "DE*ABC".parse().unwrap(),
            operator_name: None,
            provider_id: ProviderIdOrAll::All,
            pricing_default_price: "0.40".parse().unwrap(),
            pricing_default_price_currency: Text::new("EUR").unwrap(),
            pricing_default_reference_unit: ReferenceUnit::KilowattHour,
            pricing_product_data_records: vec![product()],
            extensions: Extensions::new(),
        };
        assert!(data.provider_id.is_all());
        assert!(serde_json::to_string(&data).unwrap().contains(r#""ProviderID":"*""#));
        assert!(data.validate().is_ok());
    }

    #[test]
    fn a_floor_above_its_ceiling_is_a_product_no_session_can_be_priced_under() {
        let fee = |kind, amount: &str| AdditionalReference {
            additional_reference: kind,
            additional_reference_unit: ReferenceUnit::Hour,
            price_per_additional_reference_unit: amount.parse().unwrap(),
        };

        let mut record = product();
        record.additional_references = Some(vec![
            fee(AdditionalReferenceType::MinimumFee, "5.00"),
            fee(AdditionalReferenceType::MaximumFee, "2.00"),
        ]);
        let err = record.validate().unwrap_err();
        assert!(
            err.iter().any(|x| x.pointer == "/AdditionalReferences"
                && x.message.contains("no session can be priced within both")),
            "{err}"
        );

        // The right way round is fine, and so is either fee on its own.
        record.additional_references = Some(vec![
            fee(AdditionalReferenceType::MinimumFee, "2.00"),
            fee(AdditionalReferenceType::MaximumFee, "5.00"),
        ]);
        assert!(record.validate().is_ok(), "{:?}", record.validate());
        record.additional_references = Some(vec![fee(AdditionalReferenceType::MinimumFee, "5.00")]);
        assert!(record.validate().is_ok());
    }

    #[test]
    fn fees_bounding_different_quantities_say_nothing_about_each_other() {
        // A minimum per hour of occupancy and a maximum per kWh bound different things; comparing
        // their numbers would refuse a perfectly ordinary tariff.
        let mut record = product();
        record.additional_references = Some(vec![
            AdditionalReference {
                additional_reference: AdditionalReferenceType::MinimumFee,
                additional_reference_unit: ReferenceUnit::Hour,
                price_per_additional_reference_unit: "5.00".parse().unwrap(),
            },
            AdditionalReference {
                additional_reference: AdditionalReferenceType::MaximumFee,
                additional_reference_unit: ReferenceUnit::KilowattHour,
                price_per_additional_reference_unit: "2.00".parse().unwrap(),
            },
        ]);
        assert!(record.validate().is_ok(), "{:?}", record.validate());
    }

    #[test]
    fn the_same_fee_twice_per_unit_is_ambiguous() {
        let mut record = product();
        record.additional_references = Some(vec![
            AdditionalReference {
                additional_reference: AdditionalReferenceType::MinimumFee,
                additional_reference_unit: ReferenceUnit::Hour,
                price_per_additional_reference_unit: "1.00".parse().unwrap(),
            },
            AdditionalReference {
                additional_reference: AdditionalReferenceType::MinimumFee,
                additional_reference_unit: ReferenceUnit::Hour,
                price_per_additional_reference_unit: "3.00".parse().unwrap(),
            },
        ]);
        let err = record.validate().unwrap_err();
        assert!(err.iter().any(|x| x.message.contains("which one applies is undefined")), "{err}");
    }

    #[test]
    fn a_duplicate_product_id_is_reported() {
        let data = PricingProductData {
            operator_id: "DE*ABC".parse().unwrap(),
            operator_name: None,
            provider_id: ProviderIdOrAll::All,
            pricing_default_price: "0.40".parse().unwrap(),
            pricing_default_price_currency: Text::new("EUR").unwrap(),
            pricing_default_reference_unit: ReferenceUnit::KilowattHour,
            pricing_product_data_records: vec![product(), product()],
            extensions: Extensions::new(),
        };
        let err = data.validate().unwrap_err();
        assert_eq!(err.as_slice()[0].pointer, "/PricingProductDataRecords/1/ProductID");
    }
}
