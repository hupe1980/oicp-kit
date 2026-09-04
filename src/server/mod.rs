//! The half of OICP that Hubject calls: an `axum` router driven by one trait per role.
//!
//! # The half everyone forgets
//!
//! OICP looks like a client protocol until you read the CPO document carefully. Four of its
//! operations are marked *"To `RECEIVE`"*, and one of them —
//! [`AuthorizeRemoteStart`](crate::cpo::AuthorizeRemoteStartRequest) — is
//! *"Implementation: `MANDATORY`"*. A CPO that has not implemented it cannot be started from a
//! driver's phone app, which is most of what an EMP's customers do.
//!
//! There is no discovery: the endpoints are URLs you give Hubject in their portal, and the first
//! time anyone finds out you got them wrong is when a driver's session does not start.
//!
//! So this module inverts the usual shape. You implement [`CpoService`] — a trait with four
//! methods, all of them mandatory — and [`cpo_router`] gives you an `axum` router with every path
//! wired to it, at the paths [`Operation`](crate::transport::Operation) says. If it compiles, your
//! Hubject-facing surface is complete.
//!
//! ```no_run
//! use oicp_kit::cpo::*;
//! use oicp_kit::server::{CpoService, cpo_router};
//! use oicp_kit::types::{Acknowledgement, Code};
//!
//! struct MyCpo;
//!
//! impl CpoService for MyCpo {
//!     async fn authorize_remote_start(&self, request: AuthorizeRemoteStartRequest) -> Acknowledgement {
//!         // …tell the charging point to start…
//!         Acknowledgement::success().with_session(request.session_id)
//!     }
//!     async fn authorize_remote_stop(&self, request: AuthorizeRemoteStopRequest) -> Acknowledgement {
//!         Acknowledgement::success().with_session(request.session_id)
//!     }
//!     async fn reservation_start(&self, _: AuthorizeRemoteReservationStartRequest) -> Acknowledgement {
//!         // Reservations are optional in the spec — say so honestly rather than lying with a 200.
//!         Acknowledgement::failure(Code::ServiceNotAvailable)
//!     }
//!     async fn reservation_stop(&self, _: AuthorizeRemoteReservationStopRequest) -> Acknowledgement {
//!         Acknowledgement::failure(Code::ServiceNotAvailable)
//!     }
//! }
//!
//! # async fn serve() -> Result<(), Box<dyn std::error::Error>> {
//! let app = cpo_router(std::sync::Arc::new(MyCpo));
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:8443").await?;
//! axum::serve(listener, app).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Answering honestly
//!
//! Every handler returns an [`Acknowledgement`](crate::types::Acknowledgement), and the router
//! sends it with `HTTP 200` whether it says success or failure — because that is what OICP does.
//! A refusal is `Result: false` and a [`Code`](crate::types::Code) saying why. Returning
//! `Acknowledgement::success()` from an operation you have not implemented is the one thing that
//! is worse than not implementing it: the EMP believes the session started.

mod cpo;
mod emp;

pub use cpo::{CpoService, cpo_router};
pub use emp::{EmpService, emp_router};
