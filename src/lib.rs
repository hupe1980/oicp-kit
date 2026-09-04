//! A Rust toolkit for [OICP](https://github.com/hubject/oicp) (Open InterCharge Protocol), the
//! roaming protocol that connects Charge Point Operators and e-Mobility Providers through the
//! Hubject brokering system.
//!
//! Unlike OCPI, which is peer-to-peer, OICP is **hub-and-spoke**: every partner talks only to
//! Hubject, over mutual TLS, and Hubject calls back into the partner for the reverse direction.
//! Both halves are needed to go live, and this crate ships both.
//!
//! # What is here
//!
//! | Layer | Feature | What it gives you |
//! |---|---|---|
//! | [`types`] | *(always)* | identifiers with both ISO and DIN grammars, `Number`, `Extensions`, RFC 6901 validation |
//! | [`cpo`] | `cpo` | the CPO half of the OICP 2.3 wire model |
//! | [`emp`] | `emp` | the EMP half of the OICP 2.3 wire model |
//! | [`transport`] | `transport` | the endpoint table, page envelope and error mapping |
//! | [`client`] | `client` | an async client with mutual TLS, paginated crawls and delta pulls |
//! | [`server`] | `server` | an `axum` router per role — the Hubject-facing half |
//! | [`sync`] | `sync` | the `LastCall` delta engine, and the minimal-push planner |
//! | [`eichrecht`] | `eichrecht` | typed calibration-law data, and CDR pre-flight |
//! | [`testkit`] | `testkit` | samples and `MockHubject` — a broker to test against, offline |
//!
//! # Five properties worth knowing about
//!
//! **Energy and money are never floats.** Every `number` in every message is a [`types::Number`],
//! an exact decimal, and every sum, difference and comparison in this crate is decimal arithmetic
//! — so OICP's own rule, that `ConsumedEnergy` is the difference between `MeterValueEnd` and
//! `MeterValueStart`, holds exactly. `cargo run -p xtask -- no-floats` fails the build on an
//! `f32` or `f64` anywhere in `src/`, with one exemption it names: the JSON boundary in
//! `types::Number` itself, which is exact for every value OICP carries and reports the rest
//! through [`types::Number::json_round_trips`].
//!
//! **Identifiers are parsed, and the wire form survives.** [`types::EvseId`] and its siblings
//! understand both the ISO 15118 and DIN SPEC 91286 grammars, expose the country and operator, and
//! compare case-, separator- and `+`-insensitively — but re-serialise byte-identically. Hubject
//! matches identifiers against your TLS certificate as text; a library that normalises them breaks
//! production with status code `017`.
//!
//! **Parsing and conformance are separate questions.** A `PullEvseData` page carries thousands of
//! records from dozens of operators. One malformed field cannot make the page undecodable: the
//! value arrives, and [`types::Validate::validate`] reports it with a JSON Pointer. One malformed
//! *record* cannot cost the page either — [`client::EmpClient::crawl_evse_data`] decodes the
//! envelope first and each record on its own, so a crawl loses one charging point rather than
//! every charging point after it.
//!
//! **Nothing a peer sent is thrown away.** OICP 2.3 is edited in place, without version bumps.
//! Undocumented fields land in [`types::Extensions`] and are written back verbatim; an enum value
//! this crate has never seen keeps its text in a `Custom` variant.
//!
//! **Both directions, one trait each.** Most OICP implementations ship the client half and leave
//! Hubject's callbacks as an exercise. Here the CPO role is [`client::CpoClient`] plus
//! [`server::CpoService`]; implement the trait and your Hubject-facing surface is complete.
//!
//! # Getting started
//!
#![cfg_attr(feature = "cpo", doc = "```rust")]
#![cfg_attr(not(feature = "cpo"), doc = "```rust,ignore")]
//! use oicp_kit::types::{EvseId, Validate};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Two ways of writing the same charging spot.
//! let iso: EvseId = "DE*AB7*E840*6487".parse()?;
//! let packed: EvseId = "DEAB7E8406487".parse()?;
//!
//! assert_eq!(iso, packed);                          // the same EVSE…
//! assert_eq!(iso.to_string(), "DE*AB7*E840*6487");  // …each written back as it arrived
//! assert_eq!(iso.operator_id(), packed.operator_id());
//! # Ok(())
//! # }
//! ```
//!
//! # Spec traceability
//!
//! Public items carry the name of the OICP 2.3 message or type they implement, so a reviewer — or
//! a partner's compliance team — can go from a Rust type to the document that defines it. Where
//! Hubject's own documents disagree with one another, [`types::ERRATA`] records the conflict, what
//! breaks, and which spelling this crate emits.
//!
//! OICP is a protocol owned and maintained by [Hubject GmbH](https://www.hubject.com/). This
//! project is not affiliated with Hubject.

#![cfg_attr(docsrs, feature(doc_cfg))]

/// The README's examples, compiled as doctests.
///
/// The front page of a crate is the part most people read and the part nothing checks. This makes
/// `cargo test --doc` check it: an example that stops compiling is a failing test rather than a
/// paragraph that quietly stopped being true.
// The examples reach across the whole crate, so they are checked on the feature set
// that has all of it.
#[cfg(all(doctest, feature = "full"))]
#[doc = include_str!("../README.md")]
pub struct ReadmeExamples;

pub mod types;

#[cfg(feature = "cpo")]
#[cfg_attr(docsrs, doc(cfg(feature = "cpo")))]
pub mod cpo;

#[cfg(feature = "emp")]
#[cfg_attr(docsrs, doc(cfg(feature = "emp")))]
pub mod emp;

#[cfg(feature = "transport")]
#[cfg_attr(docsrs, doc(cfg(feature = "transport")))]
pub mod transport;

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub mod client;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub mod server;

#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub mod sync;

#[cfg(feature = "eichrecht")]
#[cfg_attr(docsrs, doc(cfg(feature = "eichrecht")))]
pub mod eichrecht;

#[cfg(feature = "testkit")]
#[cfg_attr(docsrs, doc(cfg(feature = "testkit")))]
pub mod testkit;

/// The version of OICP this crate implements.
///
/// OICP 2.3 is the last version: in September 2025 Hubject committed to native OCPI support on
/// intercharge, so there is no 2.4. What does change is the 2.3 documents themselves, which
/// Hubject edits in place — which is why [`types::Extensions`] and the open enums exist.
pub const OICP_VERSION: &str = "2.3";
