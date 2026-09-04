//! `CdrCheck` — the cross-object rules, run before a CDR is submitted.

use core::fmt;

use rust_decimal::Decimal;

use crate::cpo::{ChargeDetailRecord, EvseDataRecord, MeteringStatus, SignedMeteringValue};
use crate::types::{CalibrationLawDataAvailability, Validate, ViolationCode};

/// How much trouble a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The CDR will be rejected, or will not settle.
    Error,
    /// The CDR is likely to be accepted, but something is off that will cost you later.
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Error => "error",
            Self::Warning => "warning",
        })
    }
}

/// Something wrong with a CDR, found before it was submitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// How much trouble it is.
    pub severity: Severity,
    /// A JSON Pointer into the CDR.
    pub pointer: String,
    /// What is wrong, and why it matters.
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.severity, self.pointer, self.message)
    }
}

/// Checks a charge detail record before it goes to Hubject.
///
/// # What this catches that [`Validate`] cannot
///
/// [`Validate`](crate::types::Validate) checks a CDR against itself: the energy identity, the
/// timestamp order, the field lengths. That is most of the rules, and it is what the wire type
/// does on its own.
///
/// The rest need context the CDR does not carry:
///
/// * **`SignedMeteringValues` is mandatory when the EVSE says `External`.** The condition is on the
///   *EVSE data record*, not the CDR. Give the check the EVSE with
///   [`against_evse`](Self::against_evse) and it can enforce it.
/// * **A session should be plausible.** A CDR claiming 400 kWh in twenty minutes from a 22 kW
///   charging point will settle, and then be disputed. Better to find it before it is sent.
/// * **The tariff should exist.** A `PartnerProductID` the CPO never published prices for is
///   settled at whatever the default price turns out to be.
///
/// Findings are ordered [`Error`](Severity::Error) first.
#[derive(Clone, Debug, Default)]
pub struct CdrCheck<'a> {
    evse: Option<&'a EvseDataRecord>,
    known_products: Vec<String>,
    /// The multiplier on the charging point's rated output, as an exact ratio — 1.2 for 120%.
    max_plausible_factor: Option<Decimal>,
}

/// `percent` as a ratio: `120` is 1.2.
fn ratio(percent: i64) -> Decimal {
    Decimal::from(percent) / Decimal::from(100)
}

impl<'a> CdrCheck<'a> {
    /// A check with the default rules.
    #[must_use]
    pub fn new() -> Self {
        Self { evse: None, known_products: vec![], max_plausible_factor: Some(ratio(120)) }
    }

    /// Also check the rules that depend on the charging point's own data record.
    #[must_use]
    pub fn against_evse(mut self, evse: &'a EvseDataRecord) -> Self {
        self.evse = Some(evse);
        self
    }

    /// Also check that the CDR's `PartnerProductID` is one the CPO has published.
    #[must_use]
    pub fn with_known_products<I, S>(mut self, products: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.known_products = products.into_iter().map(Into::into).collect();
        self
    }

    /// How far above the charging point's rated output a session may claim before it is
    /// implausible. The default is 120%.
    ///
    /// Some slack is right: `Power` is the rated maximum of one facility, meters round, and a
    /// charging point may have several facilities.
    #[must_use]
    pub fn with_plausibility_margin_percent(mut self, percent: i64) -> Self {
        self.max_plausible_factor = Some(ratio(percent));
        self
    }

    /// Turns off the plausibility check.
    #[must_use]
    pub const fn without_plausibility_check(mut self) -> Self {
        self.max_plausible_factor = None;
        self
    }

    /// Runs every rule, most serious first.
    #[must_use]
    pub fn run(&self, cdr: &ChargeDetailRecord) -> Vec<Finding> {
        let mut findings = vec![];

        // Everything the CDR can be checked against on its own.
        if let Err(violations) = cdr.validate() {
            for violation in &violations {
                findings.push(Finding {
                    // An imprecise number will settle; an inconsistency will be disputed.
                    severity: if violation.code == ViolationCode::Imprecise {
                        Severity::Warning
                    } else {
                        Severity::Error
                    },
                    pointer: violation.pointer.clone(),
                    message: violation.message.clone(),
                });
            }
        }

        self.check_calibration_law(cdr, &mut findings);
        Self::check_signed_values(cdr, &mut findings);
        self.check_plausibility(cdr, &mut findings);
        self.check_product(cdr, &mut findings);
        self.check_evse_match(cdr, &mut findings);

        findings.sort_by(|a, b| a.severity.cmp(&b.severity).then_with(|| a.pointer.cmp(&b.pointer)));
        findings
    }

    /// Whether the CDR is fit to submit — no findings of severity [`Error`](Severity::Error).
    #[must_use]
    pub fn is_submittable(&self, cdr: &ChargeDetailRecord) -> bool {
        !self.run(cdr).iter().any(|f| f.severity == Severity::Error)
    }

