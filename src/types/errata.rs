//! Where the OICP 2.3 documents contradict themselves, and what this crate does about it.
//!
//! OICP 2.3 is published as four documents that are supposed to describe one protocol:
//!
//! | Document | Role |
//! |---|---|
//! | `OICP 2.3 CPO` / `OICP 2.3 EMP` AsciiDoc | the **leading** specification — Hubject says so in the release notes |
//! | `oicp-cpo-2.3-api-doc` / `oicp-emp-2.3-api-doc` OpenAPI | the machine-readable schemas partners generate clients from |
//!
//! They do not agree. Each disagreement below is a field where two of Hubject's own documents give
//! different names or different types for the same thing — which means partners implementing from
//! different documents produce payloads that do not interoperate. Every one was found by diffing
//! the four sources against each other, and every one is checked by `tests/errata.rs` against the
//! vendored specs.
//!
//! # What the crate does
//!
//! For each item the crate picks the **canonical** form — the leading AsciiDoc document, since
//! Hubject designates it as leading — and accepts the other spelling on input via `#[serde(alias)]`.
//! So `oicp-kit` reads what any partner sends and writes what the leading document specifies.
//! [`ERRATA`] is that list as data, so a partner can render it into their own integration notes.
//!
//! ```
//! use oicp_kit::types::ERRATA;
//!
//! for item in ERRATA {
//!     println!("{}: {} — {}", item.id, item.field, item.resolution);
//! }
//! ```

/// One place where Hubject's own documents disagree with each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Erratum {
    /// A short stable identifier, e.g. `OICP23-E001`.
    pub id: &'static str,
    /// The object and field the disagreement is about.
    pub field: &'static str,
    /// What the leading AsciiDoc specification says.
    pub leading_document: &'static str,
    /// What the OpenAPI schema says.
    pub openapi_document: &'static str,
    /// What breaks if you pick the wrong one.
    pub impact: &'static str,
    /// What `oicp-kit` does.
    pub resolution: &'static str,
}

/// Every disagreement this crate knows about between Hubject's OICP 2.3 documents.
///
/// See [the errata section](crate::types#where-hubjects-own-documents-disagree).
pub const ERRATA: &[Erratum] = &[
    Erratum {
        id: "OICP23-E001",
        field: "eRoamingChargeDetailRecord.HubProviderID",
        leading_document: "HubProviderID — both the CPO and EMP data-type tables, and every code snippet",
        openapi_document: "HubProviderId in the EMP OpenAPI schema (whose own example still says HubProviderID); \
                           HubProviderID in the CPO OpenAPI schema",
        impact: "A CDR routed through a hub loses its hub-provider attribution if the reader matches \
                 the other spelling — which is a billing attribution, not a cosmetic field.",
        resolution: "Writes HubProviderID; accepts HubProviderId on input.",
    },
    Erratum {
        id: "OICP23-E002",
        field: "EvseDataRecord.ChargingStationId",
        leading_document: "ChargingStationId — the CPO data-type table",
        openapi_document: "ChargingStationId in the schema, but ChargingStationID in every example \
                           Hubject publishes, including the PushEvseData example and the CPO code snippets",
        impact: "A CPO that copied the example publishes a station id no EMP reading the schema will find, \
                 so charge points do not group into stations on the EMP's map.",
        resolution: "Writes ChargingStationId; accepts ChargingStationID on input.",
    },
    Erratum {
        id: "OICP23-E003",
        field: "ChargingFacility.Power",
        leading_document: "Integer, mandatory, maximum 3 digits (0..=999 kW)",
        openapi_document: "integer 0..=999 in the CPO schema; unconstrained number in the EMP schema",
        impact: "A CPO publishing a 22.5 kW facility is either conformant or not depending on which \
                 document its partner read; a strict integer parser rejects the whole record.",
        resolution: "Decodes as an exact decimal, so 22.5 arrives; Validate reports a non-integer or \
                     out-of-range value rather than refusing the record.",
    },
    Erratum {
        id: "OICP23-E004",
        field: "eRoamingGetChargeDetailRecords.CDRForwarded",
        leading_document: "CDRForwarded — the EMP services table",
        openapi_document: "CDRForwarder as the property name, while the example in the same file \
                           says CDRForwarded",
        impact: "The filter is silently ignored by a peer expecting the other spelling, so an EMP \
                 reconciling CDRs gets the unfiltered set back and double-counts.",
        resolution: "Writes CDRForwarded; accepts CDRForwarder on input.",
    },
    Erratum {
        id: "OICP23-E005",
        field: "eRoamingAuthorizeRemoteReservationStart/Stop.EMPPartnerSessionID",
        leading_document: "EMPPartnerSessionID — consistent across every other message in both roles",
        openapi_document: "EMPPartnerSessionId in the reservation schemas of both the CPO and EMP \
                           OpenAPI documents, while their own examples say EMPPartnerSessionID",
        impact: "An EMP loses its own session correlation id on reservations only, which is precisely \
                 where it is needed to match a reservation to the session that follows.",
        resolution: "Writes EMPPartnerSessionID; accepts EMPPartnerSessionId on input.",
    },
    Erratum {
        id: "OICP23-E006",
        field: "eRoamingChargingNotificationProgress.ChargingDuration",
        leading_document: "Charging Duration = EventOccurred - ChargingStart, in milliseconds (CPO document)",
        openapi_document: "\"Charging Duration = EventOccurred - Charging Duration\" in the EMP document — \
                           a self-referential definition that cannot be implemented",
        impact: "None on the wire — the field is an integer either way — but an EMP implementing the \
                 EMP document literally has no definition to implement.",
        resolution: "Documents and checks the CPO document's definition: EventOccurred - ChargingStart.",
    },
];

impl Erratum {
    /// Looks up an erratum by its identifier.
    #[must_use]
    pub fn get(id: &str) -> Option<&'static Self> {
        ERRATA.iter().find(|e| e.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errata_ids_are_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for item in ERRATA {
            assert!(seen.insert(item.id), "duplicate erratum id {}", item.id);
            assert!(item.id.starts_with("OICP23-E"), "{} is not a well-formed id", item.id);
            assert!(!item.resolution.is_empty());
        }
    }

    #[test]
    fn lookup_by_id_works() {
        assert_eq!(Erratum::get("OICP23-E001").unwrap().field, "eRoamingChargeDetailRecord.HubProviderID");
        assert!(Erratum::get("OICP23-E999").is_none());
    }
}
