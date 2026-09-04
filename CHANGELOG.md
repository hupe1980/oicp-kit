# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0]

First release. A complete OICP 2.3 toolkit for both roles and both directions.

### Wire model

* `cpo` — the CPO half: `PushEvseData`, `PushEvseStatus`, `AuthorizeStart`/`Stop`,
  `ChargeDetailRecord`, the four `ChargingNotification`s, the two pricing pushes, and the four
  messages Hubject sends *into* a CPO.
* `emp` — the EMP half: `PullEvseData` with its page envelope, the three `PullEvseStatus` shapes,
  `GetChargeDetailRecords`, `PushAuthenticationData` and the pricing pulls.
* Identifiers (`EvseId`, `EvcoId`, `OperatorId`, `ProviderId`, `ChargingPoolId`, `SessionId`,
  `Uid`, `ProviderIdOrAll`) parse both the ISO 15118 and DIN SPEC 91286 grammars, expose the
  country and the counterparty, and re-serialise byte-identically. `Eq`, `Hash` and `Ord` read one
  set of significant bytes — case, the optional separators and the optional DIN `+` folded away —
  so they cannot disagree with each other, and `+49*810*000*438` is the charging spot
  `49*810*000*438` is.
* Every `number` is an exact decimal (`types::Number`), and every sum, difference and comparison in
  the crate is decimal arithmetic. `cargo run -p xtask -- no-floats` fails on an `f32` or `f64`
  anywhere in `src/` except the one file it names: the JSON boundary in `types::Number`.
* Timestamps that do not parse stay unreadable rather than becoming the epoch: `DateTime::as_offset`
  returns `Option`, and ordering agrees with equality so a `BTreeMap` of timestamps cannot lose one.
* `types::Extensions` preserves undocumented fields; open enums preserve unknown values. Both are
  reported by `Validate` without being discarded.
* Cross-field rules JSON Schema cannot express are checked where the fields are: the CDR energy
  identity and timestamp order, `IsOpen24Hours` against `OpeningTimes`, `LastCall` against the
  filters it excludes, a `PricingProductDataRecord` whose **MINIMUM FEE exceeds its MAXIMUM FEE**
  for the same reference unit — a product under which no session can be priced, from two fields
  each of which is valid alone.
* `GeoCoordinates` is checked against the **grammar of the notation it is written in**, not only
  for parseability: decimal degrees need a decimal point and allow at most six places, and a DMS
  value's seconds carry a mandatory fractional part. `to_format` and `from_decimal_degrees` emit
  conformant text — a degrees-minutes-seconds value divided by 3600 has twenty-eight places, and
  writing those out produces a coordinate the specification does not accept.
* `Validate` reports every violation with an RFC 6901 JSON Pointer using OICP's wire field names.
  Builders validate on `build()`; `build_unchecked()` is the escape hatch.

### Layers

* `transport` — the endpoint table (service × version → path, and what each operation is **for each
  role**), `PageQuery`, and `OicpError`, which
  distinguishes a transport failure, an HTTP error, a `200`-with-`Result: false` rejection, and a
  decode failure. An HTTP error names the URL that produced it, and a `404` on a base URL without
  `/api/oicp` says so — it fails every call and looks like a server problem.
* `client` — mutual TLS with a local certificate check, a streaming `crawl_evse_data`, and a
  `RetryPolicy` that will not repeat a remote start. A crawl decodes the page envelope first and
  each record on its own, so one operator's malformed record costs that record rather than the
  page — and the page walk is bounded by `totalPages` in both directions, so a `last` flag that
  contradicts the counts can neither truncate a crawl silently nor make it endless.
* `server` — `axum` routers driven by `CpoService` and `EmpService`, covering the half of OICP that
  Hubject calls. A test pins the set of operations each role *serves* against the methods on its
  trait, so "both directions, one trait each" is checked rather than claimed.
* `sync` — the `LastCall` delta engine with a watermark protocol, and `PushPlanner`, which computes
  the minimal `insert`/`update`/`delete` set so `fullLoad` is never the default. `Planner::begin`
  empties the copy when the plan is a full pull, and the committed watermark is held back by a
  configurable clock-skew guard, because `LastCall` is read by Hubject's clock and written by yours.
