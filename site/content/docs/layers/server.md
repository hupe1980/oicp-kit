+++
title = "Server"
weight = 30
description = "The half Hubject calls — the half most implementations skip."
+++

OICP looks like a client protocol until you read the CPO document carefully. Four operations are
marked *"To `RECEIVE`"*, and one is *"Implementation: `MANDATORY`"*.

A CPO that has not implemented `AuthorizeRemoteStart` cannot be started from a driver's phone app,
which is most of what an EMP's customers do. There is no discovery: the endpoints are URLs you type
into the Hubject portal, and the first time anyone notices is when a session does not start.

## One trait, no defaults

```rust
use oicp_kit::cpo::*;
use oicp_kit::server::{CpoService, cpo_router};
use oicp_kit::types::{Acknowledgement, Code};

impl CpoService for MyCpo {
    async fn authorize_remote_start(&self, request: AuthorizeRemoteStartRequest) -> Acknowledgement {
        match self.start(&request.evse_id).await {
            Ok(())  => Acknowledgement::success().with_session(request.session_id),
            Err(NoCar) => Acknowledgement::failure(Code::NoEvConnectedToEvse),
            Err(_)  => Acknowledgement::failure(Code::CommunicationToEvseFailed),
        }
    }
    async fn authorize_remote_stop(&self, request: AuthorizeRemoteStopRequest) -> Acknowledgement { … }
    async fn reservation_start(&self, _: AuthorizeRemoteReservationStartRequest) -> Acknowledgement {
        Acknowledgement::failure(Code::ServiceNotAvailable)
    }
    async fn reservation_stop(&self, _: AuthorizeRemoteReservationStopRequest) -> Acknowledgement {
        Acknowledgement::failure(Code::ServiceNotAvailable)
    }
}

let app = cpo_router(Arc::new(my_cpo));
```

The trait has **no default methods**, deliberately. A CPO that has not decided what to do about
reservations should say `ServiceNotAvailable` out loud, in code someone can read, rather than
inherit a default that quietly answers something.

Only the two `authorize_remote_*` methods are mandatory in the specification. Refusing the
reservation ones honestly is conformant; answering them with success when nothing happened is not,
and it strands a driver who was told their bay was held.

## The EMP side

`EmpService` has the same shape: `authorize_start`, `authorize_stop`, `charge_detail_record`,
`charging_notification`.

`authorize_start` is answered while a driver stands at a charging point. Two rules:

* **Answer quickly.** Hubject reports a timeout to the CPO as `310 Partner did not respond`, and
  the driver is refused.
* **Say why.** `AuthorizationStartResponse::not_authorized(Code::NoValidContract)` fills both the
  status and the code consistently; a refusal claiming `000 Success` is unreadable, and
  `validate()` reports it.

## Everything answers `HTTP 200`

Including refusals. That is what OICP does — a `4xx` here is a protocol error that Hubject reports
as a *failed delivery* rather than as a refusal, and it will retry.

## Non-conformant requests

```rust
fn on_invalid_request(&self, violations: &Violations) -> Option<Acknowledgement> {
    None   // accept it anyway
}
```

The default refuses with `022 Data error`. See
[Parse, validate, construct](@/docs/concepts/parse-validate-construct.md) for when to override it.

## The paths cannot drift

The router's routes come from `transport::Operation`, and CI diffs that table against Hubject's
OpenAPI. `oicp endpoints --id 'DE*ABC'` prints exactly what to register in the portal.
