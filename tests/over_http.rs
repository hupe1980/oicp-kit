//! The client against a broker on a real socket.
//!
//! Everything else in this test suite exercises the crate's *logic*. This exercises the
//! *integration*: the URL the client builds from the endpoint table, the `?page=&size=` it
//! appends, how it decodes a page, what it does with a `200` that says `Result: false`, whether a
//! crawl follows `last` to the end. None of that can be tested in-process, and all of it is what
//! breaks first against the real brokering system.

use futures_util::StreamExt as _;
use oicp_kit::client::ClientConfig;
use oicp_kit::client::{CpoClient, CrawlError, EmpClient, RetryPolicy};
use oicp_kit::cpo::{ChargingNotification, EvseStatus, OperatorEvseStatus, PushEvseStatusRequest};
use oicp_kit::emp::{PullEvseDataRequest, PullEvseStatusByIdRequest, PullEvseStatusRequest};
use oicp_kit::testkit::{MockEmp, MockHubject, MockHubjectServer, samples};
use oicp_kit::transport::{OicpError, PageQuery};
use oicp_kit::types::{ActionType, Code, Extensions, GeoCoordinatesFormat, Text, Validate};

/// A broker with `fleet` charging points, behind a socket.
async fn served(fleet: u32) -> MockHubjectServer {
    let mut hubject = MockHubject::new();
    hubject.register_emp(MockEmp::permissive("DE-DCB".parse().unwrap()));
    for i in 0..fleet {
        hubject.push_evse_data(&samples::evse_data_record(&format!("DE*ABC*E{i}")).into()).expect("push");
    }
    MockHubjectServer::start(hubject).await.expect("binds")
}

fn cpo_client(server: &MockHubjectServer) -> CpoClient {
    CpoClient::builder()
        .environment(server.environment())
        .operator_id("DE*ABC".parse().unwrap())
        .build()
        .expect("builds")
}

fn emp_client(server: &MockHubjectServer) -> EmpClient {
    EmpClient::builder()
        .environment(server.environment())
        .provider_id("DE-DCB".parse().unwrap())
        .build()
        .expect("builds")
}

#[tokio::test]
async fn a_push_reaches_the_broker_at_the_url_the_endpoint_table_builds() {
    let server = served(0).await;
    let client = cpo_client(&server);

    let ack = client
        .push_evse_data_insert(vec![samples::evse_data_record("DE*ABC*E1")], "ABC Technologies")
        .await
        .expect("the push lands");

    assert!(ack.is_success());
    assert_eq!(*ack.code(), Code::Success);
    assert_eq!(server.hubject().evse_count(), 1, "the broker actually stored it");
    server.stop().await;
}

#[tokio::test]
async fn a_pull_comes_back_decoded_with_its_page_metadata() {
    let server = served(3).await;
    let client = emp_client(&server);

    let request = PullEvseDataRequest::full("DE-DCB".parse().unwrap(), GeoCoordinatesFormat::Google);
    let page = client.pull_evse_data_page(&request, PageQuery::new()).await.expect("the pull lands");

    assert_eq!(page.content.len(), 3);
    assert_eq!(page.total_elements, 3);
    assert!(page.first && page.last);
    page.validate().expect("the page is conformant");
    server.stop().await;
}

#[tokio::test]
async fn a_crawl_over_many_pages_sees_every_record_exactly_once() {
    // The page-size query has to survive the round trip, or a crawl either loops or truncates.
    let server = served(47).await;
    let client = emp_client(&server);

    let request = PullEvseDataRequest::full("DE-DCB".parse().unwrap(), GeoCoordinatesFormat::Google);
    let mut stream = Box::pin(client.crawl_evse_data(request, PageQuery::with_size(10)));

    let mut seen = vec![];
    while let Some(item) = stream.next().await {
        seen.push(item.expect("no record fails to decode").evse_id.canonical());
    }

    assert_eq!(seen.len(), 47, "the crawl visited every record");
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 47, "…and none of them twice");
    server.stop().await;
}

