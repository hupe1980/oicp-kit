//! German calibration law (Eichrecht), and the CDR pre-flight built on it.
//!
//! # What the law asks for
//!
//! Since 2019, energy sold at a public charging point in Germany must be measured by a calibrated
//! meter and the measurement must be **independently verifiable by the driver**. In OICP that
//! travels in-band on the CDR: [`SignedMeteringValue`](crate::cpo::SignedMeteringValue) carries
//! the meter's own signed readings, and
//! [`CalibrationLawVerificationInfo`](crate::cpo::CalibrationLawVerificationInfo) carries what a
//! driver needs to check them — the certificate id, the public key, and a URL to the compiled
//! data for the invoice.
//!
//! This crate does not verify signatures: that is what transparency software is for, and the
//! formats (OCMF, EDL40, Alfen) are outside OICP. What it does is make sure the data **arrives**
//! and **survives**, byte for byte, and that a CDR which is required to carry it actually does.
//!
//! # The pre-flight
//!
//! Hubject validates a CDR when it is submitted, and an EMP validates it again when it is billed —
//! by which time the session is over, the driver has left, and a rejected CDR is a written-off
//! sale. Several of the rules that get CDRs rejected cannot be checked from the CDR alone; they
//! need the EVSE's data record too.
//!
//! [`CdrCheck`] is those rules, run locally, before submission:
//!
//! ```
//! # use oicp_kit::eichrecht::CdrCheck;
//! # use oicp_kit::testkit::samples;
//! # use oicp_kit::types::CalibrationLawDataAvailability;
//! let mut evse = samples::evse_data_record("DE*ABC*E1");
//! evse.calibration_law_data_availability = CalibrationLawDataAvailability::External;
//! let cdr = samples::charge_detail_record("DE*ABC*E1", samples::session_id());
//!
//! // The EVSE promises externally-provided calibration data; the CDR carries none.
//! let findings = CdrCheck::new().against_evse(&evse).run(&cdr);
//! assert!(!findings.is_empty());
//! assert!(findings.iter().any(|f| f.message.contains("SignedMeteringValues")));
//! ```

mod check;

pub use check::{CdrCheck, Finding, Severity};
