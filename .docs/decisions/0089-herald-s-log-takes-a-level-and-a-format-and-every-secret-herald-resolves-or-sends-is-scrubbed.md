# ADR-0089: herald's log takes a level and a format, and every secret herald resolves or sends is scrubbed

**Status:** Accepted
**Date:** 2026-11-27

## Context

herald writes what a run did a line each, as words, on every host: the
CLI's commands, `herald server` and `herald lambda`, a wasi:http component,
a Fastly Compute service and a JavaScript host. Every line is written, so
an operator who wants to see only what went wrong reads past every success,
and a log collector that reads JSON reads herald's lines as plain text.

[ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md)
scrubs the secrets the config names from the CLI's output, and leaves a
value shorter than 8 characters as it is. A token herald mints as it runs,
such as a GitHub App's installation token, is named by no config and was
known to no scrubber, and a host other than the CLI scrubbed nothing.

## Decision

1. **Each line of the log has a level.** `debug` is what herald saw as it
   went, `info` what herald did, `warn` what it left undone, passed over or
   refused, and `error` what stopped or failed. No `debug` line carries
   what a private repository holds, and each passes through the scrubber
   as every line does; the CLI's runs say first, at `debug`, how many
   sources, routes and destinations they read and which store they keep
   their records in.
2. **`[log] level` is the least grave line the log writes.** `info`, every
   line but `debug` ones, where absent. `HERALD_LOG_LEVEL` takes its place,
   and a command's `--log-level`, and the Action's `log-level` input, take
   the place of both.
3. **`[log] format` is how the log writes each line.** `text`, the words as
   herald writes them, where absent; `json`, a JSON object a line holding
   its `time`, in RFC 3339 and UTC by the host's clock, its `level` and its
   `message`. `HERALD_LOG_FORMAT` takes its place, and
   a command's `--log-format` takes the place of both. A variable naming no
   level or format is a mistake the run stops on.
4. **What the log is.** The lines `herald announce`, `poll`, `housekeep`,
   `withdraw`, `server` and `lambda` write of what they did and why they
   stopped, and every line a component, a Compute service and a JavaScript
   host write of the requests they answer and the work those leave. A
   command that answers a question, such as `herald doctor` or
   `herald queue`, prints its answer as it prints it, by its own
   `--format`, and a dry run prints its requests as they are.
5. **Every secret herald resolves or sends is scrubbed.** One scrubber per
   run, or per composed host, holds every secret read through the host's
   secrets and every secret a request carries in its URL or a header, a
   token minted at run time among them, and everything herald writes passes
   through it, on every host: the log, the CLI's output and errors, and an
   answer a server echoed back. A secret read is held for as long as the
   scrubber lives, and a value sent and then read is held as read. A
   token minted with an expiry is held until ten minutes past it, and a
   value of no known expiry known only by its sending until a day after it
   was last sent. Past `[log] scrub_warn_past` values held, 4096 where
   absent, every value is still held and scrubbed, and the log says so
   once, naming the setting. An operator raises it where a run sends that
   many secrets by design, or lowers it to hear sooner; no live credential
   is dropped at it either way.
6. **`[log] scrub_from` is the fewest characters a value holds to be
   scrubbed.** 8 where absent, as a shorter value would rewrite ordinary
   words. On GitHub Actions every secret is masked whatever its length.
7. **This supersedes in part** ADR-0007's scrubbing: what the scrubber
   knows, and the 8 characters it leaves a value below, are this decision's.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **Scrub only the secrets the config names** | A token minted at run time, or a value read from a file the config names, would reach the log whenever a server echoed it. |
| **Register every secret as it is made, process-wide** | Every test in a process would share one scrubber, and a server would hold every value it ever made. |
| **Hold the last values sent up to a count** | A busy server could push out a token that is still valid, and stop scrubbing it. |
| **A fixed count to warn at** | An operator whose runs send that many secrets by design would read the warning on every run, and one wanting to hear sooner could not. |
| **A fixed least length** | An operator holding a short credential could not have it scrubbed. |

## Consequences

- A log collector reads herald's lines as JSON where `[log] format` says so,
  each with its time and its level.
- `level = "warn"` leaves the log of a quiet run empty.
- A short value is scrubbed only where `scrub_from` is lowered to it, at the
  cost of the words it rewrites.

## Related

- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): secrets, masking and the scrubbing this extends
- [ADR-0017](0017-herald-runs-wherever-code-runs-from-one-native-binary-and-one-wasm-module.md): the hosts that write the log
- [ADR-0041](0041-an-owner-learns-of-its-repositories-by-push-and-reads-as-a-github-app.md): the installation tokens herald mints
