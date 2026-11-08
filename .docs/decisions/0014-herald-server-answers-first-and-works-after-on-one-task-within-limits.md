# ADR-0014: herald server answers first and works after, on one task, within limits

**Status:** Accepted
**Date:** 2026-11-05

## Context

ADR-0002 adds an HTTP endpoint at its second and third layers: verified forge
webhooks in place of Actions, and herald's ActivityPub actors, whose
followers find them through WebFinger, read their documents and Notes, and
deliver follows to their inboxes. ADR-0001 names `herald-server` the
container's driver.

A forge gives up on a webhook within seconds. GitHub waits 10. One release
can post to several destinations and deliver to many inboxes, which takes
longer than that. An endpoint on the public internet also meets slow
clients, floods and requests it should never parse.

Every inbound activity names a key on its sender's actor document. Reading
that document for every activity costs a request to another server, and
lets a burst of follows or forgeries become a burst of outbound reads.

A Note is published inside the `Create` each follower receives, but anyone
may fetch it later by its id, as a search, a boost by URL or a remote refresh
does. Nothing herald keeps today holds its content.

## Decision

1. **The endpoint is answered by flows, and served by one crate.**
   - `herald_flows::answer` turns one request into its reply and the work it
     leaves, without I/O of its own: health, `/hooks/<source>`, WebFinger,
     each actor's document, its Notes and inboxes, and the shared inbox.
     `herald_flows::work` does the work. The serverless drivers serve the
     same two.
   - `herald-server` serves them over HTTP/1.1 with hyper. It sits in a
     layer of its own, after the native ports and before the drivers, so the
     `herald` binary names it. `herald server` is that binary's command, and
     the image runs it.
   - TLS is a reverse proxy's. herald listens on `127.0.0.1:8080` unless
     `--listen`, `HERALD_LISTEN` or `[server] listen` says otherwise, in that
     order.
2. **A request is answered first, and its work done after.**
   - A webhook is verified, admitted (ADR-0013), parsed and answered 202,
     and its release is announced once the reply is sent. An activity is
     verified and admitted, answered 202, and then taken.
   - Work that stops or fails releases its delivery, so the sender's next
     delivery of it is admitted. Work still waiting when the server stops is
     released the same way. Work that was running is finished first.
   - A process that dies mid-announcement leaves a ledger entry the next
     poll finishes (ADR-0005).
3. **Everything runs on one task, within limits the config sets.**
   - `jobs` (8) pieces of work run at once, and as many again wait. A
     request's work takes its place in the queue once the request is read
     and verified, so a slow body holds no place. Work with no place is
     released, so its sender's retry is admitted, and its request is
     answered 503 with `Retry-After`; the queue never holds more than
     `jobs`.
   - `connections` (256) are held at once. Each carries one request, whose
     headers and then body must arrive within 30 seconds, and is closed
     after 90. A body is at most `body` (1 MiB).
   - `fetches` (4) actor documents are read at once.
   - `completions` (4) autocompletions are answered at once by each
     session; one arriving while as many are answered is dropped and said.
   - `deliveries` (8) of an announcement's inboxes are delivered to at once,
     each still one step of the ledger. The CLI honours it too.
   - Every host herald composes on keeps the same limits: a body past
     `body` is refused 413, work past `jobs` is released and refused 503
     with `Retry-After`, and one round of each kind runs at a time. The
     Node shim also holds `connections` and the 30- and 90-second times.
   - Sources are polled and threads kept on `[schedule]`, and keys read
     longer ago than their refresh are forgotten. One round of each kind
     runs at a time.
4. **A key is kept by its id, in memory or in the store.**
   - A `Keyring` port keeps each key an activity was checked with, with its
     owner and when it was read. A key read within `[server.keys]
     refresh_after` (1 hour) checks the next activity signed with it without
     reading its document again. An activity that does not hold under a kept
     key has the key read again, as one rotated since, once `[server.keys]
     reread_after` (1 minute) has passed since it was read; within it the
     activity is refused, so forgeries naming a kept key read nothing.
   - `cache = "memory"`, the default, keeps at most `most` (1024) keys and
     forgets the one used longest ago. `cache = "store"` keeps them in the
     store, across restarts and shared by every instance, and every store
     built on the records kit is a keyring. The markers store keeps none, and
     the config refuses `cache = "store"` over it.
5. **Each Note is kept in the store.** When an ActivityPub route's post
   lands, its Note is written as a record by the announcement's key, before
   the ledger takes the step. `/users/<actor>/notes/<key>` serves it,
   where it names that actor's own id. The markers store keeps no Notes,
   and the config already refuses an ActivityPub route over it. The
   conformance suite holds every store that keeps Notes to keeping the one
   written last.
6. **A webhook's release must be of its source's repository.** A delivery is
   announced only where its event header names a release and its payload's
   repository is the one its source names. Another event is answered and
   announces nothing.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **Announce, then reply** | A slow destination pushes past the forge's limit, and the forge reports a failure for a delivery that landed. |
| **A separate `herald-server` binary** | Two binaries to sign and check, and the image and the Action would ship different code. |
| **axum** | Its router and middleware duplicate what `answer` already does, and its handlers must be `Send`, which a store borrowed by the server is not. |
| **TLS in herald** | Renewal becomes herald's, and a reverse proxy or the platform already terminates TLS. |
| **No key cache** | Every activity becomes a request to another server, and a burst of forgeries becomes a burst of outbound reads. |
| **Re-render a Note from its event** | A changed template would change a Note already published, which ActivityPub reads as an unsigned edit. |
| **Unbounded queues and connections** | A flood holds every connection and grows memory without end. |

## Consequences

**The serverless drivers serve the same flows.** Only their transport and
their task model differ.

**Every store gains two record kinds**: Notes, and keys where the config
keeps them in the store.

**A forge sees 202 before its release is announced.** A failure shows in the
server's output and in the ledger, not in the forge's delivery log.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): the layering this adds a layer to; superseded in part
- [ADR-0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md): the layers the endpoint serves
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the ledger that finishes interrupted work
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): every request verified before it is parsed
- [ADR-0013](0013-a-webhook-delivery-is-admitted-once-released-when-it-fails-and-kept-for-a-week.md): admitting and releasing deliveries
