+++
title = "Command line"
weight = 40
description = "The oicp binary: validate, id, cdr, open, endpoints, scenarios, errata, defects, schema, serve-mock, pull."
+++

```console
cargo install oicp-kit --features cli
```

## `oicp id` — what is this identifier?

The two grammars are easy to confuse, and a wrong one is `018 Inconsistent EvseID`.

```console
$ oicp id 'DE*AB7*E840*6487'
EvseID     DE*AB7*E840*6487
  standard ISO
  country  DE
  operator DE*AB7
  key      DEAB7E8406487

$ oicp id '+49*810*000*438'
EvseID     +49*810*000*438
  standard DIN
  country  49
  operator +49*810
  key      +49810000438
```

`key` is the form to use as a database key — never on the wire.

## `oicp validate` — is this payload conformant?

```console
$ oicp validate cdr session.json
1 violation(s):
  /ConsumedEnergy
    [inconsistent] ConsumedEnergy is 99 but MeterValueEnd - MeterValueStart is 10.0; the spec
    defines the first as the second
```

Kinds: `evse-data`, `pull-evse-data`, `push-evse-data`, `pull-evse-data-request`, `cdr`,
`notification`, `acknowledgement`. Reads standard input when no file is given.

## `oicp cdr` — will this be rejected?

Goes further than `validate`, using the charging point's own record for the rules that need it:

```console
$ oicp cdr session.json --evse charging-point.json --products 'AC 1,DC'
[error] /SignedMeteringValues: DE*ABC*E1 reports CalibrationLawDataAvailability 'External', which
        makes SignedMeteringValues mandatory on every CDR from it; …
[warning] /ConsumedEnergy: 400 kWh in 3600 s from a 22 kW charging point is above the physical
          maximum plus margin (26.4 kWh); this CDR will settle and then be disputed

1 error(s): this CDR will be rejected or disputed
```

Exits non-zero when there is an error, so it works in a submission pipeline.

## `oicp endpoints` — what do I register with Hubject?

Which endpoints those are depends on **which side you are**, so the listing asks:

```console
$ oicp endpoints --role cpo --environment prod --id 'DE*ABC'
https://service.hubject.com/api/oicp  —  as a CPO

AuthorizeStart                     you -> Hubject  …/charging/v21/operators/DE*ABC/authorize/start
AuthorizeRemoteStart               Hubject -> you  /charging/v21/providers/{providerID}/authorize-remote/start
ChargeDetailRecord                 you -> Hubject  …/cdrmgmt/v22/operators/DE*ABC/charge-detail-record
PushEvseData                       you -> Hubject  …/evsepush/v23/operators/DE*ABC/data-records
…

The 4 marked `Hubject -> you` are endpoints you serve: register their paths in the
Hubject portal and implement server::CpoService.
```

The same command as an EMP is a **different list**, with the arrows on
`AuthorizeRemoteStart` and `AuthorizeStart` the other way round:

```console
$ oicp endpoints --role emp --id 'DE-DCB'
AuthorizeStart                     Hubject -> you  /charging/v21/operators/{operatorID}/authorize/start
AuthorizeRemoteStart               you -> Hubject  …/charging/v21/providers/DE-DCB/authorize-remote/start
PullEvseData                       you -> Hubject  …/evsepull/v23/providers/DE-DCB/data-records
```

OICP proxies, so the same path is **called by one role and implemented by the other**. The paths
you call carry your own identifier; the ones you serve are shown as templates, because the
identifier in those is the peer's. A CPO's twelve endpoints and an EMP's fourteen are different
sets, not one list with different arrows.

## `oicp scenarios` — would I pass onboarding?

```console
$ oicp scenarios
PASS  cpo_publishes_and_emp_discovers
PASS  authorize_charge_settle
PASS  remote_start_from_an_app
PASS  a_refused_contract_is_reported_not_swallowed
PASS  status_updates_reach_the_emp
PASS  a_delta_crawl_converges_on_the_full_picture
PASS  a_fleet_change_needs_no_full_load
PASS  a_resubmitted_cdr_is_refused_not_settled_twice
PASS  a_session_is_stopped_with_the_medium_that_started_it
9 of 9 passed
```

## `oicp serve-mock` and `oicp pull` — a broker, and a crawl against it

```console
$ oicp serve-mock --fleet 3 --provider DE-DCB
a Hubject brokering system is listening.

  base URL   http://127.0.0.1:8080/api/oicp
  operator   DE*ABC (3 charging point(s))
  provider   DE-DCB

Point a client at it:

  oicp pull --provider DE-DCB --url http://127.0.0.1:8080/api/oicp --snapshot ./snapshot.json
```

`serve-mock` runs `MockHubject` on a socket — no certificates, no contract, no QA environment —
and prints the exact command to point at it. `pull` then crawls it with the delta engine into a
local snapshot:

```console
$ oicp pull --provider DE-DCB --url http://127.0.0.1:8080/api/oicp --snapshot ./snapshot.json
full pull — nothing has been pulled yet
  page 1 of 1
  3 inserted, 0 updated, 0 deleted
  3 record(s) in ./snapshot.json

$ oicp pull --provider DE-DCB --url http://127.0.0.1:8080/api/oicp --snapshot ./snapshot.json
delta pull — changes since 2026-09-03T20:15:10.303811Z
```

The second run is a delta, because the first left a watermark. `--rebaseline` forces a full pull.

Every OICP endpoint hangs off `/api/oicp`, and a base URL without it answers `404` on every call —
so the client's HTTP error names the URL it called and says as much.

Point `--url` at `https://service-qa.hubject.com/api/oicp` and add `--identity` once you have been
onboarded, and the same command talks to the real brokering system.

## `oicp errata` — where do the documents disagree?

Prints the six recorded contradictions with what breaks and what this crate emits. See
[Errata](@/docs/reference/errata.md).
