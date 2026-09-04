+++
title = "Identifiers"
weight = 10
description = "Two grammars per field, why the wire form must survive, and how equality works."
+++

Every OICP identifier accepts **two** encodings, and the specification gives one regular expression
that unions them:

* **ISO 15118-1** — `DE*AB7*E840*6487`, `DEAB7E8406487`. Alpha-2 country code, `*` optional.
* **DIN SPEC 91286:2011-11** — `+49*810*000*438`. ITU-T E.164 country code, `*` mandatory.

## Why not `String`

A library that models these as strings cannot tell an operator's identifier from a provider's,
cannot say which country an EVSE is in, and — worst — invites the caller to normalise.

Normalising breaks production. Hubject compares the `OperatorID`/`ProviderID` in your URL path
against your TLS client certificate, and the comparison is **textual**. `DE*ABC` and `DEABC` are
the same operator to a human and to this crate's `Eq`; they are different strings to a certificate
check, and the difference is `017 Unauthorized Access` on every request.

## What this crate does

```rust
use oicp_kit::types::EvseId;

let a: EvseId = "DE*AB7*E840*6487".parse()?;
let b: EvseId = "DEAB7E8406487".parse()?;

assert_eq!(a, b);                              // the same charging spot
assert_eq!(a.to_string(), "DE*AB7*E840*6487"); // …written back exactly as it arrived
assert_eq!(b.to_string(), "DEAB7E8406487");
assert_eq!(a.canonical(), "DEAB7E8406487");    // …and a key for your database
```

* `FromStr` parses the grammar and records which standard matched.
* `Display` and `Serialize` return the exact text that arrived.
* `PartialEq`, `Hash` and `Ord` compare semantically: case-insensitively, ignoring the optional
  separators **and the optional DIN `+`**. `EvseID`'s DIN grammar is `\+?[0-9]{1,3}\*…`, so
  `+49*810*000*438` and `49*810*000*438` are one charging spot — the specification prints both —
  and an EMP that stores one spelling and looks up the other has to find it.

All three read the same bytes, so they cannot disagree: equal values hash alike and compare
`Equal`, which is what a `HashMap` and a `BTreeMap` of charging points each rely on.

An ISO `EvseId` never equals a DIN one — not because the standard is part of the comparison, but
because the grammars cannot collide: an ISO identifier begins with two letters and a DIN one with
digits. Folding the standard into equality would buy nothing and cost transitivity, since
`ProviderID` satisfies *both* grammars — the specification lists `DE8EO` under both headings — and
a value that compares equal to two values that differ from each other is not an equivalence.
`IdStandard::Either` is the honest answer to "which standard is this?", not a third kind of
identity.

## Deriving the counterparty

Most OICP messages carry no `OperatorID` at all, because Hubject derives it from the `EvseID`. So
can you:

```rust
let evse: EvseId = "DE*AB7*E840*6487".parse()?;
assert_eq!(evse.operator_id().to_string(), "DE*AB7");
assert_eq!(evse.country(), "DE");

let contract: EvcoId = "DE-DCB-C12345678-X".parse()?;
assert_eq!(contract.provider_id().to_string(), "DE-DCB");
```

## The `ProviderID` exception

For `ProviderID`, the two grammars **coincide**: the specification lists `DE8EO` and `DE-8EO` as
examples under *both* headings. So a provider identifier's standard is
[`IdStandard::Either`](https://docs.rs/oicp-kit/latest/oicp_kit/types/enum.IdStandard.html), and
`DE-8EO`, `DE*8EO` and `DE8EO` all compare equal — which is what they are.

Modelling this wrongly is not hypothetical: it is a bug this crate had, caught by its own test
suite, before the equality rule was relaxed for that one type.

## Malformed identifiers still arrive

A page of two thousand EVSE records must not fail because one operator's identifier is wrong:

```rust
let id: EvseId = serde_json::from_str(r#""garbage""#)?;   // decodes
assert!(!id.is_well_formed());
assert_eq!(serde_json::to_string(&id)?, r#""garbage""#);  // and survives
assert!(id.validate().is_err());                          // and is reported
```

See [Parse, validate, construct](@/docs/concepts/parse-validate-construct.md).

## The types

| Type | Grammar | Notes |
|---|---|---|
| `EvseId` | ISO + DIN | the `E` marker is what makes an ISO one unambiguous |
| `EvcoId` | ISO + DIN | ISO has a `C` prefix and eight instance characters; DIN has six |
| `OperatorId` | ISO + DIN | ISO is alpha, DIN is numeric |
| `ProviderId` | coincident | see above |
| `ChargingPoolId` | emi³ | the ISO `EvseID` grammar with `P` for pool |
| `SessionId` | GUID-shaped | letters are allowed in every group, so not a hex UUID |
| `Uid` | 8/14/20 hex | RFID cards; compares case-insensitively |
| `ProviderIdOrAll` | — | a `ProviderId`, or the literal `*` for offer-to-all pricing |