    fn check_calibration_law(&self, cdr: &ChargeDetailRecord, findings: &mut Vec<Finding>) {
        let Some(evse) = self.evse else { return };

        // "This field MUST be provided when the EVSEID in the ChargeDetailRecord contains the
        //  'External' value in the CalibrationLawDataAvailability field."
        if evse.calibration_law_data_availability == CalibrationLawDataAvailability::External {
            let values = cdr.signed_metering_values.as_ref();
            let has_values = values.is_some_and(|v| !v.is_empty());
            if !has_values {
                findings.push(Finding {
                    severity: Severity::Error,
                    pointer: "/SignedMeteringValues".into(),
                    message: format!(
                        "{} reports CalibrationLawDataAvailability 'External', which makes \
                         SignedMeteringValues mandatory on every CDR from it; this CDR carries none, \
                         so the driver cannot verify the measurement and the session is not \
                         billable under German calibration law",
                        cdr.evse_id
                    ),
                });
            } else if cdr.calibration_law_verification_info.is_none() {
                findings.push(Finding {
                    severity: Severity::Warning,
                    pointer: "/CalibrationLawVerificationInfo".into(),
                    message: "the CDR carries signed metering values but nothing to verify them with; \
                              add the certificate id, public key or verification URL"
                        .into(),
                });
            }
        }
    }

    /// The rules about the signed values themselves, which need only the CDR.
    ///
    /// Kept apart from [`check_calibration_law`](Self::check_calibration_law), which returns early
    /// without an EVSE data record: these run either way.
    fn check_signed_values(cdr: &ChargeDetailRecord, findings: &mut Vec<Finding>) {
        // The spec asks for Start first and End last. A missing End reading means the driver
        // cannot verify the quantity they were billed for, only the one they started at.
        if let Some(values) = &cdr.signed_metering_values {
            let statuses: Vec<_> = values.iter().filter_map(|v| v.metering_status.clone()).collect();
            if !statuses.is_empty() {
                if !statuses.contains(&MeteringStatus::End) {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        pointer: "/SignedMeteringValues".into(),
                        message: "no signed metering value of status 'End'; the final reading is the \
                                  one the invoice is based on"
                            .into(),
                    });
                }
                if !statuses.contains(&MeteringStatus::Start) {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        pointer: "/SignedMeteringValues".into(),
                        message: "no signed metering value of status 'Start'; without it the delta \
                                  cannot be verified"
                            .into(),
                    });
                }
            }
            if values.iter().any(|v| v.signed_metering_value.is_none()) {
                findings.push(Finding {
                    severity: Severity::Error,
                    pointer: "/SignedMeteringValues".into(),
                    message: "a signed metering value entry carries no value".into(),
                });
            }

            // "SignedMeteringValue `SHOULD` be always sent in following order: 1. Start,
            // 2. Progress1, 3. Progress2, … Signed­MeteringValue for Metering Status 'End'."
            //
            // Transparency software reads the list as a sequence, and a reader that takes the
            // first entry as the opening reading and the last as the closing one computes the
            // delta backwards when they are reversed. A `SHOULD`, so a warning — but a warning the
            // driver's verification depends on.
            if let Some(out_of_place) = first_out_of_order(values) {
                findings.push(Finding {
                    severity: Severity::Warning,
                    pointer: format!("/SignedMeteringValues/{out_of_place}"),
                    message: "the signed metering values are not in the order the specification \
                              asks for — Start first, Progress in between, End last; transparency \
                              software reads the list as a sequence"
                        .into(),
                });
            }
        }
    }

    fn check_plausibility(&self, cdr: &ChargeDetailRecord, findings: &mut Vec<Finding>) {
        let (Some(factor), Some(evse)) = (self.max_plausible_factor, self.evse) else { return };
        // No readable pair of timestamps, no duration to judge a rate against. `Validate` has
        // already reported the timestamp itself.
        let Some(seconds) = cdr.charging_duration_seconds().filter(|s| *s > 0) else { return };
        // The best the charging point could physically have delivered, plus the margin.
        let Some(max_power) = evse.charging_facilities.iter().map(|f| f.power.get()).max() else { return };
        let hours = Decimal::from(seconds) / Decimal::from(3600);
        let ceiling = max_power * hours * factor;

        if cdr.consumed_energy.get() > ceiling {
            findings.push(Finding {
                severity: Severity::Warning,
                pointer: "/ConsumedEnergy".into(),
                message: format!(
                    "{} kWh in {} s from a {max_power} kW charging point is above the physical \
                     maximum plus margin ({} kWh); this CDR will settle and then be disputed",
                    cdr.consumed_energy,
                    seconds,
                    ceiling.round_dp(3),
                ),
            });
        }
    }

    fn check_product(&self, cdr: &ChargeDetailRecord, findings: &mut Vec<Finding>) {
        if self.known_products.is_empty() {
            return;
        }
        match &cdr.partner_product_id {
            None => findings.push(Finding {
                severity: Severity::Warning,
                pointer: "/PartnerProductID".into(),
                message: "no PartnerProductID; the session settles at the EMP's default price, \
                          not at the tariff the driver was shown"
                    .into(),
            }),
            Some(product) if !self.known_products.iter().any(|p| p == product.as_str()) => {
                findings.push(Finding {
                    severity: Severity::Error,
                    pointer: "/PartnerProductID".into(),
                    message: format!(
                        "{:?} is not one of the products this operator has published ({}); \
                         the EMP has no price for it",
                        product.as_str(),
                        self.known_products.join(", ")
                    ),
                });
            }
            Some(_) => {}
        }
    }

    fn check_evse_match(&self, cdr: &ChargeDetailRecord, findings: &mut Vec<Finding>) {
        let Some(evse) = self.evse else { return };
        if evse.evse_id != cdr.evse_id {
            findings.push(Finding {
                severity: Severity::Error,
                pointer: "/EvseID".into(),
                message: format!(
                    "the CDR is for {} but it is being checked against {}; the check's other \
                     findings are meaningless",
                    cdr.evse_id, evse.evse_id
                ),
            });
        } else if !evse.is_hubject_compatible {
            findings.push(Finding {
                severity: Severity::Warning,
                pointer: "/EvseID".into(),
                message: format!(
                    "{} is published with IsHubjectCompatible false, so this session cannot have \
                     been started through Hubject",
                    cdr.evse_id
                ),
            });
        }
    }
}

