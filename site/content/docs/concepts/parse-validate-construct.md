+++
title = "Parse, validate, construct"
weight = 30
description = "Why decoding never fails on a spec violation, and where strictness lives instead."
+++

One rule runs through the whole crate:

> **Parse permissively, validate explicitly, construct strictly.**

## Parse permissively

A `PullEvseData` page carries up to a few thousand EVSE records drawn from dozens of operators. One
operator's `HotlinePhoneNumber` missing its leading `+` must not make the other records
undecodable.

```rust
let page: EvseDataResponse = serde_json::from_str(&body)?;   // never fails on a spec violation
assert_eq!(page.content.len(), 2000);
```

A roaming platform that drops a page because one CPO is sloppy is worse than one that accepts the
page and reports the problem.

## Validate explicitly

```rust
use oicp_kit::types::Validate;

for record in &page.content {
    if let Err(violations) = record.validate() {
        for v in violations.iter() {
            tracing::warn!("{} [{}]: {}", v.pointer, v.code, v.message);
        }
    }
}
```

The pointers are [RFC 6901](https://datatracker.ietf.org/doc/html/rfc6901) JSON Pointers into the
JSON the peer actually sent — `/Address/PostalCode`, `/ChargingFacilities/0/Power` — using OICP's
wire names, not the snake-case Rust field names.

Violations are classified, so a pipeline can act on them:

| Code | Means |
|---|---|
| `too_long` / `too_short` | a length limit from a property table |
| `pattern_mismatch` | a regular expression, or an unknown enum value |
| `out_of_range` | a numeric bound |
| `inconsistent` | a cross-field rule — the energy identity, timestamp order |
| `missing_conditional` | a field required because of another field's value |
| `exclusive_choice` | mutually exclusive fields, both set |
| `imprecise` | a number that cannot survive a JSON round trip |

### "It parses" is not "it is conformant"

`pattern_mismatch` covers more than it looks like. A field whose *notation* has a grammar is
checked against that grammar, not merely against whether a number can be read out of it —
`GeoCoordinates` is the clearest case, with one regular expression per notation:

| Notation | Grammar | Rejected |
|---|---|---|
| `DecimalDegree` | `^-?1?\d{1,2}\.\d{1,6}$` | `"52"` — OICP has no notation for a whole number of degrees — and `"9.3609222"`, one place too many |
| `Google` | the same, twice, latitude first | `"52 9"` |
| `DegreeMinuteSeconds` | `^-?1?\d{1,2}°[ ]?\d{1,2}'[ ]?\d{1,2}\.\d+''$` | `"9°21'39''"` — the seconds' fractional part is not optional |

All three parse perfectly well as numbers. None of them is a coordinate OICP accepts.

## Construct strictly

An object *this crate builds* should never be out of spec, including one it derives from another:
`GeoCoordinates::to_format` rounds a converted coordinate to the six decimal places its grammar
allows, because dividing degrees-minutes-seconds by 3600 produces twenty-eight of them, and
`from_decimal_degrees(52, 13)` writes `52.0`, not `52`.

Every wire type's builder validates:

```rust
let address = Address::builder()
    .country("DE")          // alpha-2, but OICP 2.2 and 2.3 allow only alpha-3
    .city("Berlin")
    .street("EUREF CAMPUS")
    .postal_code("10829")
    .house_num("22")
    .build();               // → Err(Violations)

assert_eq!(address.unwrap_err().as_slice()[0].pointer, "/Country");
```

`build()` returns `Result<T, Violations>`. `build_unchecked()` skips the check, for a test fixture
or for re-emitting a peer's payload exactly as it arrived — and its name says what it is.

The client applies the same rule at the last moment: `ClientConfig::validate_requests` is on by
default, so a non-conformant request is refused locally with a JSON Pointer rather than by Hubject
with `022 Data error` and no detail.

## The server chooses

A server receiving a non-conformant request has a real decision to make, so the trait exposes it:

```rust
fn on_invalid_request(&self, violations: &Violations) -> Option<Acknowledgement> {
    None   // accept it anyway
}
```

The default refuses with `022 Data error`, which is what Hubject does. Overriding it to `None` is
often right for a CDR: refusing to be *paid* because a CPO overran a text field is not a commercial
position anyone wants to defend.
