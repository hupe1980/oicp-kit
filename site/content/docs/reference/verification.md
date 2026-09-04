+++
title = "How it is verified"
weight = 20
description = "What CI checks, and what each check would catch."
+++

## Against the specification

`cargo run -p xtask -- all` runs four checks. Three of them need the Hubject documents cloned into
`specs/` (gitignored, since the specifications are CC BY-SA and not redistributed here); without
them they **skip** rather than fail, so a contributor without the clones is not blocked.

**`no-floats`** scans all of `src/` for `f32`/`f64` in code this crate wrote. Energy and money end
up on an invoice, and a binary float cannot represent `0.10`. One file is exempt — `types/number.rs`,
the JSON boundary — and the check prints its name on success rather than hiding it behind an
`allow`.

**`endpoints`** diffs `transport::Operation` against the paths in Hubject's published OpenAPI
documents. A revision that moves an endpoint becomes a failing job rather than a 404 in production.

**`errata`** checks that each recorded contradiction *still exists* upstream. An erratum Hubject
fixes fails CI, so the registry cannot rot into stale claims.

**`spec-sync`** compares the vendored clones against the commits this crate was written against,
and with `--upstream` asks Hubject's remotes what they hold *now* — one ref lookup each, no clone.
OICP 2.3 is edited **in place**, so nothing in this repository changes when Hubject edits it and a
check that only reads the working copy can never notice. The `--upstream` form runs weekly in CI,
so an edit arrives as a failing job with the new commit in it.

## Against the wire

**The specification's own examples** are decoded, validated and re-encoded byte-for-byte in
`tests/wire.rs` — including the ones that are themselves non-conformant, where the test asserts
that `validate()` reports the problem.

**Snapshots** (`tests/snapshots.rs`) pin the JSON this crate emits, the endpoint table and the
errata registry. OICP field names are irregular — `EvseID` but `lastUpdate`, `deltaType` but
`DynamicInfoAvailable` — and a rename that looks like a tidy-up in Rust is a broken integration on
the wire.

**Round-trip tests** assert that every sample, every unknown field and every unknown enum value
comes back out unchanged.

**Fuzzing.** Three `cargo fuzz` targets — `identifiers`, `wire` and `delta` — cover the surfaces
that decode input this crate did not write. `fuzz/` is a nested workspace that `cargo check` at the
root skips, so CI type-checks it separately on every push, and a nightly workflow runs each target
for a bounded time, uploading any crash as an artefact.

`cargo run -p xtask -- seed-fuzz` writes the corpus from the crate's own conformant messages and
every identifier spelling the specification prints. The seeds are derived rather than committed, so
they cannot rot beside the wire model — re-run it after changing a type.

**Mutation testing** answers the question coverage cannot: not *was this line executed* but *would
a test have noticed if it did the wrong thing*. `cargo mutants` is run over `sync` and `types` after
a change to either. It is not in CI — a full run takes hours, and survivors need judgement rather
than a threshold: a `<` that could be `<=` on a length guard is real, while one that only changes
which of two rejections an over-short input receives cannot be killed at all.

## Properties

`tests/properties.rs` checks what examples cannot:

* An identifier is written back exactly as it arrived, for arbitrary ISO and DIN inputs.
* Separators and case do not change identity — including `Hash`/`Eq` agreement, without which a
  `HashMap` of charging points silently duplicates.
* Any string decodes as an identifier and round-trips; it is either well-formed **or** reported.
* Every number OICP can carry survives the JSON boundary exactly.
* **Applying any sequence of deltas leaves the same state as a full pull.** This one found a real
  bug: `deltaType` was being stored, so a delta-built copy could never equal a full-pull one.
* Applying the same delta twice is the same as applying it once, so a retried page is harmless.
* A push plan replays onto the previous fleet to reproduce the current one, and never uses
  `fullLoad`.
* Arbitrary unknown fields survive a round trip.

## Sequences

`tests/end_to_end.rs` runs a CPO and an EMP, both built on this crate, through a `MockHubject`:
authorize, notify, settle; a remote start reaching the CPO; a CDR for a session nobody opened;
a reservation refused honestly.

`testkit::scenarios::run_all()` is the same idea packaged for *your* code, so you can run it
against your own service implementations.

## Everything else

* `cargo clippy --all-features --all-targets -- -D warnings`, with `pedantic` on.
* `cargo fmt --check`.
* `cargo test --all-features`, plus `--no-default-features` and each feature alone, so a feature
  that only compiles when another happens to be on is caught.
* `cargo doc --all-features` with `-D warnings`: every public item is documented, and every intra-
  doc link resolves.
* `cargo deny check` for licences and advisories.
* `cargo bench` on a schedule.