#[tokio::test]
async fn a_two_hundred_that_says_result_false_is_an_error_not_a_success() {
    // The failure mode this crate exists to prevent: Hubject answers a refused operation with
    // HTTP 200 and a body saying it did not happen.
    let server = served(1).await;
    let client = cpo_client(&server);

    // A CDR for a session the broker never opened.
    let cdr = samples::charge_detail_record("DE*ABC*E0", samples::session_id());
    let error = client.send_charge_detail_record(&cdr).await.expect_err("the broker refuses it");

    assert!(matches!(error, OicpError::Rejected { .. }), "a refusal must not look like success: {error}");
    assert_eq!(error.code(), Some(&Code::SessionIsInvalid));
    assert!(!error.is_retryable(), "an invalid session does not become valid on a retry");
    server.stop().await;
}

#[tokio::test]
async fn a_malformed_body_comes_back_as_an_http_error_with_the_reason() {
    // Hubject answers a body that is not the message it should be with 400 and a description,
    // rather than with `Result: false`. A client that only handles the acknowledgement path would
    // treat this as a success.
    let server = served(0).await;
    let client = cpo_client(&server);
    let url = format!("{}/evsepush/v23/operators/DE*ABC/data-records", server.base_url());

    let error: OicpError = client
        .transport()
        .post_raw::<_, oicp_kit::types::Acknowledgement>(
            oicp_kit::transport::Operation::PushEvseData,
            &url,
            &serde_json::json!({"ActionType": "insert", "OperatorEvseData": "not an object"}),
        )
        .await
        .expect_err("the broker refuses it");

    let OicpError::Http { status, url: called, body } = &error else {
        panic!("expected an HTTP error: {error}")
    };
    assert_eq!(*status, 400);
    assert!(body.contains("OperatorEvseData"), "the reason names the field: {body}");
    assert_eq!(called, &url, "the error says which endpoint answered");
    server.stop().await;
}

#[tokio::test]
async fn a_non_conformant_request_is_refused_locally_before_it_is_sent() {
    let server = served(0).await;
    let client = cpo_client(&server);

    // A fullLoad with no records would withdraw the operator's whole fleet.
    let error = client
        .push_evse_data_full_load(vec![], "ABC Technologies")
        .await
        .expect_err("the client refuses to send it");

    assert!(matches!(error, OicpError::Invalid(_)), "expected a local refusal: {error}");
    assert!(error.to_string().contains("removes every charging point"));
    // Nothing reached the broker.
    assert!(server.hubject().events().is_empty());
    server.stop().await;
}

#[tokio::test]
async fn the_local_check_can_be_switched_off_for_a_peer_that_needs_it() {
    let server = served(0).await;
    let client = CpoClient::builder()
        .environment(server.environment())
        .operator_id("DE*ABC".parse().unwrap())
        .config(ClientConfig { validate_requests: false, ..ClientConfig::default() })
        .build()
        .expect("builds");

    // The same request now goes out, and the *broker* decides. The refusal that came back as
    // `Invalid` when checked locally comes back as `Rejected` when checked remotely — which is the
    // whole difference the setting makes, and why leaving it on is the better default.
    let error = client.push_evse_data_full_load(vec![], "ABC Technologies").await.expect_err("still refused");

    assert!(matches!(error, OicpError::Rejected { .. }), "expected a refusal from the broker: {error}");
    assert_eq!(error.code(), Some(&Code::DataError));
    server.stop().await;
}

#[tokio::test]
async fn the_whole_session_sequence_runs_over_http() {
    let server = served(1).await;
    let cpo = cpo_client(&server);

    // 1. A driver presents a card.
    let response = cpo
        .authorize_start(&samples::authorize_start_request("DE*ABC*E0"))
        .await
        .expect("the authorization lands");
    assert!(response.is_authorized());
    response.validate().expect("conformant");
    let session_id = response.session_id.clone().expect("an authorized response carries a session");

    // 2. Energy starts flowing.
    let notification =
        ChargingNotification::Start(samples::charging_notification_start("DE*ABC*E0", session_id.clone()));
    cpo.send_charging_notification(&notification).await.expect("the notification lands");

    // 3. The session settles.
    let cdr = samples::charge_detail_record("DE*ABC*E0", session_id);
    cpo.send_charge_detail_record(&cdr).await.expect("the CDR settles");

    assert_eq!(server.hubject().cdrs().len(), 1);
    assert!(server.hubject().sessions().iter().all(|s| s.settled));
    server.stop().await;
}

