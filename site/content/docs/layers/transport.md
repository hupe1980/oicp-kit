+++
title = "Transport"
weight = 10
description = "The endpoint table, the page envelope, and what counts as failure."
+++

Nothing here does IO. The client and the server build on it, and so can a partner with its own HTTP
stack.

## The endpoint table

OICP versions *services*, not the protocol. `transport::Operation` is the one place that knows
which service is at which version:

```rust
use oicp_kit::transport::{HubjectEnv, Operation, PathId};

let url = Operation::PushEvseData.url(
    HubjectEnv::Prod.base_url(),
    &PathId::Operator("DE*ABC".parse()?),
)?;
assert_eq!(url, "https://service.hubject.com/api/oicp/evsepush/v23/operators/DE*ABC/data-records");
```

The identifier goes in **verbatim** — Hubject compares it to your certificate as text.

Giving the wrong *kind* of identifier is caught locally:

```rust
// PushEvseData takes an operatorID. In production this would be 017 on every request.
assert!(Operation::PushEvseData.path(&PathId::Provider("DE-DCB".parse()?)).is_err());
```

`cargo run -p xtask -- endpoints` diffs the table against Hubject's published OpenAPI documents, so
a revision shows up as a failing CI job rather than a 404.

## Which operations do you serve?

That question has no role-free answer. OICP proxies: `AuthorizeRemoteStart` is a request an **EMP
sends** to Hubject and a request **a CPO receives** from it, at the same path. So the table is asked
per role:

```rust
use oicp_kit::transport::{Involvement, Operation, Role};

assert_eq!(Operation::AuthorizeRemoteStart.involvement(Role::Emp), Some(Involvement::YouCall));
assert_eq!(Operation::AuthorizeRemoteStart.involvement(Role::Cpo), Some(Involvement::YouServe));

// A CPO has no ProviderID and never pulls EVSE data.
assert_eq!(Operation::PullEvseData.involvement(Role::Cpo), None);

let serves: Vec<_> = Operation::for_role(Role::Cpo)
    .into_iter()
    .filter(|op| op.is_served_by(Role::Cpo))
    .collect();
// AuthorizeRemoteStart, AuthorizeRemoteStop,
// AuthorizeRemoteReservationStart, AuthorizeRemoteReservationStop
```

Those four are exactly the methods on `CpoService`, and a test asserts it. The EMP's four are the
other four: `AuthorizeStart`, `AuthorizeStop`, `ChargeDetailRecord` and `ChargingNotifications`.

A second invariant follows from the first: **the role that calls an operation is the role whose
identifier the path carries.** A CPO only ever calls `{operatorID}` paths, an EMP only ever
`{providerID}` ones — which is how `oicp endpoints` knows which kind to substitute.

## Paging

Two operations are paginated — `PullEvseData` and `GetChargeDetailRecords` — with Spring Data's
query and envelope:

```rust
use oicp_kit::transport::PageQuery;

let query = PageQuery::with_size(2000);         // the spec's default is 20 and its maximum 2000
let url = query.append_to(&base_url);           // …?page=0&size=2000
```

`Page<T>::next_page()` trusts the server's `last` flag over arithmetic on the counts, because on a
data set changing under a crawl the two can disagree. `Page::validate()` reports a `last` that
would truncate a crawl.

## What counts as failure

```rust
pub enum OicpError {
    Transport { .. },   // the request never got an answer          → retry
    Http { .. },        // a non-2xx status                          → retry on 5xx
    Rejected { .. },    // HTTP 200 with Result: false               → ask the Code
    Decode { .. },      // not the message it should be              → no
    Invalid(..),        // the request we were asked to send is not conformant
    Endpoint(..),       // it could not be addressed
}
```

`Rejected` is the one that catches people out. Hubject answers a refused push with `HTTP 200` and a
body saying it did not happen; a client that only checks the status line believes the push landed.

```rust
match client.push_evse_data(&request).await {
    Err(e) if e.is_authorization_failure() => { /* tell the driver */ }
    Err(e) if e.is_retryable()             => { /* the policy already tried */ }
    Err(e) => tracing::error!(code = ?e.code(), "{e}"),
    Ok(ack) => { /* it landed */ }
}
```
