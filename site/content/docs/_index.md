+++
title = "Documentation"
description = "Guides and reference for oicp-kit — the ideas behind the crate, a walkthrough of every layer, and what you need to know to talk to Hubject."
sort_by = "weight"
template = "section.html"
page_template = "page.html"
+++

`oicp-kit` implements [OICP](https://github.com/hubject/oicp) — the Open InterCharge Protocol, the
roaming protocol of the **Hubject** brokering system, which connects Charge Point Operators and
e-Mobility Providers across most of Europe.

This guide explains the ideas the crate is built on and how each layer is meant to be used. For
item-by-item API detail, see [docs.rs/oicp-kit](https://docs.rs/oicp-kit).

New to the protocol? [OICP in brief](@/docs/getting-started/oicp-in-brief.md) covers the hub-and-
spoke shape, the two directions, and the per-service versioning that the rest of this guide
assumes.

## Where to start

* **Building a CPO or EMP integration** — [Install](@/docs/getting-started/install.md), then
  [Your first request](@/docs/getting-started/first-request.md).
* **Wondering why a design is the way it is** — the [Concepts](@/docs/concepts/_index.md) section.
* **Debugging an integration** — [Errata](@/docs/reference/errata.md) lists the six places
  Hubject's own documents disagree, which is where a surprising number of problems come from.
