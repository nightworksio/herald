# ADR-0013: A webhook delivery is admitted once, released when it fails, and kept for a week

**Status:** Accepted
**Date:** 2026-11-01

## Context

ADR-0007 (decision 3) verifies a GitHub or Forgejo webhook by the HMAC of its
body, and keeps its delivery id against replay. A signature proves who sent a
request, not that it is the first time it arrives: anyone who saw a signed
delivery can send it again, and it verifies again.

The never-twice ledger (ADR-0005) already makes a replay harmless to the
channel: a replayed release maps to the same key, and the plan skips it. A
replay still costs a parse, a store read and a plan, and nothing records that
it happened.

A sender also delivers the same request again on purpose. A delivery herald
failed to process, or one an operator redelivers from the forge, carries the
id it carried the first time.

## Decision

1. **A delivery id is admitted once.**
   - **What names a delivery.** A delivery is named by what its signature
     covers, so whoever replays it cannot rename it:
     - a GitLab webhook by its `webhook-id`, which it signs;
     - a GitHub or Forgejo webhook by its signature itself, in hex. Their
       `X-GitHub-Delivery`, `X-Forgejo-Delivery` and `X-Gitea-Delivery`
       headers are signed by nothing, and a replay could carry a fresh one.

     The claim of a verified request names its delivery: the scheme, and
     that name.
   - **Admitting.** After the signature holds and before anything parses the
     body, the store admits the delivery: the first time it answers fresh
     and keeps the id, and every later time it answers seen. Admitting is one
     compare-and-set, so two copies arriving at once admit one.
   - **A seen delivery.** It is refused, and nothing parses it.
2. **A delivery that fails is released.** When processing a fresh delivery
   fails, its id is released, so the sender's next delivery of it is
   admitted. A delivery that succeeded stays admitted.
3. **An id is kept for a week.** Housekeeping expires every id admitted more
   than `DELIVERIES_KEPT`, seven days, before it runs. A replay older than
   that verifies, is admitted, and is skipped by the ledger.
4. **A store that keeps no deliveries says so.** `Capabilities` names
   whether a store keeps deliveries. Every store built on the records kit
   keeps them, and the markers store does not. A store that does not answers
   unsupported, and doctor names it as a gap where a config serves webhooks:
   its replays reach the parser, and the ledger is what skips them.
5. **The conformance suite holds every store to it.** A store that keeps
   deliveries admits each once, admits a released one again, and expires
   only what is older than it is asked to.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **Rely on the ledger alone** | A replay then reaches a parser and costs a plan, and nothing refuses or records it. ADR-0007 keeps the id. |
| **Name a GitHub or Forgejo delivery by its delivery header** | No signature covers the header, so a replay with a fresh id would be admitted. Two deliveries with the same signature carry the same body, and the second is a replay. |
| **Admit after processing** | Two copies arriving together would both be processed before either is admitted. The ledger keeps them from posting twice, but the work is done twice. |
| **Keep ids for ever** | The store grows with every webhook. A replay past a week is skipped by the ledger anyway. |
| **Refuse a redelivery of a failed delivery** | An operator redelivering a webhook herald failed to process would be refused, and the release would go unannounced. |

## Consequences

**A replayed webhook reaches no parser** while its id is kept, wherever the
store keeps deliveries.

**A failed delivery can be delivered again**, by the forge's retry or by an
operator.

**Every store gains a record kind.** A store built later implements it or
says it keeps none, and the conformance suite checks whichever it says.

## Related

- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the ledger that skips what a replay would announce
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): the verification this follows
