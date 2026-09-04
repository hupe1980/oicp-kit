+++
title = "CDR pre-flight"
weight = 50
description = "German calibration law, and catching a rejected CDR before the session is a month old."
+++

Hubject validates a CDR when it is submitted, and the EMP validates it again when it is billed — by
which time the session is over, the driver has left, and a rejected CDR is a written-off sale.

Several of the rules that get CDRs rejected cannot be checked from the CDR alone.

```rust
use oicp_kit::eichrecht::{CdrCheck, Severity};

let findings = CdrCheck::new()
    .against_evse(&evse)                     // the charging point's own record
    .with_known_products(["AC 1", "DC"])     // the tariffs this operator published
    .run(&cdr);

for finding in &findings {
    println!("{finding}");
}
assert!(CdrCheck::new().against_evse(&evse).is_submittable(&cdr));
```

Findings are ordered most serious first. `Error` means the CDR will be rejected or will not settle;
`Warning` means it will be accepted and then cost you something.

## What it catches

**Calibration law.** German law requires an EV driver to verify independently that the energy they
were billed for is what the meter measured. When the *EVSE record* says
`CalibrationLawDataAvailability: External`, `SignedMeteringValues` is mandatory on every CDR from
it — a condition on a different object, which no CDR-only validator can see:

```
[error] /SignedMeteringValues: DE*ABC*E1 reports CalibrationLawDataAvailability 'External', which
        makes SignedMeteringValues mandatory on every CDR from it; this CDR carries none, so the
        driver cannot verify the measurement and the session is not billable under German
        calibration law
```

It also warns about signed values with no `Start` or `End` reading — the final reading is the one
the invoice is based on — about signed values with no verification info attached, and about values
**out of order**:

> *SignedMeteringValue `SHOULD` be always sent in following order: 1. Start, 2. Progress1,
> 3. Progress2, … SignedMeteringValue for Metering Status "End".*

Transparency software reads the list as a sequence: first entry the opening reading, last the
closing one. Reversed, the delta it computes is negative, and the driver's own verification
disagrees with the invoice.

These three read only the CDR, so they run whether or not you supply an EVSE record.

**Plausibility.** A CDR claiming more energy than the charging point could physically have
delivered will settle, and then be disputed:

```
[warning] /ConsumedEnergy: 400 kWh in 3600 s from a 22 kW charging point is above the physical
          maximum plus margin (26.4 kWh); this CDR will settle and then be disputed
```

The default margin is 120%, because `Power` is the rated maximum of one facility, meters round, and
a charging point may have several. Tune it with `with_plausibility_margin_percent`, or turn it off.

**Unpublished tariffs.** A `PartnerProductID` the operator never published has no price at the EMP,
so the session settles at whatever the default turns out to be.

**Everything `validate()` checks**, folded in — the energy identity, the timestamp order, the field
limits — so one call covers the lot.

## What it does not do

It does not verify signatures. That is what transparency software is for, and the formats (OCMF,
EDL40, Alfen) are outside OICP. What this crate guarantees is that the signed data **arrives** and
**survives byte for byte**: `SignedMeteringValue` is a plain string, and nothing in the crate
rewrites it.

## In a submission pipeline

```console
$ oicp cdr session.json --evse charging-point.json --products 'AC 1,DC'
```

Exits non-zero on an error.