#[tokio::test]
async fn status_pulls_answer_all_three_shapes_on_one_endpoint() {
    let server = served(2).await;
    let cpo = cpo_client(&server);
    let emp = emp_client(&server);

    cpo.push_evse_status(&PushEvseStatusRequest {
        action_type: ActionType::Update,
        operator_evse_status: OperatorEvseStatus {
            operator_id: "DE*ABC".parse().unwrap(),
            operator_name: Some(Text::new_unchecked("ABC Technologies")),
            evse_status_record: vec![samples::evse_status_record("DE*ABC*E0", EvseStatus::Occupied)],
            extensions: Extensions::new(),
        },
    })
    .await
    .expect("the status push lands");

    // The bulk pull.
    let all = emp
        .pull_evse_status(&PullEvseStatusRequest {
            provider_id: "DE-DCB".parse().unwrap(),
            search_center: None,
            evse_status: None,
            extensions: Extensions::new(),
        })
        .await
        .expect("the status pull lands");
    let records: Vec<_> = all.records().collect();
    assert_eq!(records.len(), 2, "both charging points are reported");
    assert!(
        records.iter().any(|(_, r)| r.evse_status == EvseStatus::Occupied),
        "the pushed status came back"
    );
    assert!(
        records.iter().any(|(_, r)| r.evse_status == EvseStatus::Unknown),
        "a point with no pushed status is Unknown, not omitted"
    );

    // The by-id pull, on the same endpoint, told apart by its body.
    let by_id = emp
        .pull_evse_status_by_id(&PullEvseStatusByIdRequest {
            provider_id: "DE-DCB".parse().unwrap(),
            evse_id: vec!["DE*ABC*E0".parse().unwrap(), "DE*ABC*E999".parse().unwrap()],
            extensions: Extensions::new(),
        })
        .await
        .expect("the by-id pull lands");

    let records = &by_id.evse_status_records.evse_status_record;
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].evse_status, EvseStatus::Occupied);
    assert_eq!(
        records[1].evse_status,
        EvseStatus::EvseNotFound,
        "an unknown id is EvseNotFound, so the EMP can tell it from 'not returned'"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_crawl_reports_a_failed_page_rather_than_silently_stopping() {
    let server = served(5).await;
    let client = emp_client(&server);
    let request = PullEvseDataRequest::full("DE-DCB".parse().unwrap(), GeoCoordinatesFormat::Google);

    // Take the broker away mid-crawl.
    server.stop().await;

    let mut stream = Box::pin(client.crawl_evse_data(request, PageQuery::with_size(2)));
    let first = stream.next().await.expect("the crawl yields something");
    assert!(matches!(first, Err(CrawlError::Page { page: 0, .. })), "a lost page is reported, not skipped");
    assert!(stream.next().await.is_none(), "and the crawl stops rather than continuing past it");
}

#[tokio::test]
async fn a_retry_policy_of_none_does_not_slow_a_failure_down() {
    // The client is configurable end to end; this also pins that a dead endpoint fails fast.
    let client = EmpClient::builder()
        .environment(oicp_kit::transport::HubjectEnv::Custom("http://127.0.0.1:1".into()))
        .provider_id("DE-DCB".parse().unwrap())
        .config(ClientConfig { retry: RetryPolicy::none(), ..ClientConfig::default() })
        .build()
        .expect("builds");

    let request = PullEvseDataRequest::full("DE-DCB".parse().unwrap(), GeoCoordinatesFormat::Google);
    let started = std::time::Instant::now();
    let error =
        client.pull_evse_data_page(&request, PageQuery::new()).await.expect_err("nothing is listening");

    assert!(matches!(error, OicpError::Transport { .. }));
    assert!(error.is_retryable(), "a lost connection is worth another try — the policy just said no");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}
