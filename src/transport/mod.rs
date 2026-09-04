//! Addressing, paging and error mapping: what sits between the wire model and a socket.
//!
//! Nothing here does IO. [`client`](crate::client) and [`server`](crate::server) build on it, and
//! so can a partner with its own HTTP stack: the endpoint table, the query parameters and the
//! error mapping are useful without `reqwest`.
//!
//! # What OICP does not have
//!
//! No envelope, no version negotiation, no registration handshake, no tokens. A request is a JSON
//! body POSTed over mutual TLS to a versioned path; a response is either an
//! [`Acknowledgement`](crate::types::Acknowledgement) or, for the two big pulls, a
//! [`Page`](crate::emp::Page). That is the whole transport. The subtlety is in three places, and
//! this module holds all three:
//!
//! * **[Which path](Operation)** — services carry their own versions, independent of "2.3".
//! * **[Which identifier](PathId)** — Hubject matches it against your TLS certificate, as text.
//! * **[What counts as failure](OicpError)** — `HTTP 200` with `Result: false` is a failure.

mod endpoint;
mod error;
mod query;

pub use endpoint::{EndpointError, EndpointInfo, HubjectEnv, Involvement, Operation, PathId, Role};
pub use error::{OicpError, Result};
pub use query::PageQuery;