// Fixtures come from `testkit::samples`, so these tests compile when that feature is on.
/// The index of the first signed value that is out of the specification's order, if any.
///
/// The order is `Start`, then any number of `Progress`, then `End`. Entries whose status is absent
/// or unrecognised sit wherever they are: this reports a value that is demonstrably in the wrong
/// place, not one whose place cannot be known.
fn first_out_of_order(values: &[SignedMeteringValue]) -> Option<usize> {
    fn rank(value: &SignedMeteringValue) -> Option<u8> {
        match value.metering_status.as_ref()? {
            MeteringStatus::Start => Some(0),
            MeteringStatus::Progress => Some(1),
            MeteringStatus::End => Some(2),
            MeteringStatus::Custom(_) => None,
        }
    }
    let mut highest = 0;
    for (index, value) in values.iter().enumerate() {
        let Some(rank) = rank(value) else { continue };
        if rank < highest {
            return Some(index);
        }
        highest = rank;
    }
    None
}

#[cfg(all(test, feature = "testkit"))]
mod tests {
    use super::*;
    use crate::cpo::SignedMeteringValue;
    use crate::testkit::samples;
    use crate::types::{Number, Text};

    fn external_evse() -> EvseDataRecord {
        let mut evse = samples::evse_data_record("DE*ABC*E1");
        evse.calibration_law_data_availability = CalibrationLawDataAvailability::External;
        evse
    }

    fn cdr() -> ChargeDetailRecord {
        samples::charge_detail_record("DE*ABC*E1", samples::session_id())
    }

    #[test]
    fn a_conformant_cdr_without_context_passes() {
        assert!(CdrCheck::new().run(&cdr()).is_empty());
        assert!(CdrCheck::new().is_submittable(&cdr()));
    }

    #[test]
    fn an_external_evse_makes_signed_values_mandatory() {
        let evse = external_evse();
        let findings = CdrCheck::new().against_evse(&evse).run(&cdr());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[0].pointer, "/SignedMeteringValues");
        assert!(!CdrCheck::new().against_evse(&evse).is_submittable(&cdr()));

