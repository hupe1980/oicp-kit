# oicp-kit

[![CI](https://github.com/hupe1980/oicp-kit/actions/workflows/ci.yml/badge.svg)](https://github.com/hupe1980/oicp-kit/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/oicp-kit.svg)](https://crates.io/crates/oicp-kit)
[![docs.rs](https://docs.rs/oicp-kit/badge.svg)](https://docs.rs/oicp-kit)
[![license](https://img.shields.io/crates/l/oicp-kit.svg)](#license)

A Rust toolkit for [OICP](https://github.com/hubject/oicp) (Open InterCharge Protocol), the
protocol that carries EV roaming traffic between Charge Point Operators (CPO) and e-Mobility
Providers (EMP) through the **Hubject** brokering system.

Unlike OCPI, which is peer-to-peer, OICP is hub-and-spoke: every partner talks only to Hubject,
over mutual TLS — and Hubject calls back into the partner for the reverse direction. Both halves
are needed to go live, and this crate ships both, along with the pieces that are hard to get right:
an **EVSE delta-sync engine**, a **CDR pre-flight**, and **MockHubject** — a complete brokering
system in a process, so you can integrate before you have onboarded.

```console
cargo add oicp-kit
```

## The five properties that decide quality

**Energy and money are never floats.** OICP *defines* `ConsumedEnergy` as `MeterValueEnd -
MeterValueStart`, and every one of those numbers ends up on an invoice between two companies. In
`f64`, `10.1 - 0.1` is `10.000000000000002`; here it is `10.0`, because every OICP number is an
exact decimal (`rust_decimal::Decimal` behind `types::Number`) and every sum, difference and
comparison in the crate is decimal arithmetic. `cargo run -p xtask -- no-floats` fails CI on an
`f32` or `f64` **anywhere in `src/`**, with one exemption it prints by name: the JSON boundary in
`types::Number`, exact for every value OICP carries and reported by `Number::json_round_trips`
where it would not be.

**Identifiers are parsed, and the wire form survives.** Every OICP identifier accepts *two*
grammars — ISO 15118 and DIN SPEC 91286 — and Hubject compares the one in your URL against your
TLS client certificate **as text**, answering a mismatch with `017 Unauthorized Access`.

```rust
use oicp_kit::types::EvseId;

let a: EvseId = "DE*AB7*E840*6487".parse()?;   // ISO, separated
let b: EvseId = "DEAB7E8406487".parse()?;      // ISO, packed

assert_eq!(a, b);                              // the same charging spot…
assert_eq!(a.to_string(), "DE*AB7*E840*6487"); // …each written back exactly as it arrived
assert_eq!(a.operator_id(), b.operator_id());  // …and Hubject's own routing rule, for free
# Ok::<(), oicp_kit::types::IdError>(())
```

**Parsing and conformance are separate questions.** A `PullEvseData` page carries thousands of
records from dozens of operators. One operator's malformed `HotlinePhoneNumber` cannot make the
page undecodable — the value arrives, and `Validate::validate` reports it with an RFC 6901 JSON
Pointer into the JSON the peer actually sent:

```rust
# use oicp_kit::testkit::samples;
# let mut value = serde_json::to_value(samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
# value["HotlinePhoneNumber"] = serde_json::json!("call us");
# let json = serde_json::to_string(&value).unwrap();
use oicp_kit::emp::PullEvseDataRecord;
use oicp_kit::types::Validate;

let record: PullEvseDataRecord = serde_json::from_str(&json)?;   // permissive
for violation in record.validate().unwrap_err().iter() {
    println!("{} [{}]: {}", violation.pointer, violation.code, violation.message);
    // /HotlinePhoneNumber [pattern_mismatch]: "call us" is not a '+' followed by 5 to 15 digits
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

The rule throughout is **parse permissively, validate explicitly, construct strictly** — every
builder's `build()` returns `Result<T, Violations>`, so an object this crate emits is conformant by
construction. `build_unchecked()` exists, and says what it is.

"Conformant" is stricter than "it parses". A field whose *notation* has a grammar is checked
against that grammar: `GeoCoordinates` has one regular expression per notation, so `"52"` is not a
decimal degree (OICP has no notation for a whole number of degrees) and `"9°21'39''"` is not a
DMS coordinate (the seconds' fractional part is not optional). Both read fine as numbers.

One malformed *record* does not cost the page either. A crawl decodes the page envelope first and
each record on its own, so a `PullEvseData` over a European data set loses one charging point
rather than every charging point after it — and says which:

```rust
# use oicp_kit::client::CrawlError;
# use oicp_kit::emp::PullEvseDataRecord;
# fn store(_: PullEvseDataRecord) {}
# fn handle(item: Result<PullEvseDataRecord, CrawlError>) {
match item {                                  // …for each item the crawl yields
    Ok(record) => store(record),
    Err(error) => eprintln!("{error}"),       // record 812 on page 39, and the crawl goes on
}
# }
```

**Nothing a peer sent is thrown away.** OICP 2.3 is a *terminal version* that Hubject nonetheless
edits **in place**, with no version bump. Undocumented fields land in `types::Extensions` and are
written back verbatim; an enum value this crate has never seen keeps its text in a `Custom` variant
and is still reported by `validate()`. A hub built on this crate forwards next year's field intact.

**Both directions, one trait each.** Four OICP operations are marked *"To RECEIVE"* in the CPO
document, and one of them is *"Implementation: MANDATORY"*. Most implementations ship the client
half and leave Hubject's callbacks as an exercise — so a driver's phone app cannot start a session.
Here the CPO role is one client and one trait, and if it compiles, your Hubject-facing surface is
complete.

## What is in the box

| Layer | Feature | What it gives you |
|---|---|---|
| `types` | *(always)* | ID grammars (ISO + DIN), `Number`, `Extensions`, RFC 6901 validation, the errata registry |
| `cpo` | `cpo` | the CPO half of the OICP 2.3 wire model — pushes, authorization, CDRs, notifications, pricing |
| `emp` | `emp` | the EMP half — pulls, pagination, CDR retrieval, authentication data |
| `transport` | `transport` | the endpoint table (service × version → path), page envelope, error mapping |
| `client` | `client` | async client over `reqwest` with mutual TLS, streaming crawls, safe retries |
| `server` | `server` | `axum` router per role, driven by one trait — the Hubject-facing half |
| `sync` | `sync` | the `LastCall` delta engine, and the minimal-push planner |
| `eichrecht` | `eichrecht` | typed calibration-law data, and CDR pre-flight |
| `testkit` | `testkit` | samples, `MockHubject`, and the onboarding scenarios as runnable checks |
| `schema` | `schema` | `JsonSchema` for every wire type |
| `oicp` CLI | `cli` | `validate`, `id`, `cdr`, `open`, `endpoints`, `scenarios`, `errata`, `defects`, `schema`, `serve-mock`, `pull` |

Default features are `cpo`, `emp` and `transport`. `full` turns on everything except the CLI.

## MockHubject: integrate before you onboard

Testing against the real brokering system needs a signed contract, certificates issued by
Hubject's CA and access to their QA environment — weeks before the first request. And the failures
that cost time are *sequences*, not single messages.

```rust
use oicp_kit::testkit::{MockHubject, MockEmp, samples};
use oicp_kit::types::Code;

let mut hubject = MockHubject::new();
hubject.register_emp(MockEmp::permissive("DE-DCB".parse()?));
hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())?;

// A driver swipes a card; the broker routes it to the EMP that owns the contract.
let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
let session = response.session_id.clone().unwrap();

// A CDR for a session the broker opened settles…
assert!(hubject.submit_cdr(&samples::charge_detail_record("DE*ABC*E1", session)).is_ok());

// …and one for a session it did not is refused, exactly as the real broker refuses it.
let invented = samples::charge_detail_record("DE*ABC*E1", samples::session_id());
assert_eq!(*hubject.submit_cdr(&invented).unwrap_err().code(), Code::SessionIsInvalid);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The broker also enforces the two rules that need **session state**, which no `validate()` can see:

* **One CDR per `SessionID`.** A submission that times out gets resubmitted; the real broker
  refuses the second. A mock that accepts both teaches a reconciliation to count acknowledgements
  instead of sessions, and that is how a session's revenue gets counted twice.
* **A session is stopped with the medium that started it** — *"the session `MUST` only be stopped
  with the same medium, which was used for starting the session"*.

`testkit::scenarios::run_all()` packages the sequences Hubject walks partners through at
onboarding, both of these among them, so the paid integration test is not the first time you see
them.

## The delta engine

Every EMP rebuilds the same fragile logic: full pull, then periodic `LastCall` deltas, applying
`insert`/`update`/`delete`. Five rules are easy to get wrong and expensive to get wrong *quietly*:

1. **`LastCall` is exclusive with the filters.** A delta scoped to a country silently omits charge
   points that moved *out* of it, and those stale records live forever.
2. **A delete is a tombstone.** Applying it as an upsert leaves a corrupted half-record behind.
3. **The watermark advances on success, not on request.** Move it early and a failed page's changes
   are lost permanently — the next delta starts after them.
4. **Deltas expire.** After a long outage a delta cannot be trusted and the copy must be rebuilt.
5. **`LastCall` is read by Hubject's clock and written by yours.** A machine running a minute fast
   asks for changes since an instant Hubject has not reached, and everything in that minute is
   never sent again. The watermark is therefore committed with a skew guard — five minutes by
   default — which costs a small overlap the engine applies idempotently.

`sync::Planner` encodes all five, and a property test proves the thing that matters:

> applying **any** sequence of deltas leaves the same state as a full pull.

```rust
# use oicp_kit::sync::{InMemoryEvseRepository, Planner, PlannerConfig};
# use oicp_kit::types::GeoCoordinatesFormat;
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let mut repository = InMemoryEvseRepository::new();
# let planner = Planner::new(PlannerConfig::new("DE-DCB".parse()?, GeoCoordinatesFormat::Google));
// `begin` decides the pull *and* empties the copy when the answer is a full one — a full pull is
// the whole world, so what is not in it has been withdrawn.
let (plan, watermark) = planner.begin(&mut repository)?;
// …crawl every page of plan.request() and apply it…
planner.commit(&mut repository, watermark)?;   // only now
# Ok(())
# }
```

The CPO has the mirror problem, with a sharper edge: `ActionType::FullLoad` **replaces** everything
Hubject holds for the operator, so a nightly job with one wrong filter withdraws a fleet from the
roaming network. `sync::PushPlanner` computes the minimal `insert`/`update`/`delete` set, and
`fullLoad` is a separately named method that says what it does.

## CDR pre-flight

Hubject rejects an inconsistent CDR after the fact, when the session is long over and the sale is
written off. Several of the reasons cannot be seen from the CDR alone:

```rust
# use oicp_kit::testkit::samples;
# use oicp_kit::types::CalibrationLawDataAvailability;
# let mut evse = samples::evse_data_record("DE*ABC*E1");
# evse.calibration_law_data_availability = CalibrationLawDataAvailability::External;
# let cdr = samples::charge_detail_record("DE*ABC*E1", samples::session_id());
use oicp_kit::eichrecht::CdrCheck;

let findings = CdrCheck::new()
    .against_evse(&evse)                        // the calibration-law and plausibility rules
    .with_known_products(["AC 1", "DC"])        // the tariff the EMP will be billed under
    .run(&cdr);

for finding in &findings {
    println!("{finding}");
    // [error] /SignedMeteringValues: DE*ABC*E1 reports CalibrationLawDataAvailability 'External',
    //         which makes SignedMeteringValues mandatory on every CDR from it; …
}
```

## Where Hubject's own documents disagree

OICP 2.3 is published as four documents that describe one protocol, and they do not agree. Each
conflict is recorded in `types::ERRATA` with what breaks and which spelling this crate emits — and
`cargo run -p xtask -- errata` checks that each is *still* a conflict upstream, so an erratum
Hubject fixes fails CI rather than lingering as a stale claim.

| Id | Field | The disagreement |
|---|---|---|
| `OICP23-E001` | `ChargeDetailRecord.HubProviderID` | the EMP schema says `HubProviderId`; everything else says `HubProviderID` |
| `OICP23-E002` | `EvseDataRecord.ChargingStationId` | the schema says `…Id`; every published example says `…ID` |
| `OICP23-E003` | `ChargingFacility.Power` | `Integer` in the leading document and the CPO schema; unconstrained `number` in the EMP schema |
| `OICP23-E004` | `GetChargeDetailRecords.CDRForwarded` | the property is `CDRForwarder`; the leading document and the file's own example say `CDRForwarded` |
| `OICP23-E005` | reservation `EMPPartnerSessionID` | the reservation schemas say `…Id`; every other message says `…ID` |
| `OICP23-E006` | `ChargingNotificationProgress.ChargingDuration` | the EMP document defines it in terms of itself |

`oicp-kit` reads both spellings and writes the leading document's.

```console
$ oicp errata
```

## Command line

```console
$ oicp id 'DE*AB7*E840*6487'
EvseID     DE*AB7*E840*6487
  standard ISO
  country  DE
  operator DE*AB7
  key      DEAB7E8406487

$ oicp validate cdr session.json
1 violation(s):
  /ConsumedEnergy
    [inconsistent] ConsumedEnergy is 99 but MeterValueEnd - MeterValueStart is 10.0; …

$ oicp cdr session.json --evse charging-point.json --products 'AC 1,DC'
$ oicp endpoints --role cpo --environment prod --id 'DE*ABC'
$ oicp defects                       # where OICP 2.3 is narrower than real hardware
$ oicp scenarios                     # the onboarding sequences, against an in-process broker
$ oicp serve-mock                    # …or the same broker on a socket
$ oicp pull --provider DE-DCB --url http://127.0.0.1:8080/api/oicp --snapshot ./snapshot.json
```

`serve-mock` prints the exact `pull` command to run against it. The `/api/oicp` is not decoration:
every OICP endpoint hangs off it, and a base URL without it answers `404` on every call — which is
why the client's HTTP error says so rather than just reporting the status.

## Versions

**OICP 2.3 is the version to target, and the last one.** The release notes state that *"From OICP
2.3 version only REST APIs are offered"*; 2.1 was retired in April 2023 and 2.2 is superseded. In
September 2025 Hubject joined the EVRoaming Foundation as a Full Contributor and committed to
native OCPI support on intercharge, so there is no 2.4 — only in-place maintenance of 2.3, which is
exactly what `Extensions` and the open enums are for.

OICP versions **services**, not the protocol: a "2.3 implementation" is `evsepush/v23` *and*
`charging/v21` *and* `cdrmgmt/v22` *and* `reservation/v11` *and* `dynamicpricing/v10` *and*
`notificationmgmt/v11`. `transport::Operation` is the single place that knows which is which, and
CI diffs it against Hubject's published OpenAPI documents.

## Related crates

* [ocpp-kit](https://github.com/hupe1980/ocpp-kit) — OCPP, the station ↔ CSMS protocol.
* [ocpi-kit](https://github.com/hupe1980/ocpi-kit) — OCPI, peer-to-peer roaming.

## Contributing

`cargo test --all-features` and `cargo run -p xtask -- all` must pass, and every feature must
build on its own — `--all-features` is exactly the build that cannot tell whether a feature
declares what it needs.

The `endpoints`, `errata` and `spec-sync` checks read the Hubject specifications from `specs/`
(gitignored, because they are third-party publications); without them they skip rather than fail.

```console
git clone https://github.com/hubject/oicp specs/oicp
git clone https://github.com/hubject/oicp-cpo-2.3-api-doc specs/oicp-cpo-2.3-api-doc
git clone https://github.com/hubject/oicp-emp-2.3-api-doc specs/oicp-emp-2.3-api-doc
```

`cargo run -p xtask -- spec-sync --upstream` asks Hubject's remotes whether the documents have
moved since the pinned commits — no clone and no local `specs/` needed. It runs weekly in CI,
because Hubject edits the specification **in place** and nothing here changes when they do.

`cargo run -p xtask -- seed-fuzz` writes the fuzz corpus from this crate's own conformant messages
and every identifier spelling the specification prints. The seeds are derived rather than
committed, so re-run it after changing a wire type.

```console
cargo run -p xtask -- seed-fuzz
cargo +nightly fuzz run wire        # …and identifiers, and delta
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

OICP is a protocol owned and maintained by [Hubject GmbH](https://www.hubject.com/). This project
is not affiliated with Hubject. The specifications are published under CC BY-SA 4.0 and are not
redistributed here.
