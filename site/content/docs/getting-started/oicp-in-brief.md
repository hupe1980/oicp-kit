+++
title = "OICP in brief"
weight = 10
description = "The hub-and-spoke shape, the two directions, per-service versioning, and what OICP does not have."
+++

OICP is Hubject's roaming protocol. If you know OCPI, the differences that matter are structural,
and they change how an integration is built.

## Hub and spoke, not peer to peer

OCPI parties talk to each other directly. OICP parties talk **only to Hubject**:

```
   CPO ──────► Hubject ──────► EMP
       ◄──────         ◄──────
```

There is no peer discovery, no credentials exchange, no per-partner endpoint. There is one base
URL — `https://service.hubject.com/api/oicp` in production, `https://service-qa.hubject.com/api/oicp`
for QA — and Hubject routes on the identifiers inside the messages.

That routing is worth knowing, because you can do it too:

* **The operator comes from the `EvseID`.** `DE*AB7*E840*6487` names operator `DE*AB7`. Most
  messages carry no `OperatorID` at all; Hubject derives it. So does
  [`EvseId::operator_id`](https://docs.rs/oicp-kit/latest/oicp_kit/types/struct.EvseId.html#method.operator_id).
* **The provider comes from the `EvcoID`.** `DE-DCB-C12345678-X` names provider `DE-DCB`.

## Both parties are servers

This is the part most implementations miss. Four operations are marked *"To `RECEIVE`"* in the
specification, and Hubject calls **into the partner** for them:

| Direction | Operation | Required? |
|---|---|---|
| Hubject → CPO | `AuthorizeRemoteStart` | **MANDATORY** |
| Hubject → CPO | `AuthorizeRemoteStop` | mandatory in practice |
| Hubject → CPO | `AuthorizeRemoteReservationStart` / `Stop` | optional |
| Hubject → EMP | `AuthorizeStart` / `AuthorizeStop`, forwarded from a CPO | **MANDATORY** |
| Hubject → EMP | `ChargeDetailRecord` | **MANDATORY** |
| Hubject → EMP | `ChargingNotifications` | optional |

A CPO that implements only the client half cannot be started from a driver's phone app — which is
most of what roaming is for. See [Server](@/docs/layers/server.md).

There is no discovery for these: they are URLs you type into the Hubject portal, and the first time
anyone finds out you got them wrong is when a session does not start.

## Authentication is a certificate

No API key, no OAuth, no bearer token. Authentication *is* mutual TLS, with a client certificate
Hubject issues. Authorisation is Hubject comparing the `OperatorID`/`ProviderID` **in your URL
path** against that certificate:

> *Hubject compares the given Provider- or OperatorID to the partner's SSL client certificate
> information with every web service request. […] If Hubject detects a mismatch […] Hubject will
> not perform the operation and will respond with the status code 017 "Unauthorized Access".*

That comparison is textual, which is why this crate never rewrites an identifier — see
[Identifiers](@/docs/concepts/identifiers.md) — and why
[`ClientIdentity`](https://docs.rs/oicp-kit/latest/oicp_kit/client/struct.ClientIdentity.html)
makes the same comparison locally at startup.

## `HTTP 200` does not mean success

There is no envelope. A pull returns its page directly; a command returns an
`eRoamingAcknowledgment`:

```json
{ "Result": false, "StatusCode": { "Code": "603", "Description": "Unknown EVSE ID" } }
```

…with `HTTP 200`. A client that only checks the status line believes the push landed. This crate
turns that into an `Err` — see
[`OicpError::Rejected`](https://docs.rs/oicp-kit/latest/oicp_kit/transport/enum.OicpError.html).

## Services carry their own versions

"OICP 2.3" is not one version on the wire. It is a *set* of independently versioned services:

| Service | Version in 2.3 | Changed in 2.3? |
|---|---|---|
| `evsepush` / `evsepull` (data) | `v23` | yes, from `v22` |
| `evsepush` / `evsepull` (status) | `v21` | no |
| `charging` | `v21` | no |
| `cdrmgmt` | `v22` | yes, from `v21` |
| `reservation` | `v11` | no |
| `dynamicpricing` | `v10` | no |
| `notificationmgmt` | `v11` | yes, from `v10` |

[`transport::Operation`](https://docs.rs/oicp-kit/latest/oicp_kit/transport/enum.Operation.html) is
the one place in the crate that knows which is which, and CI diffs it against Hubject's published
OpenAPI documents.

## 2.3 is the last version

The 2.3 release notes state that *"From OICP 2.3 version only REST APIs are offered"*. OICP 2.1 was
retired in April 2023 and 2.2 is superseded. In September 2025 Hubject joined the EVRoaming
Foundation as a Full Contributor and committed to native OCPI support on intercharge — so there is
no 2.4.

What *does* change is the 2.3 documents themselves, which Hubject edits **in place** without a
version bump. That is why every object in this crate carries
[`Extensions`](@/docs/concepts/extensions.md), why enums keep values they do not know, and why the
vendored specifications are pinned to specific commits.

## What to read next

* [Install](@/docs/getting-started/install.md), then
  [Your first request](@/docs/getting-started/first-request.md).
* [Identifiers](@/docs/concepts/identifiers.md) — the ISO/DIN split, and why it matters.
* [Errata](@/docs/reference/errata.md) — where Hubject's documents disagree with each other.
