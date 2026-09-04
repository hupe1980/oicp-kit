//! The types every OICP message is built from: identifiers, numbers, text, validation.
//!
//! This module is always compiled — the wire models, the client, the server and the sync engine
//! all speak it.
//!
//! # Identifiers are parsed but never rewritten
//!
//! Every OICP identifier accepts **two** grammars, ISO 15118 and DIN SPEC 91286, and Hubject
//! compares the one in your URL path against your TLS client certificate **as text**. So
//! [`EvseId`] and its siblings parse the grammar, expose the country and operator, and compare
//! case- and separator-insensitively — but re-serialise byte-identically. A library that
//! normalises `DE*ABC` to `DEABC` earns [`Code::UnauthorizedAccess`] on every request.
//!
//! ```
//! use oicp_kit::types::EvseId;
//!
//! let a: EvseId = "DE*AB7*E840*6487".parse()?;
//! let b: EvseId = "DEAB7E8406487".parse()?;
//!
//! assert_eq!(a, b);                                 // the same charging spot
//! assert_eq!(a.to_string(), "DE*AB7*E840*6487");    // …written back as it arrived
//! assert_eq!(a.operator_id(), b.operator_id());     // …and Hubject's routing rule, for free
//! # Ok::<(), oicp_kit::types::IdError>(())
//! ```
//!
//! # Energy and money are never floats
//!
//! Every OICP `number` is a [`Number`], an exact decimal. OICP *defines* `ConsumedEnergy` as
//! `MeterValueEnd - MeterValueStart`, and in `f64` that identity does not hold: `10.1 - 0.1` is
//! `10.000000000000002`. `cargo run -p xtask -- no-floats` keeps it that way.
//!
//! # Parse permissively, validate explicitly, construct strictly
//!
//! A `PullEvseData` page carries thousands of records from dozens of operators. One operator's
//! malformed `HotlinePhoneNumber` must not make the page undecodable — a roaming platform that
//! drops a page because one CPO is sloppy is worse than one that accepts it and reports the
//! problem. So:
//!
//! * `Deserialize` accepts what the peer sent, as long as it is well-typed JSON.
//! * [`Validate::validate`] reports *every* violation, each with a JSON Pointer
//!   ([RFC 6901](https://datatracker.ietf.org/doc/html/rfc6901)) using OICP's **wire** field names
//!   — `/EvseID`, `/Address/PostalCode` — not the snake-case Rust ones.
//! * Every builder's `build()` returns `Result<T, Violations>`, so an object this crate emits is
//!   conformant by construction. `build_unchecked()` exists, and says what it is.
//!
//! ```
//! use oicp_kit::types::Address;
//!
//! // Constructing strictly: OICP 2.2 and 2.3 allow only alpha-3 country codes.
//! let err = Address::builder()
//!     .country("DE")
//!     .city("Berlin")
//!     .street("EUREF CAMPUS")
//!     .postal_code("10829")
//!     .house_num("22")
//!     .build()
//!     .unwrap_err();
//! assert_eq!(err.as_slice()[0].pointer, "/Country");
//! ```
//!
//! # Open enums by default
//!
//! OICP 2.3 writes every enumerated type as a closed list, but Hubject edits the 2.3 documents
//! **in place** without a version bump, and the lists grow. Discarding a value this crate has not
//! seen would make a forwarding hub lossy — so almost every enum here keeps it in a `Custom`
//! variant, re-serialises it byte-identically, and still reports it through [`Validate`].
//!
//! The exceptions are the two where an unknown value has no safe reading: [`ActionType`], where
//! guessing could delete an operator's fleet from the roaming network, and
//! `AuthorizationStatus`, where neither "probably authorized" nor "probably not" is a decision a
//! library should make.
//!
//! Undocumented *fields* land in [`Extensions`] and are written back verbatim, for the same reason.
//!
//! # Where Hubject's own documents disagree
//!
//! OICP 2.3 is published as four documents — two leading AsciiDoc specifications and two OpenAPI
//! schema sets — and they contradict each other in six places, each of which means partners
//! implementing from different documents produce payloads that do not interoperate.
//!
//! [`ERRATA`] records every one with what breaks and which spelling this crate emits: the leading
//! document's, while accepting both on input. `cargo run -p xtask -- errata` checks that each is
//! still a contradiction upstream, so an erratum Hubject fixes fails CI rather than lingering.
//!
//! ```
//! use oicp_kit::types::ERRATA;
//!
//! for erratum in ERRATA {
//!     println!("{}: {} — {}", erratum.id, erratum.field, erratum.resolution);
//! }
//! ```

mod ack;
mod builder;
mod common;
mod datetime;
mod defects;
mod errata;
mod extensions;
mod geo;
mod identification;
mod ids;
mod number;
mod open_enum;
mod opening;
mod status;
mod text;
mod validate;

pub use ack::Acknowledgement;
pub use common::{
    Accessibility, AccessibilityLocation, ActionType, Address, AuthenticationMode,
    CalibrationLawDataAvailability, ChargingFacility, ChargingMode, DaySelection, DynamicInfoAvailable,
    EnergySource, EnergyType, EnvironmentalImpact, OpeningTimes, PaymentOption, Period, Plug, PowerType,
    ReferenceUnit, ValueAddedService,
};
pub use datetime::{DateTime, DateTimeError, HourMinute};
pub use defects::{SPEC_DEFECTS, SpecDefect};
pub use errata::{ERRATA, Erratum};
pub use extensions::Extensions;
pub use geo::{GeoCoordinates, GeoCoordinatesFormat};
pub use identification::{
    HashFunction, HashedPin, Identification, IdentificationProcess, LegacyHashData, LegacyHashFunction,
    PlugAndChargeIdentification, QrCodeIdentification, RemoteIdentification, RfidIdentification,
    RfidMifareFamilyIdentification, RfidType,
};
pub use ids::{
    ChargingPoolId, EvcoId, EvseId, IdError, IdStandard, OperatorId, ProviderId, ProviderIdOrAll, SessionId,
    Uid,
};
pub use number::Number;
pub use open_enum::UnknownVariant;
pub use opening::{Opening, UnknownReason};
pub use status::{Code, CodeArea, StatusCode};
pub use text::{InfoText, PartnerSessionId, Text, TextError};
pub use validate::{Validate, Validator, Violation, ViolationCode, Violations};

#[allow(unused_imports)]
pub(crate) use builder::strict_builder;
pub(crate) use opening::is_open_at as opening_at;
pub(crate) use validate::validate_fields;