* `eichrecht` — `CdrCheck`, which applies the German calibration-law rules, a plausibility bound and
  the tariff check before a CDR is submitted. The rules about the signed values themselves — a
  missing `Start` or `End`, an entry with no value, and values **out of the order** the
  specification asks for, which transparency software reads as a sequence — need only the CDR and
  run whether or not an EVSE record is supplied.
* `testkit` — `MockHubject`, a brokering system in a process; validated samples; and
  `scenarios::run_all()`, the onboarding sequences as runnable checks. The broker enforces the
  rules that need session state and so cannot live in `Validate`: **one CDR per `SessionID`**, and
  a session **stopped with the medium that started it**. Both refuse with `400 Session is invalid`
  and a message naming the rule, and both are scenarios — a CDR resubmitted after a timeout is how
  a real reconciliation learns to double-count.
* `schema` — `JsonSchema` for every wire type.
* `oicp` CLI — `validate`, `id`, `cdr`, `open`, `endpoints`, `scenarios`, `errata`, `defects`,
  `schema`, `serve-mock` and `pull`. `endpoints --role cpo|emp` lists only that role's endpoints,
  with its own identifier substituted into the ones it calls and the path template shown for the
  ones it serves — the identifier in those is the peer's.

### Specification errata

Six places where Hubject's own OICP 2.3 documents contradict each other are recorded in
`types::ERRATA`, handled with `#[serde(alias)]` — reading both spellings, writing the leading
document's — and re-checked against the vendored specifications by
`cargo run -p xtask -- errata`:

* `OICP23-E001` `ChargeDetailRecord.HubProviderID` vs `HubProviderId`
* `OICP23-E002` `EvseDataRecord.ChargingStationId` vs `ChargingStationID`
* `OICP23-E003` `ChargingFacility.Power` as `Integer` vs `number`
* `OICP23-E004` `GetChargeDetailRecords.CDRForwarded` vs `CDRForwarder`
* `OICP23-E005` reservation `EMPPartnerSessionID` vs `EMPPartnerSessionId`
* `OICP23-E006` a self-referential definition of `ChargingDuration`

### Where the specification contradicts the hardware

Four places where all four Hubject documents agree and the agreed constraint is narrower than real
charging equipment are recorded in `types::SPEC_DEFECTS`, each citing the issue where a partner
reported it. The value is preserved and sent; the violation is reported with a message that names
the defect, so a partner with a 500 A charger reads "OICP 2.3 caps this at 99 A, here is the issue"
rather than concluding the library cannot count.

* `OICP23-D001` `ChargingFacility.Amperage` — two digits against a 350 kW charger's 500 A
* `OICP23-D002` `ChargingFacility.Voltage` — three digits against an 800 V architecture's 920 V
* `OICP23-D003` `SignedMeteringValues` — 3000 characters and ten entries against a long session
* `OICP23-D004` `Plug` — a closed list last extended before the Megawatt Charging System

### Verification

* `cargo run -p xtask -- all`: `no-floats`, `endpoints` (diffed against Hubject's OpenAPI),
  `errata`, and `spec-sync`. With `--upstream`, `spec-sync` asks Hubject's remotes whether the
  documents have moved since the pinned commits — which is the only way to find out, because
  Hubject edits OICP 2.3 **in place** and nothing here changes when they do. It runs weekly in CI.
* The specification's own examples decoded, validated and re-encoded byte-for-byte — including the
  ones that are themselves non-conformant, where the test asserts that `validate()` says so.
* Property tests, including the one the delta engine exists for: **applying any sequence of deltas
  leaves the same state as a full pull.**
* End-to-end sequences through `MockHubject`, over a real socket as well as in process, and
  snapshots of the JSON, the endpoint table and the errata registry.
* Every feature built on its own in CI, as well as together, and the MSRV checked on every push.
* `cargo deny` on every push, over licences, advisories, wildcards and sources.
* The `fuzz/` workspace — nested, so the root `check`, `clippy` and `test` all skip it —
  type-checked in CI, and a nightly run of all three targets from the seeded corpus.
* `cargo run -p xtask -- seed-fuzz` writes that corpus from the crate's own conformant messages and
  every identifier spelling the specification prints, derived rather than committed.
* The README's examples compiled as doctests, so the crate's front page cannot drift from it.

[Unreleased]: https://github.com/hupe1980/oicp-kit/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/hupe1980/oicp-kit/releases/tag/v0.1.0
