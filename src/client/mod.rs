//! An async OICP client over mutual TLS: pushes, pulls, streaming crawls.
//!
//! # What is different about this client
//!
//! **`HTTP 200` is not success.** Hubject answers a refused push with a 200 and a body saying it
//! did not happen. Every method here turns that into an `Err` — see [`OicpError`].
//!
//! **The certificate is checked before the first request.** OICP authenticates with a client
//! certificate and authorises by comparing your `OperatorID`/`ProviderID` against it. A mismatch
//! is `017 Unauthorized Access` on *every* call, with nothing to say which side is wrong.
//! [`ClientIdentity::check_against`] does that comparison locally at construction.
//!
//! **Crawls stream.** A European `PullEvseData` is hundreds of thousands of records.
//! [`EmpClient::crawl_evse_data`] yields records, holding one page at a time, and a record that
//! fails to decode is reported as one bad record rather than one lost page.
//!
//! **Retries know what they are retrying.** [`RetryPolicy`] refuses to repeat a remote start,
//! because a duplicate could start a second charging session.
//!
//! ```no_run
//! # use oicp_kit::client::{CpoClient, ClientIdentity};
//! # use oicp_kit::transport::HubjectEnv;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let identity = ClientIdentity::from_pem_file("hubject-client.pem")?;
//! let client = CpoClient::builder()
//!     .environment(HubjectEnv::Qa)
//!     .operator_id("DE*ABC".parse()?)
//!     .identity(identity)
//!     .build()?;
//!
//! # let record = oicp_kit::testkit::samples::evse_data_record("DE*ABC*E1");
//! client.push_evse_data_insert(vec![record], "ABC technologies").await?;
//! # Ok(())
//! # }
//! ```

mod cpo;
mod emp;
mod http;
mod identity;
mod retry;

pub use cpo::CpoClient;
pub use emp::{CrawlError, EmpClient};
pub use http::{ClientConfig, Transport};
pub use identity::{ClientIdentity, IdentityWarning, PartyId};
pub use retry::RetryPolicy;

pub use crate::transport::{HubjectEnv, OicpError};
