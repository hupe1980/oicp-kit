+++
title = "Design decisions"
weight = 40
description = "The choices that shaped the crate, and what was rejected."
+++

## One wire model, not a version tree

`ocpi-kit` has `v2_3_0`, `v2_2_1` and `v2_1_1` modules and converts between them. `oicp-kit` has
none of that, because OICP is different: 2.1 is retired, 2.2 is superseded, and **2.3 is the last
version**. Modelling versions that nobody may use would be cost without benefit.

What OICP versions instead is *services* — `evsepush/v23`, `charging/v21`, `cdrmgmt/v22` — and that
lives in one table in [`transport`](@/docs/layers/transport.md).

## `Identification` is a Rust enum, not a struct of five options

OICP models it as an object with five optional members, and Hubject's own examples fill in all five
at once — which no real payload does. Modelling that faithfully in Rust would mean every consumer
writes a chain of `if let Some(…)` with an unreachable fallback.

So the wire shape stays exactly as specified, and the Rust type is a closed choice. The cost:
a payload carrying several members keeps the first in spec order and drops the rest. That is the
one place in this crate where data does not survive a round trip, and it is deliberate — an
`Identification` naming two different drivers has no faithful representation, and forwarding the
ambiguity moves a billing dispute downstream. `Identification::from_wire` reports every member that
was present, for a conformance run that wants to see it.

## `GeoCoordinates` keeps its notation

The same position in `Google` and `DecimalDegree` notation is the same position — but a record that
arrived as one and goes back out as the other is a *changed* record, and for a hub forwarding it, a
corrupted one. So the variant is preserved and `to_format` is explicit.

## Open enums by default, closed by exception

The reverse of `ocpi-kit`, where OCPI 2.3.0 formally distinguishes `enum` from `OpenEnum`. OICP has
no such notion — every enumerated type is written as a closed list — but Hubject edits the documents
in place and the lists grow. Discarding a value would make a hub lossy.

The exceptions are the two where an unknown value has no safe reading: `ActionType` and
`AuthorizationStatus`. See [Extensions and open enums](@/docs/concepts/extensions.md).

## Builders validate; `build_unchecked` says what it is

Field types are permissive so a builder can take a `&str`, and the check happens once on the
finished object. The alternative — fallible per-field setters — makes a twenty-field
`EvseDataRecord` unreadable, and pushing validation to a separate call means it gets forgotten.

## Identifier grammars are hand-written, not a regex

The specification states each grammar as a regular expression. Implementing them as character
checks keeps the crate dependency-light, keeps parsing a 2000-record page allocation-free per
field, and puts each rule next to the sentence it comes from. The spec's own examples are the
tests, and property tests fuzz the round-trip.

## One float in the crate, and it is the JSON boundary

`types::Number` converts through `f64` to write a fractional JSON number, because `serde_json`
represents one that way unless its `arbitrary_precision` feature is on — and that feature changes
`serde_json::Value` for every crate in the build, which is not a library's decision to make. The
conversion is exact for every value OICP carries, and `Number::json_round_trips()` reports the
values where it would not be.

Nothing else in the crate touches a float: the retry backoff walks its jitter spread in integer
milliseconds, and the CDR checker's plausibility margin is an exact `Decimal` ratio.

## The server trait has no default methods

A CPO that has not decided what to do about reservations should say `ServiceNotAvailable` out loud,
in code someone can read. A default would let "we never implemented this" and "we deliberately do
not offer this" look identical.

## `MockHubject` is not an HBS replacement

It simulates the broker for testing: routing, session tracking, spec-accurate status codes and
pagination. It does not implement Hubject's contract model, its partner registry, or its billing.
Passing `scenarios::run_all()` does not make Hubject's own integration test unnecessary.

## Rejected: generating the wire model from the OpenAPI

Tempting — the schemas are machine-readable and there are about seventy of them. Rejected because
the schemas encode rules that codegen cannot express: the `LastCall`/filter exclusivity, the
per-process `Identification` constraints, the energy identity, the `IsOpen24Hours`/`OpeningTimes`
relationship. Those rules are most of the value here. Instead the model is hand-written and `xtask`
diffs the *endpoints* against the schemas, which is the part that genuinely drifts.

## Deferred: an OCPI ↔ OICP bridge

Every roaming platform that speaks both eventually maps `EvseDataRecord` ↔ OCPI `Location`/`EVSE`
and OICP CDR ↔ OCPI CDR. The mapping is lossy in both directions and belongs in neither crate's
core. Since Hubject's September 2025 commitment to native OCPI, everyone on OICP will run both
protocols for years — so a `roaming-bridge` crate on top of `ocpi-kit` and `oicp-kit`, with the same
loss accounting as `ocpi-kit`'s `convert`, is the obvious next thing. It is out of scope for 0.1,
but it is why `types` does not leak idioms that would make the mapping awkward.
