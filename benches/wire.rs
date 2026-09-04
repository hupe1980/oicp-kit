//! What decoding a real OICP workload costs.
//!
//! The number that matters is the page one: an EMP crawling European EVSE data decodes millions of
//! records a day, and the difference between "one page per second" and "one page per minute" is
//! whether a delta crawl finishes before the next one starts.
//!
//! On an M-series laptop, a 2000-record page decodes in about 3 ms (650 MiB/s) and decodes *and*
//! validates in about 6 ms — so a 500 000-record European crawl spends a couple of seconds in this
//! crate and the rest waiting for Hubject.

// The benchmark functions are the harness, not API.
#![allow(missing_docs)]

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use oicp_kit::emp::{EvseDataResponse, PullEvseDataRecord};
use oicp_kit::testkit::samples;
use oicp_kit::types::{EvseId, Validate};

fn page_json(records: u32) -> String {
    let page = serde_json::json!({
        "content": samples::fleet("DE*ABC", records),
        "number": 0,
        "size": records,
        "totalElements": records,
        "totalPages": 1,
        "first": true,
        "last": true,
        "numberOfElements": records,
        "StatusCode": {"Code": "000"}
    });
    serde_json::to_string(&page).expect("encodes")
}

fn identifiers(c: &mut Criterion) {
    let mut group = c.benchmark_group("identifiers");

    // Every record on every page carries several of these.
    group.bench_function("parse_iso_evse_id", |b| {
        b.iter(|| std::hint::black_box("DE*AB7*E840*6487").parse::<EvseId>().unwrap());
    });
    group.bench_function("parse_din_evse_id", |b| {
        b.iter(|| std::hint::black_box("+49*810*000*438").parse::<EvseId>().unwrap());
    });

    // The routing decision Hubject makes for every authorization.
    let id: EvseId = "DE*AB7*E840*6487".parse().unwrap();
    group.bench_function("derive_operator_from_evse_id", |b| {
        b.iter(|| std::hint::black_box(&id).operator_id());
    });

    group.finish();
}

fn records(c: &mut Criterion) {
    let mut group = c.benchmark_group("records");

    let record = samples::pull_evse_data_record("DE*ABC*E1");
    let json = serde_json::to_string(&record).expect("encodes");

    group.bench_function("decode_evse_record", |b| {
        b.iter(|| serde_json::from_str::<PullEvseDataRecord>(std::hint::black_box(&json)).unwrap());
    });
    group.bench_function("encode_evse_record", |b| {
        b.iter(|| serde_json::to_string(std::hint::black_box(&record)).unwrap());
    });
    group.bench_function("validate_evse_record", |b| {
        b.iter(|| std::hint::black_box(&record).validate().unwrap());
    });

    let cdr = samples::charge_detail_record("DE*ABC*E1", samples::session_id());
    let cdr_json = serde_json::to_string(&cdr).expect("encodes");
    group.bench_function("decode_cdr", |b| {
        b.iter(|| {
            serde_json::from_str::<oicp_kit::cpo::ChargeDetailRecord>(std::hint::black_box(&cdr_json))
                .unwrap()
        });
    });
    group.bench_function("validate_cdr", |b| {
        b.iter(|| std::hint::black_box(&cdr).validate().unwrap());
    });

    group.finish();
}

fn pages(c: &mut Criterion) {
    let mut group = c.benchmark_group("pages");
    group.sample_size(20);

    // A realistic page: what one round trip of a crawl carries.
    let json = page_json(2000);
    group.throughput(criterion::Throughput::Bytes(json.len() as u64));

    group.bench_function("decode_page_of_2000", |b| {
        b.iter(|| serde_json::from_str::<EvseDataResponse>(std::hint::black_box(&json)).unwrap());
    });

    group.bench_function("decode_and_validate_page_of_2000", |b| {
        b.iter(|| {
            let page: EvseDataResponse = serde_json::from_str(std::hint::black_box(&json)).unwrap();
            for record in &page.content {
                let _ = record.validate();
            }
            page
        });
    });

    // Applying a page to an EMP's copy — the other half of a crawl's cost.
    let page: EvseDataResponse = serde_json::from_str(&json).unwrap();
    group.bench_function("apply_page_of_2000_to_repository", |b| {
        b.iter_batched(
            || (oicp_kit::sync::InMemoryEvseRepository::new(), page.content.clone()),
            |(mut repository, records)| oicp_kit::sync::apply(&mut repository, records).unwrap(),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, identifiers, records, pages);
criterion_main!(benches);
