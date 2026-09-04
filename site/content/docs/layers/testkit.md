+++
title = "Testkit"
weight = 60
description = "A brokering system in a process, validated samples, and the onboarding scenarios."
+++

Going live on OICP means signing a contract with Hubject, having certificates issued by their CA,
and getting access to their QA environment. That is weeks of calendar time before the first
request — and the things that go wrong are not single messages but **sequences**.

## MockHubject

```rust
use oicp_kit::testkit::{MockHubject, MockEmp, samples};
use oicp_kit::types::Code;

let mut hubject = MockHubject::new();
hubject.register_emp(MockEmp::permissive("DE-DCB".parse()?));
hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())?;

let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
let session = response.session_id.clone().unwrap();

hubject.submit_cdr(&samples::charge_detail_record("DE*ABC*E1", session))?;

// A CDR for a session the broker never opened is refused, exactly as the real broker refuses it.
let invented = samples::charge_detail_record("DE*ABC*E1", samples::session_id());
assert_eq!(*hubject.submit_cdr(&invented).unwrap_err().code(), Code::SessionIsInvalid);
```

It routes the way the real broker does — deriving the operator from the `EvseID` and the provider
from the `EvcoID` — tracks sessions, paginates accurately, converts coordinates into the notation
the pull asked for, and answers with the specification's own status codes.

The last assertion above is the commonest integration failure there is: a CPO that invents its own
session ids finds every CDR refused.

### The rules that need session state

Two of OICP's rules are about a *session*, not a message, so no `validate()` can see them — the
broker is the only thing that knows what opened the session. Both are enforced, and both refuse
with `400 Session is invalid` and a message naming the rule:

* **One CDR per `SessionID`.** *"Hubject will accept only one CDR per SessionID."* This is the rule
  a retry meets: the first submission lands, its answer is lost, the CPO sends the record again.
  A broker that accepts both teaches a reconciliation to count acknowledgements rather than
  sessions, which is how a session's revenue gets counted twice.
* **A session is stopped with the medium that started it.** *"the session `MUST` only be stopped
  with the same medium, which was used for starting the session."* The comparison is narrow on
  purpose — contracts against contracts, cards against cards, and silence when one side names a
  contract and the other a bare RFID UID, because Hubject can resolve one to the other and this
  broker cannot. A simulator stricter than the thing it simulates sends you chasing bugs that are
  not there.

### Scripting an EMP

```rust
hubject.register_emp(MockEmp::with("DE-DCB".parse()?, |identification| {
    match identification.uid().map(|u| u.canonical()).as_deref() {
        Some("7568290FFF765F") => AuthorizationDecision::Authorized,
        _ => AuthorizationDecision::Refused(Code::NoValidContract),
    }
}));
```

### Asserting on what happened

```rust
assert!(matches!(hubject.events().last(), Some(Event::CdrSubmitted { .. })));
assert!(hubject.sessions().iter().all(|s| s.settled));
```

## Samples

Every sample passes `validate()`, checked by `tests/wire.rs` so it cannot rot. They are drawn from
Hubject's own OpenAPI examples, corrected where those examples are themselves non-conformant —
several fill in all five `Identification` members at once, which no real payload does.

```rust
samples::evse_data_record("DE*ABC*E1");
samples::pull_evse_data_record("DE*ABC*E1");   // with the operator derived, as Hubject does it
samples::charge_detail_record("DE*ABC*E1", samples::session_id());
samples::fleet("DE*ABC", 2000);                // for exercising a crawl
```

## Onboarding scenarios

Hubject's onboarding ends with an integration test: a series of scenarios run against their QA
environment with an engineer watching. Failing one costs another round.

```rust
let report = scenarios::run_all();
assert!(report.passed(), "{report}");
```

```
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

Passing them does not make the paid test unnecessary — only Hubject can certify — but it means the
first time you see these sequences is not the day they are being marked.
