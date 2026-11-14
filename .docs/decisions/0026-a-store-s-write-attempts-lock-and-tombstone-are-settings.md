# ADR-0026: A store's write attempts, lock and tombstone are settings

**Status:** Accepted
**Date:** 2026-11-14

## Context

Every store that keeps records writes with a compare-and-set (ADR-0005). A
write that loses its race to a write of another record is tried again, a
fixed three times. The `repo-variables` store holds one lock for every
write, taken as abandoned once held ten minutes by GitHub's clock, or, where
GitHub's answer names no time, once found held and unchanged ten times. The
`fastly-kv` store marks a deleted record for an hour before Fastly expires
the mark, so a write that read the record before it was deleted fails. None
of these could be set, though how long a lock holds and how often a lost
write is tried weigh a store's speed against how soon a run stopped half way
is recovered from.

## Decision

1. **`[store] attempts`**, 3 where absent and at most 12, sets how many times
   every store that keeps records tries a write that lost a race to an
   unrelated write. It sits beside `keep_deliveries`, `keep_ledger` and
   `keep_notes`.
2. **`[store] lock_lease`** on `repo-variables`, "10m" where absent and at
   most "40m", sets how long its lock holds by GitHub's clock before it is
   taken as abandoned.
3. **`[store] lock_patience`** on `repo-variables`, 10 where absent and at
   most 40, sets how many times a lock with no time to judge it by is found
   held and unchanged before it is taken as abandoned.
4. **`[store] tombstone`** on `fastly-kv`, "1h" where absent, sets how long a
   deleted record's mark is kept.
5. **The caps are four times the default**, as for every soundness bound
   the config sets, and `herald doctor` names each of `attempts`,
   `lock_lease` and `lock_patience` set above its default.

```toml
[store]
kind = "repo-variables"
token = { secret = "HERALD_VARIABLES_TOKEN" }
key = { secret = "HERALD_STORE_KEY" }
attempts = 3
lock_lease = "10m"
lock_patience = 10
```

## Consequences

**A store that races often can be given more attempts**, and doctor says so
whenever the value is above 3.

**A repository whose runs are slow can hold its lock longer**, at the cost of
waiting longer for a run that stopped.

**A config that sets none of them runs as before.**

## Related

- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the compare-and-set and the stores, extended here
- [ADR-0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md): the config and its schema
- [ADR-0025](0025-routes-and-sources-go-at-once-within-limits-and-a-session-keeps-where-it-stands.md): the limits capped at four times their default
