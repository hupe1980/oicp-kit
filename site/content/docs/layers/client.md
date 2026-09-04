+++
title = "Client"
weight = 20
description = "Mutual TLS, streaming crawls, and retries that will not start a session twice."
+++

## Mutual TLS, checked locally

OICP has no tokens. Authentication is the client certificate; authorisation is Hubject comparing
your identifier against it.

```rust
let identity = ClientIdentity::from_pem_file("hubject-client.pem")?;
let client = CpoClient::builder()
    .environment(HubjectEnv::Qa)
    .operator_id("DE*ABC".parse()?)
    .identity(identity)
    .build()?;

if let Some(warning) = client.identity_warning() {
    eprintln!("{warning}");
}
```

The warning is a warning rather than a refusal for two reasons: a hub operator legitimately acts
for bundled sub-partners whose identifiers are not in its certificate, and a certificate this crate
cannot fully parse must not stop a partner from working.

`ClientIdentity`'s `Debug` never prints key material.

The default environment is `Qa`. A default that pushes to production would be a trap.

## Streaming crawls

An unfiltered European `PullEvseData` is hundreds of thousands of records. The crawl holds one page
at a time and yields per record:

```rust
use futures_util::StreamExt;

let request = PullEvseDataRequest::full(client.provider_id().clone(), GeoCoordinatesFormat::Google);
let mut stream = Box::pin(client.crawl_evse_data(request, PageQuery::new()));

while let Some(item) = stream.next().await {
    match item {
        Ok(record) => store(record).await?,
        Err(CrawlError::Record { page, index, message }) => {
            tracing::warn!(page, index, %message, "skipping one bad record");
        }
        Err(CrawlError::PageInconsistent { page, message }) => {
            tracing::warn!(page, %message, "the page contradicts itself");
        }
        Err(error @ CrawlError::Page { .. }) => return Err(error.into()),
    }
}
```

A bad **record** costs one record: the page envelope is decoded first and each record on its own,
so one operator's malformed `EvseDataRecord` does not cost the other 1999 — or every record after
them. A failed **page** ends the crawl, because continuing past it would silently skip data.

## The pages a page claims

A page says twice whether there is more — once in `last`, once in `totalPages` — and they can
disagree. Neither is believed alone:

* `last: true` on page 0 of 300 would end the crawl after a third of a percent of Europe, and every
  count would read as a success. The crawl goes on by `totalPages` and yields a
  `PageInconsistent`.
* `last: false` on the final page, or a server that never advances `number`, would crawl forever.
  `totalPages` bounds the walk whenever it is known; an empty page always ends it.

Every disagreement is yielded rather than resolved in silence: a paging bug at the far end is worth
knowing about before it shows up as a hole in the map.

## Retries that know what they are retrying

```rust
let policy = RetryPolicy::default();
let lost = OicpError::transport("connection reset");

assert!(policy.should_retry(Operation::PushEvseData, &lost, 0).is_some());
assert!(policy.should_retry(Operation::AuthorizeRemoteStart, &lost, 0).is_none());
```

A lost transport response may mean the request never arrived — or that it arrived and the *answer*
was lost. For a remote start the second case means a retry could start a **second charging
session**, so the four state-changing operations are not retried by default:

`AuthorizeRemoteStart`, `AuthorizeRemoteStop`, `AuthorizeRemoteReservationStart`,
`AuthorizeRemoteReservationStop`.

Everything else is a push of state, a read, or a record of something that already happened, which
Hubject deduplicates on the session id.

Rejections are never retried except for the transient codes — `001`, `002`, `009`, `021`, `310`,
`320`. Repeating an identical request gets an identical decision.

Backoff doubles, is capped, and is jittered by default: without jitter every client that lost the
same Hubject instance retries in lockstep and knocks it over again on recovery.

## Pushing a fleet safely

```rust
let plan = PushPlanner::plan(&previous_snapshot, &current_fleet);
client.push_evse_data_plan(plan, "ABC Technologies").await?;
```

See [Delta sync](@/docs/layers/sync.md). `push_evse_data_full_load` exists, logs a warning when
pointed at production, and says in its name what it does.
