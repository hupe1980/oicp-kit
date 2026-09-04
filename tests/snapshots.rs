//! Snapshots of the JSON this crate emits.
//!
//! These exist so a change to the wire format shows up as a reviewable diff. OICP field names are
//! irregular — `EvseID` but `lastUpdate`, `deltaType` but `DynamicInfoAvailable` — and a rename
//! that looks like a tidy-up in Rust is a broken integration on the wire.

use std::fmt::Write as _;

use oicp_kit::cpo::{ChargingNotification, EvseStatus};
use oicp_kit::testkit::samples;
use oicp_kit::transport::{Involvement, Operation, PathId};
use oicp_kit::types::{Acknowledgement, Code};

/// Serialises with keys in insertion order, so the snapshot shows the wire order.
fn json(value: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(value).expect("encodes")
}

#[test]
fn evse_data_record() {
    insta::assert_snapshot!(json(&samples::evse_data_record("DE*ABC*E1")));
}

#[test]
fn pull_evse_data_record() {
    insta::assert_snapshot!(json(&samples::pull_evse_data_record("DE*ABC*E1")));
}

#[test]
fn charge_detail_record() {
    insta::assert_snapshot!(json(&samples::charge_detail_record("DE*ABC*E1", samples::session_id())));
}

#[test]
fn authorize_start_request() {
    insta::assert_snapshot!(json(&samples::authorize_start_request("DE*ABC*E1")));
}

#[test]
fn charging_notification_start() {
    insta::assert_snapshot!(json(&ChargingNotification::Start(samples::charging_notification_start(
        "DE*ABC*E1",
        samples::session_id()
    ))));
}

#[test]
fn evse_status_record() {
    insta::assert_snapshot!(json(&samples::evse_status_record("DE*ABC*E1", EvseStatus::Available)));
}

#[test]
fn acknowledgements() {
    insta::assert_snapshot!(json(&Acknowledgement::success()));
    insta::assert_snapshot!(json(&Acknowledgement::failure(Code::UnknownEvseId)));
}

#[test]
fn the_endpoint_table() {
    // The paths a partner registers with Hubject. A change here is a change to the integration.
    let mut rendered = String::new();
    for operation in Operation::ALL {
        let info = operation.info();
        let _ = writeln!(
            rendered,
            "{:<34} {:<16} {:<5} {:<8} {:<8} {:<4} {}",
            format!("{operation:?}"),
            info.service,
            info.version,
            info.cpo.map_or("-", |i| if i == Involvement::YouCall { "calls" } else { "serves" }),
            info.emp.map_or("-", |i| if i == Involvement::YouCall { "calls" } else { "serves" }),
            if info.paginated { "page" } else { "ack" },
            info.path_template,
        );
    }
    insta::assert_snapshot!(rendered);
}

#[test]
fn urls_against_the_two_hubject_environments() {
    use oicp_kit::transport::HubjectEnv;

    let operator = PathId::Operator("DE*ABC".parse().unwrap());
    let provider = PathId::Provider("DE-DCB".parse().unwrap());
    let mut rendered = String::new();
    for environment in [HubjectEnv::Qa, HubjectEnv::Prod] {
        for operation in Operation::ALL {
            let id = if operation.takes_operator_id() { &operator } else { &provider };
            if let Ok(url) = operation.url(environment.base_url(), id) {
                let _ = writeln!(rendered, "{url}");
            }
        }
    }
    insta::assert_snapshot!(rendered);
}

#[test]
fn the_errata_registry() {
    let mut rendered = String::new();
    for erratum in oicp_kit::types::ERRATA {
        let _ = writeln!(rendered, "{}  {}\n    {}", erratum.id, erratum.field, erratum.resolution);
    }
    insta::assert_snapshot!(rendered);
}
