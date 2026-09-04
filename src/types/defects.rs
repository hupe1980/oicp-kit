//! Where OICP 2.3 contradicts the physical world, and what this crate does about it.
//!
//! [`ERRATA`](super::ERRATA) records places where Hubject's four documents disagree with *each
//! other*. This is the other kind of problem: places where all four documents agree, and the
//! agreed constraint is **narrower than real charging hardware**.
//!
//! The distinction matters because the two need opposite handling. An erratum has a right answer —
//! the leading document's — and the crate simply uses it. A defect has *no* right answer: the
//! specification says one thing, the equipment does another, and Hubject validates against the
//! specification. A CPO with a 350 kW charger is stuck between a record that describes its
//! hardware and a record Hubject will accept.
//!
//! So the crate does the only honest thing: it **reports the violation** — because Hubject will
//! reject the record, and silence would be worse — but the message says the specification is the
//! problem, names the upstream issue, and says what will happen. A partner reading
//!
//! > `/ChargingFacilities/0/Amperage: 500 is outside the allowed range 0..=99`
//!
//! reasonably concludes the library is broken. A partner reading the message this crate actually
//! produces knows to talk to Hubject.
//!
//! Every entry links to the issue on `hubject/oicp` where a partner reported it, so the claim is
//! checkable rather than an opinion.
//!
//! ```
//! use oicp_kit::types::SPEC_DEFECTS;
//!
//! for defect in SPEC_DEFECTS {
//!     println!("{} {} — {}", defect.id, defect.field, defect.upstream_issue);
//! }
//! ```

/// A place where OICP 2.3's stated constraint is narrower than real equipment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SpecDefect {
    /// A short stable identifier, e.g. `OICP23-D001`.
    pub id: &'static str,
    /// The object and field the constraint is on.
    pub field: &'static str,
    /// What OICP 2.3 says.
    pub specification_says: &'static str,
    /// What real equipment does.
    pub reality: &'static str,
    /// What happens to a partner who has such equipment.
    pub consequence: &'static str,
    /// Where a partner reported it to Hubject.
    pub upstream_issue: &'static str,
    /// What `oicp-kit` does.
    pub resolution: &'static str,
}

/// Every constraint in OICP 2.3 that this crate knows to be narrower than reality.
///
/// See the [module documentation](super#where-the-specification-contradicts-the-hardware).
pub const SPEC_DEFECTS: &[SpecDefect] = &[
    SpecDefect {
        id: "OICP23-D001",
        field: "ChargingFacility.Amperage",
        specification_says: "Integer of at most two digits — 0 to 99 A",
        reality: "A 350 kW CCS charger draws around 500 A, and liquid-cooled cables carry more. \
                  Even a 22 kW three-phase AC point is 32 A, which fits; anything DC does not.",
        consequence: "A CPO cannot describe its DC fleet accurately. Publishing the true amperage \
                      risks rejection; publishing 99 misinforms every EMP's driver app about what \
                      the charging point can deliver.",
        upstream_issue: "https://github.com/hubject/oicp/issues/153",
        resolution: "Reported as OutOfRange, with a message naming this defect so the partner knows \
                     the specification is the problem. The value itself is preserved and sent.",
    },
    SpecDefect {
        id: "OICP23-D002",
        field: "ChargingFacility.Voltage",
        specification_says: "Integer of at most three digits — 0 to 999 V",
        reality: "An 800 V vehicle architecture charges at around 920 V, and the Megawatt Charging \
                  System runs at up to 1250 V. Both exceed the cap.",
        consequence: "As above: the fastest charging points in a fleet are the ones that cannot be \
                      described.",
        upstream_issue: "https://github.com/hubject/oicp/issues/153",
        resolution: "Reported as OutOfRange, naming this defect. The value is preserved and sent.",
    },
    SpecDefect {
        id: "OICP23-D003",
        field: "ChargeDetailRecord.SignedMeteringValues[].SignedMeteringValue",
        specification_says: "At most 3000 characters, and at most ten values per CDR",
        reality: "A transparency-software blob from a high-power charging point that signs a reading \
                  every two minutes exceeds 3000 characters, and a long session produces more than \
                  ten readings.",
        consequence: "The CPO must either truncate the signed data — which destroys the signature, \
                      and with it the driver's ability to verify the bill under German calibration \
                      law — or send a record Hubject may reject.",
        upstream_issue: "https://github.com/hubject/oicp/issues/143",
        resolution: "Reported as TooLong / TooManyItems, naming this defect. The signed value is \
                     never truncated or rewritten: a mangled signature is worse than a long one.",
    },
    SpecDefect {
        id: "OICP23-D004",
        field: "Plug",
        specification_says: "A closed list of eighteen connector types, last extended before MCS",
        reality: "The Megawatt Charging System is deployed at truck charging sites and has no value \
                  in the list.",
        consequence: "A CPO with such a charging point has no conformant way to describe its \
                      connector.",
        upstream_issue: "https://github.com/hubject/oicp/issues/152",
        resolution: "Plug is an open enum: an unlisted connector is preserved verbatim in \
                     Plug::Custom and forwarded intact, so the value survives the round trip. \
                     Validate still reports it, because a peer that has not agreed on it will not \
                     understand it.",
    },
];

impl SpecDefect {
    /// Looks up a defect by its identifier.
    #[must_use]
    pub fn get(id: &str) -> Option<&'static Self> {
        SPEC_DEFECTS.iter().find(|d| d.id == id)
    }

    /// The sentence appended to a violation message for this defect.
    #[must_use]
    pub fn note(&self) -> String {
        format!(
            "this is a known defect in OICP 2.3 rather than a mistake in your data — see {} ({}); \
             the value is preserved and sent, but Hubject validates against the specification",
            self.id, self.upstream_issue
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defect_ids_are_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for defect in SPEC_DEFECTS {
            assert!(seen.insert(defect.id), "duplicate defect id {}", defect.id);
            assert!(defect.id.starts_with("OICP23-D"), "{} is not a well-formed id", defect.id);
            assert!(
                defect.upstream_issue.starts_with("https://github.com/hubject/oicp/issues/"),
                "{} does not cite a partner report",
                defect.id
            );
            assert!(!defect.consequence.is_empty());
        }
    }

    #[test]
    fn the_note_names_the_defect_and_the_issue() {
        let defect = SpecDefect::get("OICP23-D001").expect("registered");
        let note = defect.note();
        assert!(note.contains("OICP23-D001"));
        assert!(note.contains("issues/153"));
        assert!(note.contains("preserved and sent"));
    }

    #[test]
    fn lookup_by_id_works() {
        assert_eq!(SpecDefect::get("OICP23-D002").unwrap().field, "ChargingFacility.Voltage");
        assert!(SpecDefect::get("OICP23-D999").is_none());
    }
}
