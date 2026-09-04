+++
title = "Install"
weight = 20
description = "Adding the crate, choosing features, and what each one pulls in."
+++

```console
cargo add oicp-kit
```

Default features are `cpo`, `emp` and `transport`: the wire model and the addressing, with no async
runtime and no HTTP client.

## Features

| Feature | Pulls in | For |
|---|---|---|
| `cpo` | — | the CPO half of the wire model |
| `emp` | — | the EMP half |
| `transport` | `http` | the endpoint table, paging, error mapping |
| `client` | `reqwest`, `tokio`, `rustls`, `tracing` | talking to Hubject |
| `server` | `axum`, `tower`, `tokio` | the half Hubject calls |
| `sync` | — | the delta engine and the push planner |
| `eichrecht` | — | calibration-law types and CDR pre-flight |
| `testkit` | — | samples, `MockHubject`, onboarding scenarios |
| `schema` | `schemars` | `JsonSchema` for every wire type |
| `cli` | `clap` + `full` | the `oicp` binary |

`full` turns on everything except the CLI.

A typical CPO:

```toml
oicp-kit = { version = "0.1", features = ["client", "server", "sync", "eichrecht"] }
```

A typical EMP:

```toml
oicp-kit = { version = "0.1", features = ["client", "server", "sync"] }
```

Add `testkit` as a dev-dependency either way — it is how you test without onboarding:

```toml
[dev-dependencies]
oicp-kit = { version = "0.1", features = ["testkit"] }
```

## The CLI

```console
cargo install oicp-kit --features cli
oicp id 'DE*AB7*E840*6487'
```

## Minimum supported Rust

**1.85** — the first release with edition 2024, which this crate uses. It is checked in CI on every
push, so it is a fact rather than an intention. Raising it is a minor version bump.
