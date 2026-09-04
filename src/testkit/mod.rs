//! A conformant brokering system and a set of valid objects, so you can test before you onboard.
//!
//! # Why this is the feature that matters
//!
//! Going live on OICP means signing a contract with Hubject, having certificates issued by their
//! CA, and getting access to their QA environment. That is weeks of calendar time before the first
//! request. And the things that go wrong are not single messages — they are *sequences*: a CDR for
//! a session the broker never opened, a remote start at a charge point published with
//! `IsHubjectCompatible: false`, a delta crawl that loses a page.
//!
//! [`MockHubject`] is those sequences, in a process, with no network:
//!
//! ```
//! # use oicp_kit::testkit::{MockHubject, MockEmp, samples};
//! # use oicp_kit::types::{Code, Validate};
//! let mut hubject = MockHubject::new();
//! hubject.register_emp(MockEmp::permissive("DE-DCB".parse().unwrap()));
//! hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E1").into()).unwrap();
//!
//! let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
//! let session = response.session_id.clone().unwrap();
//!
//! // A CDR for a session the broker did open settles…
//! let cdr = samples::charge_detail_record("DE*ABC*E1", session);
//! assert!(hubject.submit_cdr(&cdr).is_ok());
//!
//! // …and one for a session it did not is refused, exactly as the real broker refuses it.
//! let invented = samples::charge_detail_record("DE*ABC*E1", samples::session_id());
//! assert_eq!(*hubject.submit_cdr(&invented).unwrap_err().code(), Code::SessionIsInvalid);
//! ```
//!
//! [`scenarios`] packages the sequences Hubject walks partners through during onboarding, so you
//! can arrive at the paid integration test having already passed it.

pub mod samples;
pub mod scenarios;

mod mock;

#[cfg(feature = "server")]
mod server;

pub use mock::{AuthorizationDecision, Event, MockEmp, MockHubject, MockSession};

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(all(feature = "testkit", feature = "server"))))]
pub use server::MockHubjectServer;
