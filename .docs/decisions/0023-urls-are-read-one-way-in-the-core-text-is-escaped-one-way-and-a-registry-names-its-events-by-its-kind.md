# ADR-0023: URLs are read one way in the core, text is escaped one way, and a registry names its events by its kind

**Status:** Accepted
**Date:** 2026-11-13

## Context

ADR-0001 puts escaped path segments in the `herald-wire` kit. Several
crates read a URL's host by splitting its text: a source's identity, the
socket hosts Slack and Discord hand out, the DynamoDB store's signed
`Host`, the Actions cache's blob origin and the CLI's health check. The
HTTP clients that reach those URLs read them as WHATWG does, so on a URL
with a user, an IPv6 address, a default port or a backslash the two
readings name different hosts. Four encoders wrote a byte as a mark and
two hex digits, each with its own set of bytes kept.

ADR-0003 names a registry's events by the registry, the package and the
version. Where a source sets `api` to a mirror, the registry was taken to
be the mirror's host.

## Decision

1. **`herald-core` reads every URL, with the `url` crate.** `Address`
   reads a URL as WHATWG does, as reqwest and the composition's reach do,
   and gives its scheme, host, port, authority, origin and request target.
   No crate reads a URL's parts from its text. The arch test allows `url`
   in the core, and `herald-wire` holds no escaping and no host.
2. **Text is escaped one way.** `Escaping` is a set of bytes kept, a mark,
   and the case of the hex digits after it. Path segments and query values
   keep a URL's unreserved bytes and mark the rest with `%`. Links in
   Markdown keep visible ASCII but what ends a link. Record paths keep
   letters, digits, `.`, `_` and `-` and mark the rest with `~`. The same
   value reads its text back.
3. **Identity names the registry by its kind.** A package version's
   identity is the registry's kind, the package and the version, so a
   source moved to a mirror sees the versions it saw before. A release's
   identity names the host of its page as `Address` reads it, and a page
   that names no host makes no event.

## Alternatives considered

| Option | Why not |
|--------|---------|
| **`url` in the kits alone** | The socket hosts are checked in the core and the signed `Host` in a store, so each would keep its own reading, or a copy of one. |
| **Each hand parser corrected** | Each would still read some URL otherwise than the client that reaches it, and the next parser would start again. |
| **A registry named by its `api` host** | Moving a source to a mirror makes every version it saw new, and a poll announces a page of them. |

## Consequences

**A host herald checks is the host it reaches.** A socket URL is taken, a
request is signed and a blob origin is reached by the host the HTTP client
connects to.

**A fuzz target reads hostile URLs.** Each address reads back from its own
origin, a socket is taken only under its domains with no user, and escaped
text reads back to itself (ADR-0008).

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): what `herald-wire` holds, superseded in part here
- [ADR-0003](0003-every-source-becomes-one-neutral-event.md): each source's identity, extended here
- [ADR-0008](0008-every-line-is-covered-and-no-mutant-survives.md): every parser fuzzed
