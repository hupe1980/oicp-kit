+++
title = "Security"
weight = 30
description = "Mutual TLS, what the crate does with key material, and the hashing OICP still permits."
+++

## Mutual TLS is the whole of authentication

OICP has no API key, no OAuth and no bearer token. Hubject issues a client certificate; that
certificate is the identity. Authorisation is Hubject comparing the `OperatorID`/`ProviderID` in
your URL path against it, textually.

Consequences worth stating:

* **A leaked client certificate is a full compromise** of that partner's OICP surface — pushes,
  authorizations, CDRs. There is no second factor and no token to rotate independently.
* **Identifiers must not be normalised.** See [Identifiers](@/docs/concepts/identifiers.md).
* **`ClientIdentity`'s `Debug` never prints key material**, and the PEM is not logged.

`ClientIdentity::check_against` compares your identifier with the certificate's names locally, so
the `017 Unauthorized Access` that would otherwise appear on every request is caught at startup. It
is a diagnostic, not a security boundary: it warns rather than refuses, because a hub operator
legitimately acts for bundled sub-partners.

## Serving the Hubject-facing endpoints

The `axum` routers in [`server`](@/docs/layers/server.md) do **not** terminate TLS or verify client
certificates — that belongs to your TLS terminator, and doing it in the wrong layer is how it ends
up not being done at all.

Terminate mutual TLS in front of the router, verify the peer against Hubject's CA, and — since
OICP's own authorisation model is identifier-versus-certificate — check that the identifier in the
request path matches the peer you just authenticated.

## Hashed PINs

`QRCodeIdentification` may carry a PIN. The specification allows:

* `HashedPIN` with `Function: Bcrypt` — the only acceptable choice for new data.
* `LegacyHashData` with `MD5` or `SHA-1`, for PINs an EMP hashed before bcrypt was required.
* A **plaintext** `PIN` field.

This crate models the legacy functions as a *separate type* (`LegacyHashFunction`) rather than two
more variants of `HashFunction`, so a value of that type is by construction in the legacy slot and
cannot be selected for new data by accident.

**Which of the two to send is not a preference, and the instinct is backwards.** The specification
splits them by process, in opposite directions:

> *`HashedPIN`: […] This field can be provided only when uploading Authentication data. In
> Authorization requests this field must be null!*
>
> *`PIN`: The pin number, this field is required in Authorization requests!*

So an `eRoamingAuthorization` request carries the **plaintext** PIN and no hash, and a
`PushAuthenticationData` upload carries the hash. Sending the hash to authorize reads as the safer
choice and is what the specification forbids, in so many words.

`Identification::validate_in_process` checks both directions, and the wire types pass their
context, so an authorization request that carries a `HashedPIN` or omits its `PIN` is reported
before it is sent. A context-free `validate()` cannot decide this: the same object is right in one
message and wrong in the other. What it does report on its own is a payload carrying both, and a
`PIN` longer than the twenty characters the field allows.

Sending a PIN in the clear across a roaming hub is still a poor idea. The field exists because the
specification requires it, and mutual TLS is what protects it in transit.

## What this crate does not protect you from

* **Replay.** OICP has no request signing or nonce. Mutual TLS is the only protection.
* **A malicious counterparty's data.** `Extensions` preserves whatever a peer sent, including
  values you will later render or store. Treat it as untrusted input.
* **Denial of service by page size.** A crawl streams, so memory is bounded per page — but a peer
  choosing an enormous page can still make one response large. Set a timeout; the client's default
  is 30 seconds.

## Reporting

Security issues: open a private advisory on the repository rather than a public issue.