        // The same CDR against a Local EVSE is fine — the rule is contextual.
        let local = samples::evse_data_record("DE*ABC*E1");
        assert!(CdrCheck::new().against_evse(&local).run(&cdr()).is_empty());
    }

    #[test]
    fn signed_values_without_a_start_or_end_reading_are_flagged() {
        let mut record = cdr();
        record.signed_metering_values = Some(vec![SignedMeteringValue {
            signed_metering_value: Some(Text::new("AAAA").unwrap()),
            metering_status: Some(MeteringStatus::Progress),
        }]);
        let findings = CdrCheck::new().against_evse(&external_evse()).run(&record);
        assert!(findings.iter().any(|f| f.message.contains("'End'")));
        assert!(findings.iter().any(|f| f.message.contains("'Start'")));
        // Warnings only: the CDR will still settle.
        assert!(findings.iter().all(|f| f.severity == Severity::Warning));
    }

    #[test]
    fn the_signed_value_rules_do_not_need_an_evse() {
        // They read only the CDR, so a caller who has not got the charging point's data record
        // still gets them.
        let mut record = cdr();
        record.signed_metering_values = Some(vec![SignedMeteringValue {
            signed_metering_value: None,
            metering_status: Some(MeteringStatus::Progress),
        }]);
        let findings = CdrCheck::new().run(&record);
        assert!(findings.iter().any(|f| f.message.contains("carries no value")), "{findings:?}");
        assert!(findings.iter().any(|f| f.message.contains("'Start'")));
        assert!(findings.iter().any(|f| f.message.contains("'End'")));
    }

    #[test]
    fn signed_values_out_of_order_are_flagged() {
        // Transparency software reads the list as a sequence: first entry the opening reading,
        // last the closing one. Reversed, the delta it computes is negative.
        let value = |status| SignedMeteringValue {
            signed_metering_value: Some(Text::new("AAAA").unwrap()),
            metering_status: Some(status),
        };
        let mut record = cdr();
        record.signed_metering_values = Some(vec![value(MeteringStatus::End), value(MeteringStatus::Start)]);
        let findings = CdrCheck::new().run(&record);
        let ordering = findings
            .iter()
            .find(|f| f.message.contains("not in the order"))
            .expect("the reversed pair is reported");
        assert_eq!(ordering.severity, Severity::Warning, "the specification says SHOULD");
        assert_eq!(ordering.pointer, "/SignedMeteringValues/1");

        // Start, two Progress, End is exactly the order asked for.
        record.signed_metering_values = Some(vec![
            value(MeteringStatus::Start),
            value(MeteringStatus::Progress),
            value(MeteringStatus::Progress),
            value(MeteringStatus::End),
        ]);
        assert!(!CdrCheck::new().run(&record).iter().any(|f| f.message.contains("not in the order")));

        // An entry whose status nobody recognises sits wherever it is; its place cannot be known.
        record.signed_metering_values = Some(vec![
            value(MeteringStatus::Start),
            value(MeteringStatus::Custom("Interim".into())),
            value(MeteringStatus::End),
        ]);
        assert!(!CdrCheck::new().run(&record).iter().any(|f| f.message.contains("not in the order")));
    }

    #[test]
    fn a_physically_impossible_session_is_flagged_before_it_is_disputed() {
        let mut record = cdr();
        // One hour of charging at a 22 kW point cannot deliver 400 kWh.
        record.consumed_energy = Number::from(400);
        record.meter_value_end = Some(Number::from(400));

        let findings = CdrCheck::new().against_evse(&samples::evse_data_record("DE*ABC*E1")).run(&record);
        assert!(
            findings.iter().any(|f| f.pointer == "/ConsumedEnergy" && f.message.contains("physical maximum"))
        );

        // 22 kWh in an hour from a 22 kW point is exactly right.
        let mut fine = cdr();
        fine.consumed_energy = Number::from(22);
        fine.meter_value_end = Some(Number::from(22));
        let findings = CdrCheck::new().against_evse(&samples::evse_data_record("DE*ABC*E1")).run(&fine);
        assert!(findings.is_empty());
    }

    #[test]
    fn an_unpublished_tariff_is_an_error() {
        let findings = CdrCheck::new().with_known_products(["DC", "AC 3"]).run(&cdr());
        // The sample CDR says "AC 1".
        assert!(findings.iter().any(|f| f.pointer == "/PartnerProductID" && f.severity == Severity::Error));

        let findings = CdrCheck::new().with_known_products(["AC 1"]).run(&cdr());
        assert!(findings.is_empty());
    }

    #[test]
    fn checking_a_cdr_against_the_wrong_evse_says_so() {
        let other = samples::evse_data_record("DE*ABC*E2");
        let findings = CdrCheck::new().against_evse(&other).run(&cdr());
        assert!(findings.iter().any(|f| f.pointer == "/EvseID" && f.severity == Severity::Error));
    }

    #[test]
    fn the_energy_identity_is_reported_as_an_error() {
        let mut record = cdr();
        record.consumed_energy = Number::from(99);
        let findings = CdrCheck::new().run(&record);
        assert!(findings.iter().any(|f| f.pointer == "/ConsumedEnergy" && f.severity == Severity::Error));
    }

    #[test]
    fn findings_are_ordered_most_serious_first() {
        let mut record = cdr();
        record.consumed_energy = Number::from(99); // an error
        let findings = CdrCheck::new()
            .against_evse(&external_evse()) // another error, plus warnings
            .run(&record);
        assert!(findings.len() >= 2);
        assert_eq!(findings[0].severity, Severity::Error);
        assert!(findings.windows(2).all(|w| w[0].severity <= w[1].severity));
    }
}
