+++
title = "Extensions and open enums"
weight = 40
description = "OICP 2.3 is edited in place. What that means for a library that must not lose data."
+++

OICP 2.3 has no extensibility chapter, no version negotiation, and no more versions coming. What it
*does* have is Hubject editing the 2.3 documents **in place**, without a version bump:

* `IsHubjectCompatible` and `IsOpen24Hours` were added to `PullEvseData` this way.
* The CDR schema gained a `PartnerProductID` clarification in 2026.
* `Plug`, `ValueAddedService` and `PaymentOption` have each grown values.

A partner's stack built against last year's snapshot is still expected to forward this year's
payloads intact.

## Unknown fields

Every wire object carries an `extensions` field, flattened into the JSON:

```rust
let json = r#"{"EvseID":"DE*ABC*E1","EvseStatus":"Available","HubjectAddedThis":42}"#;
let record: EvseStatusRecord = serde_json::from_str(json)?;

assert_eq!(record.extensions.get::<u32>("HubjectAddedThis")?, Some(42));
assert_eq!(serde_json::to_string(&record)?, json);   // byte-identical
```

Keys live in a `BTreeMap`, so serialisation order is deterministic. An object that carried no
extensions is written back without the field at all.

## Unknown enum values

Almost every enum in this crate is **open**: a value the specification does not document is kept
rather than rejected.

```rust
let plug: Plug = serde_json::from_str(r#""MCS""#)?;   // a connector added after this crate
assert!(!plug.is_known());
assert_eq!(plug.as_str(), "MCS");
assert_eq!(serde_json::to_string(&plug)?, r#""MCS""#);
assert!(plug.validate().is_err());                    // preserved, and still reported
```

Preserved *and* reported: a conformance run sees it, a forwarding hub does not damage it.

### The two closed enums

Two types reject unknown values, because neither has a safe default:

* **`ActionType`** — the values are `fullLoad`, `update`, `insert`, `delete`, and guessing wrong
  can delete an operator's fleet from the roaming network. See [Delta sync](@/docs/layers/sync.md).
* **`AuthorizationStatus`** — treating an unrecognised status as "probably authorized" gives away
  energy; as "not authorized" it strands a driver. Neither is a decision a library should make.

`ChargingNotificationType` is also closed, because it is what selects the shape of the rest of the
body: an unrecognised value leaves nothing to decode.

## Why this is not just tidiness

A hub sits between two parties who may have agreed on something it knows nothing about. Discarding
a field it does not understand turns a faithful forward into silent data loss — and the party that
notices is the one whose invoices no longer reconcile.
