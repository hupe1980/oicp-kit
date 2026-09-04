+++
title = "Reading a CDR"
weight = 50
description = "The money document, its cross-field rules, and the ones that need more than the CDR."
+++

Everything else in OICP is operational. The charge detail record is what two companies settle
against.

## The rules inside the record

`ChargeDetailRecord::validate` checks what can be checked from the record alone:

**The energy identity.** `ConsumedEnergy` is *defined* as `MeterValueEnd - MeterValueStart`.

```rust
let mut cdr = samples::charge_detail_record("DE*ABC*E1", session);
cdr.consumed_energy = Number::from(99);

let err = cdr.validate().unwrap_err();
assert_eq!(err.as_slice()[0].pointer, "/ConsumedEnergy");
// ConsumedEnergy is 99 but MeterValueEnd - MeterValueStart is 10.0
```

Exact, because [numbers are decimals](@/docs/concepts/numbers.md).

**Timestamp order.** Charging happens inside the session:
`SessionStart ≤ ChargingStart ≤ ChargingEnd ≤ SessionEnd`. Each violation is reported at its own
field, so the message says which timestamp to look at.

**A meter that does not run backwards**, non-negative energy, and at most ten signed metering
values.

## The rules that need more

Several reasons a CDR is rejected cannot be seen from the CDR:

* `SignedMeteringValues` is **mandatory** when the *EVSE* reports
  `CalibrationLawDataAvailability: External`. The condition is on the charging point's record.
* A session that claims more energy than the charging point could physically have delivered will
  settle, and then be disputed.
* A `PartnerProductID` the operator never published has no price at the EMP.

That is what [`eichrecht::CdrCheck`](@/docs/layers/eichrecht.md) is for:

```rust
let findings = CdrCheck::new()
    .against_evse(&evse)
    .with_known_products(["AC 1", "DC"])
    .run(&cdr);

if !CdrCheck::new().against_evse(&evse).is_submittable(&cdr) {
    // …fix it before the session is a month old and the sale is written off
}
```

## Calibration law

German calibration law (Eichrecht) requires an EV driver to be able to verify, independently, that
the energy they were billed for is what the meter measured. In OICP that travels on the CDR:

* `SignedMeteringValues` — the meter's own signed readings, in transparency-software format.
* `CalibrationLawVerificationInfo` — the certificate id, public key, and a URL to the compiled data.

This crate does not verify signatures — that is what transparency software is for, and the formats
are outside OICP. What it does is make sure the data **arrives** and **survives byte for byte**,
and that a CDR required to carry it actually does.

## The one field with two spellings

`HubProviderID` is spelled `HubProviderId` in the EMP OpenAPI schema and `HubProviderID` everywhere
else — including that schema's own example. This crate reads both and writes the leading document's.
See [Errata](@/docs/reference/errata.md), `OICP23-E001`.
