+++
title = "Errata"
weight = 10
description = "Six places Hubject's own OICP 2.3 documents contradict each other, and what this crate does about each."
+++

OICP 2.3 is published as four documents that are supposed to describe one protocol:

| Document | Role |
|---|---|
| `OICP 2.3 CPO` / `OICP 2.3 EMP` AsciiDoc | the **leading** specification — Hubject says so in the release notes |
| `oicp-cpo-2.3-api-doc` / `oicp-emp-2.3-api-doc` OpenAPI | the machine-readable schemas partners generate clients from |

They do not agree. Each conflict below was found by diffing the four sources against each other,
and each means that partners implementing from different documents produce payloads that do not
interoperate.

For every one, `oicp-kit` **writes the leading document's form and reads both**. The registry is
available as data (`types::ERRATA`), from the CLI (`oicp errata`), and — importantly —
`cargo run -p xtask -- errata` checks that each is *still* a conflict upstream, so an erratum
Hubject fixes fails CI rather than lingering as a stale claim.

## `OICP23-E001` — `ChargeDetailRecord.HubProviderID`

* **Leading document:** `HubProviderID`, in both the CPO and EMP data-type tables and every code
  snippet.
* **OpenAPI:** `HubProviderId` in the EMP schema — whose own example still says `HubProviderID` —
  and `HubProviderID` in the CPO schema.
* **Impact:** a CDR routed through a hub loses its hub-provider attribution if the reader matches
  the other spelling. That is a billing attribution, not a cosmetic field.

## `OICP23-E002` — `EvseDataRecord.ChargingStationId`

* **Leading document:** `ChargingStationId`, in the CPO data-type table.
* **OpenAPI:** `ChargingStationId` in the schema, but `ChargingStationID` in **every example**
  Hubject publishes, including the `PushEvseData` example and the CPO code snippets.
* **Impact:** a CPO that copied the example publishes a station id no EMP reading the schema will
  find, so charge points do not group into stations on the EMP's map.

## `OICP23-E003` — `ChargingFacility.Power`

* **Leading document:** `Integer`, mandatory, at most three digits — so 0 to 999 kW.
* **OpenAPI:** `integer` 0–999 in the CPO schema; an unconstrained `number` in the EMP schema.
* **Impact:** a CPO publishing a 22.5 kW facility is conformant or not depending on which document
  its partner read, and a strict integer parser rejects the whole record.
* **What this crate does:** decodes as an exact decimal, so `22.5` arrives and round-trips;
  `validate()` reports the deviation with the erratum id rather than refusing the record.

## `OICP23-E004` — `GetChargeDetailRecords.CDRForwarded`

* **Leading document:** `CDRForwarded`, in the EMP services table.
* **OpenAPI:** `CDRForwarder` as the property name, while the example *in the same file* says
  `CDRForwarded`.
* **Impact:** the filter is silently ignored by a peer expecting the other spelling, so an EMP
  reconciling CDRs gets the unfiltered set back and double-counts.

## `OICP23-E005` — reservation `EMPPartnerSessionID`

* **Leading document:** `EMPPartnerSessionID`, consistent across every other message in both roles.
* **OpenAPI:** `EMPPartnerSessionId` in the reservation schemas of *both* documents, while their own
  examples say `EMPPartnerSessionID`.
* **Impact:** an EMP loses its own session correlation id on reservations only — precisely where it
  is needed, to match a reservation to the session that follows.

## `OICP23-E006` — `ChargingNotificationProgress.ChargingDuration`

* **CPO document:** `Charging Duration = EventOccurred - ChargingStart`, in milliseconds.
* **EMP document:** *"Charging Duration = EventOccurred - Charging Duration"* — a self-referential
  definition that cannot be implemented.
* **Impact:** none on the wire — the field is an integer either way — but an EMP implementing the
  EMP document literally has no definition to implement.
* **What this crate does:** implements and checks the CPO document's definition.
  `ChargingNotificationProgress::implied_duration_ms()` computes it, and `validate()` reports a
  stated duration that contradicts the timestamps by more than a minute of clock skew.

## Related: examples that are not conformant

Not errata, but worth knowing when you read Hubject's documents:

* The `Identification` examples fill in **all five** members at once. No real payload does; a real
  one carries exactly one. This crate models it as a Rust enum and takes the first member in spec
  order.
* The `GeoCoordinates` examples fill in all three notations at once, for the same reason.
* The `eRoamingPullEvseData` example sets `LastCall` **and** `CountryCodes` **and** `OperatorIds`,
  which the same document forbids two paragraphs earlier. `tests/wire.rs` decodes that example and
  asserts that `validate()` reports it.
