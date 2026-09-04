//! Repository automation for `oicp-kit`.
//!
//! ```console
//! cargo run -p xtask -- no-floats     # no f32/f64 anywhere but the JSON boundary
//! cargo run -p xtask -- endpoints     # the endpoint table matches the vendored OpenAPI
//! cargo run -p xtask -- errata        # every recorded erratum still exists upstream
//! cargo run -p xtask -- seed-fuzz     # write conformant seeds into the fuzz corpus
//! cargo run -p xtask -- spec-sync     # the vendored specs match their pinned commits
//! cargo run -p xtask -- spec-sync --upstream   # …and Hubject has not moved them since
//! cargo run -p xtask -- all           # everything above
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The Hubject repositories this crate is written against, and the commits it was checked at.
///
/// Hubject edits the OICP 2.3 documents **in place**, with no version bump, so a pin is the only
/// way to know whether the specification moved. `spec-sync` compares the local checkouts against
/// these, and with `--upstream` asks the remotes what they hold now.
const SPEC_PINS: &[(&str, &str, &str)] = &[
    ("oicp", "https://github.com/hubject/oicp", "119c5d5655694f89a013ab0ff1266a76299c2895"),
    ("oicp-cpo-2.3-api-doc", "https://github.com/hubject/oicp-cpo-2.3-api-doc", "f5a8e2f"),
    ("oicp-emp-2.3-api-doc", "https://github.com/hubject/oicp-emp-2.3-api-doc", "9a773b3"),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = args.first().cloned().unwrap_or_else(|| "all".to_owned());
    let upstream = args.iter().any(|a| a == "--upstream");
    let root = repository_root();

    let results = match task.as_str() {
        "no-floats" => vec![no_floats(&root)],
        "endpoints" => vec![endpoints(&root)],
        "errata" => vec![errata(&root)],
        "seed-fuzz" => vec![seed_fuzz(&root)],
        "spec-sync" => vec![spec_sync(&root, upstream)],
        "all" => vec![no_floats(&root), endpoints(&root), errata(&root), spec_sync(&root, upstream)],
        other => {
            eprintln!("unknown task {other:?}; try: no-floats, endpoints, errata, spec-sync, seed-fuzz, all");
            eprintln!("       spec-sync and all also take --upstream, which asks Hubject's remotes");
            return ExitCode::FAILURE;
        }
    };

    let mut failed = false;
    for result in results {
        match result {
            Ok(message) => println!("ok    {message}"),
            Err(Failure::Skipped(message)) => println!("skip  {message}"),
            Err(Failure::Failed(message)) => {
                eprintln!("FAIL  {message}");
                failed = true;
            }
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

enum Failure {
    /// The check could not run — the vendored specs are not present, for instance.
    Skipped(String),
    /// The check ran and did not pass.
    Failed(String),
}

type Check = Result<String, Failure>;

fn repository_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `<root>/xtask`.
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask has a parent").to_path_buf()
}

// --- no floats ------------------------------------------------------------------------------

/// Energy and money in OICP end up on an invoice, so no `f32`/`f64` may appear anywhere in the
/// library — not as a field, not as a cast, and not as a call.
///
/// This is a scan rather than a lint because `clippy::disallowed_types` fires on the `visit_f64`
/// that serde's derive generates for every struct, which says nothing about the code anyone wrote.
///
/// It matches the **substring**, not the word, so `s.serialize_f64(..)` and `base.mul_f32(..)` are
/// visible to it — those calls are the floating-point operations, and a word-boundary match cannot
/// see any of them.
fn no_floats(root: &Path) -> Check {
    // The JSON boundary. OICP sends numbers as JSON numbers, and `serde_json` represents a
    // fractional one as an `f64` unless its `arbitrary_precision` feature is on — which changes
    // `serde_json::Value` for every crate in the build, so this crate does not impose it.
    // `types::Number` is therefore the one place a float is touched, it is exact for every value
    // OICP carries, and `Number::json_round_trips` reports the values where it would not be.
    const EXEMPT: &[&str] = &["src/types/number.rs"];

    let mut offenders = String::new();
    for path in rust_files(&root.join("src")) {
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        if EXEMPT.contains(&relative.as_str()) {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap_or_default();
        for (number, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if code.contains("f32") || code.contains("f64") {
                let _ = writeln!(offenders, "  {relative}:{}: {}", number + 1, line.trim());
            }
        }
    }

    if offenders.is_empty() {
        Ok(format!("no-floats: no f32/f64 anywhere in src/ except {} — the JSON boundary", EXEMPT.join(", ")))
    } else {
        Err(Failure::Failed(format!(
            "no-floats: energy and money in OICP end up on an invoice, and a binary float cannot \
             represent 0.10. Use `types::Number`.\n{offenders}"
        )))
    }
}

fn rust_files(directory: &Path) -> Vec<PathBuf> {
    let mut found = vec![];
    let Ok(entries) = std::fs::read_dir(directory) else { return found };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
    found.sort();
    found
}

// --- endpoints ------------------------------------------------------------------------------

/// The endpoint table in `src/transport/endpoint.rs` must list exactly the paths Hubject's OpenAPI
/// documents define. A Hubject revision then shows up as a failing job rather than a 404.
fn endpoints(root: &Path) -> Check {
    let specs = root.join("specs");
    if !specs.is_dir() {
        return Err(Failure::Skipped("endpoints: specs/ is not present (it is gitignored)".into()));
    }

    let mut declared: Vec<String> = vec![];
    for document in ["oicp-cpo-2.3-api-doc", "oicp-emp-2.3-api-doc"] {
        let path = specs.join(document).join("src/openapi.yaml");
        let Ok(yaml) = std::fs::read_to_string(&path) else {
            return Err(Failure::Skipped(format!("endpoints: {} is not present", path.display())));
        };
        for line in yaml.lines() {
            // Paths are the two-space-indented keys that start with a slash.
            if let Some(rest) = line.strip_prefix("  /") {
                if let Some(path) = rest.strip_suffix(':') {
                    declared.push(format!("/{path}"));
                }
            }
        }
    }
    declared.sort();
    declared.dedup();

    let table = std::fs::read_to_string(root.join("src/transport/endpoint.rs")).unwrap_or_default();

    let mut missing = vec![];
    for path in &declared {
        // The EMP document writes the remote-stop path with `{externalID}` where the CPO document
        // writes `{providerID}`; they are the same endpoint, and the table uses the CPO spelling.
        let normalised = path.replace("{externalID}", "{providerID}");
        if !table.contains(&format!("\"{normalised}\"")) {
            missing.push(normalised);
        }
    }

    if missing.is_empty() {
        Ok(format!("endpoints: all {} paths in Hubject's OpenAPI are in the table", declared.len()))
    } else {
        Err(Failure::Failed(format!(
            "endpoints: Hubject's OpenAPI defines paths the table does not have:\n{}",
            missing.iter().map(|p| format!("  {p}\n")).collect::<String>()
        )))
    }
}

// --- errata ---------------------------------------------------------------------------------

/// Every erratum recorded in `src/types/errata.rs` must still be a real disagreement between
/// Hubject's documents. One that Hubject fixes should be removed, not left as a stale claim.
fn errata(root: &Path) -> Check {
    let specs = root.join("specs");
    if !specs.is_dir() {
        return Err(Failure::Skipped("errata: specs/ is not present (it is gitignored)".into()));
    }

    let read = |relative: &str| std::fs::read_to_string(specs.join(relative)).unwrap_or_default();
    let cpo_schemas = specs.join("oicp-cpo-2.3-api-doc/src/schemas");
    let emp_schemas = specs.join("oicp-emp-2.3-api-doc/src/schemas");
    if !cpo_schemas.is_dir() || !emp_schemas.is_dir() {
        return Err(Failure::Skipped("errata: the OpenAPI schema directories are not present".into()));
    }

    let mut stale = vec![];

    // E001: the EMP CDR schema writes HubProviderId; the CPO schema writes HubProviderID.
    let emp_cdr = read("oicp-emp-2.3-api-doc/src/schemas/eRoamingChargeDetailRecord.yaml");
    let cpo_cdr = read("oicp-cpo-2.3-api-doc/src/schemas/eRoamingChargeDetailRecord.yaml");
    if !(emp_cdr.contains("  HubProviderId:") && cpo_cdr.contains("  HubProviderID:")) {
        stale.push("OICP23-E001 (HubProviderID/HubProviderId) is no longer a disagreement");
    }

    // E002: the schema says ChargingStationId; the PushEvseData example says ChargingStationID.
    let evse_record = read("oicp-cpo-2.3-api-doc/src/schemas/EvseDataRecord.yaml");
    let push_example = read("oicp-cpo-2.3-api-doc/src/schemas/eRoamingPushEvseData.yaml");
    if !(evse_record.contains("  ChargingStationId:") && push_example.contains("ChargingStationID:")) {
        stale.push("OICP23-E002 (ChargingStationId/ChargingStationID) is no longer a disagreement");
    }

    // E003: the CPO schema types Power as an integer; the EMP schema as a number.
    let cpo_facility = read("oicp-cpo-2.3-api-doc/src/schemas/ChargingFacility.yaml");
    let emp_facility = read("oicp-emp-2.3-api-doc/src/schemas/ChargingFacility.yaml");
    let typed_as = |yaml: &str| {
        yaml.split("  Power:").nth(1).and_then(|rest| rest.lines().nth(1).map(str::trim).map(str::to_owned))
    };
    if typed_as(&cpo_facility) == typed_as(&emp_facility) {
        stale.push("OICP23-E003 (ChargingFacility.Power integer vs number) is no longer a disagreement");
    }

    // E004: the property is CDRForwarder; the example in the same file says CDRForwarded.
    let get_cdrs = read("oicp-emp-2.3-api-doc/src/schemas/eRoamingGetChargeDetailRecords.yaml");
    if !(get_cdrs.contains("  CDRForwarder:") && get_cdrs.contains("CDRForwarded:")) {
        stale.push("OICP23-E004 (CDRForwarded/CDRForwarder) is no longer a disagreement");
    }

    // E005: the reservation schemas write EMPPartnerSessionId.
    let reservation = read("oicp-emp-2.3-api-doc/src/schemas/eRoamingAuthorizeRemoteReservationStart.yaml");
    if !reservation.contains("  EMPPartnerSessionId:") {
        stale.push("OICP23-E005 (EMPPartnerSessionID/EMPPartnerSessionId) is no longer a disagreement");
    }

    // E006: the EMP document defines ChargingDuration in terms of itself.
    let emp_progress = read("oicp-emp-2.3-api-doc/src/schemas/eRoamingChargingNotificationProgress.yaml");
    if !emp_progress.contains("EventOccurred - Charging Duration") {
        stale.push("OICP23-E006 (self-referential ChargingDuration) is no longer a disagreement");
    }

    if stale.is_empty() {
        Ok("errata: every recorded erratum still exists in the vendored specs".into())
    } else {
        Err(Failure::Failed(format!(
            "errata: Hubject appears to have fixed something. Remove the erratum and its serde \
             alias, and update tests/errata.rs:\n{}",
            stale.iter().map(|s| format!("  {s}\n")).collect::<String>()
        )))
    }
}

// --- spec sync ------------------------------------------------------------------------------

/// The vendored specs must be at the commits the crate was written against.
///
/// Run on a schedule in CI: a Hubject edit then arrives as a reviewable PR rather than as a
/// difference between what the code says and what the protocol does.
fn spec_sync(root: &Path, upstream: bool) -> Check {
    let specs = root.join("specs");
    let mut drifted = vec![];

    // Half one: the working copy is what this crate was written against.
    if specs.is_dir() {
        for (directory, url, pin) in SPEC_PINS {
            let path = specs.join(directory);
            if !path.is_dir() {
                drifted.push(format!("{directory} is missing; clone it from {url}"));
                continue;
            }
            match git(&["-C", &path.to_string_lossy(), "rev-parse", "HEAD"]) {
                Some(head) if head.starts_with(pin) => {}
                Some(head) => drifted.push(format!("the local {directory} is at {head}, pinned at {pin}")),
                None => drifted.push(format!("{directory} is not a git checkout; clone it from {url}")),
            }
        }
    } else if !upstream {
        return Err(Failure::Skipped("spec-sync: specs/ is not present (it is gitignored)".into()));
    }

    // Half two, and the one that matters: Hubject edits these documents in place. Nothing local
    // changes when they do, so a check that only reads the working copy can never see it. This
    // asks the remotes — no clone, one ref lookup each — and is what the scheduled job runs.
    if upstream {
        for (directory, url, pin) in SPEC_PINS {
            match git(&["ls-remote", url, "HEAD"]) {
                Some(line) => {
                    let head = line.split_whitespace().next().unwrap_or_default();
                    if !head.starts_with(pin) {
                        drifted.push(format!(
                            "{directory} upstream is at {head}, pinned at {pin} — Hubject has \
                             edited the specification in place"
                        ));
                    }
                }
                None => {
                    return Err(Failure::Skipped(format!(
                        "spec-sync: {url} could not be reached, so upstream was not checked"
                    )));
                }
            }
        }
    }

    if drifted.is_empty() {
        let scope = if upstream { "and their upstreams" } else { "locally" };
        Ok(format!("spec-sync: all {} vendored specs are at their pinned commits {scope}", SPEC_PINS.len()))
    } else {
        Err(Failure::Failed(format!(
            "spec-sync: the specifications have moved. Review the diff, update the crate and the \
             pins in xtask, then update the errata registry if a disagreement was fixed:\n{}",
            drifted.iter().map(|s| format!("  {s}\n")).collect::<String>()
        )))
    }
}

// --- seed-fuzz ------------------------------------------------------------------------------

/// Writes conformant messages into `fuzz/corpus/`, so a fuzzing run starts from the shapes OICP
/// actually carries rather than from random bytes.
///
/// A fuzzer that has to discover `{"EvseID":"DE*ABC*E1",…}` byte by byte spends its whole budget
/// reaching the first line of the decoder. Seeded with real messages, it spends it on the mutations
/// of them — which is where the interesting inputs are.
///
/// The seeds are **derived, not committed**: `fuzz/corpus/` is gitignored, and a corpus checked in
/// beside a wire model it no longer matches is worse than none. Re-run this after changing a type.
fn seed_fuzz(root: &Path) -> Check {
    use oicp_kit::testkit::samples;

    let corpus = root.join("fuzz/corpus");
    let mut written = 0_usize;

    let mut write = |target: &str, name: &str, bytes: &[u8]| -> Result<(), Failure> {
        let directory = corpus.join(target);
        std::fs::create_dir_all(&directory).map_err(|e| {
            Failure::Failed(format!("seed-fuzz: {} could not be created: {e}", directory.display()))
        })?;
        std::fs::write(directory.join(name), bytes)
            .map_err(|e| Failure::Failed(format!("seed-fuzz: {name} could not be written: {e}")))?;
        written += 1;
        Ok(())
    };

    // `wire`: one conformant message per shape the target decodes.
    let session = samples::session_id();
    let messages: Vec<(&str, String)> = vec![
        ("evse_data_record", to_json(&samples::evse_data_record("DE*ABC*E1"))),
        ("pull_evse_data_record", to_json(&samples::pull_evse_data_record("DE*ABC*E1"))),
        ("charge_detail_record", to_json(&samples::charge_detail_record("DE*ABC*E1", session.clone()))),
        (
            "charging_notification_start",
            to_json(&samples::charging_notification_start("DE*ABC*E1", session.clone())),
        ),
        ("authorize_start_request", to_json(&samples::authorize_start_request("DE*ABC*E1"))),
        ("authorize_stop_request", to_json(&samples::authorize_stop_request("DE*ABC*E1", session))),
        ("acknowledgement", to_json(&oicp_kit::types::Acknowledgement::success())),
    ];
    for (name, json) in &messages {
        write("wire", &format!("{name}.json"), json.as_bytes())?;
    }

    // `identifiers`: every spelling the specification prints, which is where the two grammars and
    // their optional separators meet.
    for (index, text) in [
        "DE*AB7*E840*6487",
        "DEAB7E8406487",
        "DE*XYZ*ETEST1",
        "+49*810*000*438",
        "49*810*000*438",
        "DE-8EO-CAet5e4XY-3",
        "DE8EOCAet5e43X1",
        "DE*8EO*Aet5e4*3",
        "DE-8EO-Aet5e4-3",
        "DE8EOAet5e43",
        "DE*A36",
        "DEA36",
        "+49*536",
        "DE8EO",
        "DE-8EO",
        "DE*8EO",
        "IT*123*P456*AB789",
        "b2688855-7f00-0002-6d8e-48d883f6abb6",
        "7568290FFF765F",
        "*",
        "",
    ]
    .iter()
    .enumerate()
    {
        write("identifiers", &format!("id-{index:02}"), text.as_bytes())?;
    }

    // `delta`: pages that exercise each `deltaType`, plus the shapes a generator does not think of
    // — the same record twice, and a delete for something that was never inserted.
    let one = samples::pull_evse_data_record("DE*ABC*E1");
    let two = samples::pull_evse_data_record("DE*ABC*E2");
    let mut deleted = one.clone();
    deleted.delta_type = Some(oicp_kit::cpo::DeltaType::Delete);
    let mut updated = two.clone();
    updated.delta_type = Some(oicp_kit::cpo::DeltaType::Update);
    let mut inserted = two.clone();
    inserted.delta_type = Some(oicp_kit::cpo::DeltaType::Insert);

    for (name, page) in [
        ("full-pull", vec![one.clone(), two.clone()]),
        ("insert-update-delete", vec![inserted, updated, deleted.clone()]),
        ("the-same-record-twice", vec![one.clone(), one.clone()]),
        ("a-delete-for-nothing", vec![deleted]),
        ("empty", vec![]),
    ] {
        write("delta", &format!("{name}.json"), to_json(&page).as_bytes())?;
    }

    Ok(format!("seed-fuzz: wrote {written} seeds into {}", corpus.display()))
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("a sample always encodes")
}

/// Runs `git` and returns its trimmed standard output, or `None` if it could not run.
fn git(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_matching_does_not_fire_on_identifiers_that_merely_contain_the_needle() {
        assert!(contains_word("let x: f64 = 1.0;", "f64"));
        assert!(contains_word("(f64)", "f64"));
        assert!(!contains_word("visit_f64", "f64"));
        assert!(!contains_word("my_f64_thing", "f64"));
        assert!(!contains_word("Inf64", "f64"));
    }
}
