+++
title = "Numbers and money"
weight = 20
description = "Every OICP number is an exact decimal, never a float — and what that costs at the JSON boundary."
+++

Every OICP `number` in this crate is `types::Number`, a wrapper around `rust_decimal::Decimal`,
and every sum, difference and comparison the crate performs on one is decimal arithmetic. There is
**no `f32` or `f64` anywhere in `src/`** except one file, named below: the JSON boundary inside
`types::Number` itself.

## Why this matters more than it sounds

OICP does not merely *carry* numbers that happen to be money. It **defines a relationship between
them**:

> `ConsumedEnergy` — *The difference between MeterValueEnd and MeterValueStart in kWh.*

That is an exact decimal identity, and binary floating point cannot honour it:

```rust
assert_eq!(10.1_f64 - 0.1_f64, 10.000000000000002);   // in f64
```

A CDR is the document two companies settle against. If the library computing or checking that
identity uses floats, then either the check spuriously fails on correct data, or it is loosened
until it stops catching real errors. With decimals it simply holds:

```rust
use oicp_kit::types::Number;

let end: Number = "10.1".parse()?;
let start: Number = "0.1".parse()?;
assert_eq!((end - start).to_string(), "10.0");
```

`ChargeDetailRecord::validate` checks exactly this, and reports a CDR whose stated energy
contradicts its meter readings before it is submitted.

## The JSON boundary

OICP sends these as JSON *numbers*, not strings. `serde_json` represents a fractional JSON number
as an `f64` unless its `arbitrary_precision` feature is on — a feature that changes
`serde_json::Value` for every crate in the build, so this crate does not impose it. Instead:

* Integral values pass through exactly, as JSON integers.
* Fractional values with at most 15 significant decimal digits — which covers every price and
  energy OICP carries — pass through exactly, because the shortest decimal that round-trips an
  `f64` *is* the original decimal.
* Beyond that, `Number::json_round_trips()` returns `false` and `validate()` reports
  `ViolationCode::Imprecise`. It can never happen silently.

A peer that sends a number as a JSON string (`"0.25"`) is tolerated on input; output is always a
JSON number.

## Enforcement

`cargo run -p xtask -- no-floats` scans **all of `src/`** for `f32`/`f64` in code this crate wrote,
and CI runs it on every push.

It is a scan rather than a clippy lint because `clippy::disallowed_types` fires on the `visit_f64`
that serde's derive generates for *every* struct, which says nothing about the code anyone wrote.

**One file is exempt**, and the check prints its name on success: `types/number.rs`, the JSON
boundary described above. Nothing else in the crate touches a float — the retry backoff computes
its jitter in integer milliseconds, and the CDR checker's plausibility margin is an exact ratio.

