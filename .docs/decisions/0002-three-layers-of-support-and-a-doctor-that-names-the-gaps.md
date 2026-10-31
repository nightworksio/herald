# ADR-0002: Three layers of support, each a superset of the one before, and a doctor that names what a layer cannot serve

**Status:** Accepted
**Date:** 2026-10-31

## Context

Most projects that want their releases announced do not want to run a
service for it. A bot that has to stay online is a bot somebody has to keep
online. herald therefore needs nothing long-lived: a GitHub repository is
enough to start, and more capability comes from adding an endpoint, not from
replacing the setup.

Some features cannot work without an endpoint. A slash command is a request
Discord sends to herald, and an Actions run has no address to receive it on.
A config written for a serverless instance and then run as an Action would
silently lose those features. The adopter would find out from a channel where
nothing happened.

## Decision

1. **There are three layers, and each serves everything the one before it
   serves.**
   - **Actions-only.** The CLI runs in GitHub Actions. A workflow triggered
     by a forge event announces it, and a scheduled workflow polls feeds,
     registries and other repositories' releases and runs thread
     housekeeping. Any workflow step or script can announce an app's event
     with `herald announce --event`. There is no inbound endpoint.
   - **Serverless endpoint.** One of the serverless drivers (ADR-0009) adds an
     HTTP endpoint. That endpoint gives:
     - the HTTP API through which any app announces;
     - verified forge webhooks, in place of Actions;
     - Discord's and Slack's HTTP interactions;
     - a Matrix application service for commands;
     - herald's own ActivityPub actor (ADR-0004);
     - the platform's cron for polling and housekeeping.
   - **Container.** The same endpoint as a signed image (ADR-0009), with SQLite
     on a volume or Postgres, and polling and housekeeping on its own
     schedule.

2. **What each layer serves.**

   | Feature | Actions-only | Serverless | Container |
   |---------|--------------|------------|-----------|
   | Releases of the repository the workflow runs in | the Actions event | a verified webhook | a verified webhook |
   | Releases of other repositories, registries and feeds | a scheduled workflow polls | the platform's cron polls | its own schedule polls |
   | An app's event | `herald announce --event` | the HTTP API | the HTTP API |
   | Thread housekeeping: archive, lock, pin the latest | a scheduled workflow | the platform's cron | its own schedule |
   | Slash commands and buttons on Discord and Slack | no | yes | yes |
   | Commands on Matrix | no | yes | yes |
   | herald's own ActivityPub actor | no | yes | yes |
   | Mentions of opt-in roles and subscribers | yes | yes | yes |

   The stores each layer can use are listed in ADR-0005.

3. **The CLI is layer one wherever it runs.** On a GitHub runner it also
   reaches the Actions-only stores. On another CI or a cron job, it serves
   the same features with the stores that need no Actions environment.

4. **`herald doctor` names every feature the config asks for that the layer
   cannot serve, before anything posts.** It reads the config and the layer.
   The layer is the one its driver serves, or the one `--layer
   actions|serverless|container` names, so a config can be checked for a
   target from anywhere. For each gap it prints:
   - the feature;
   - the config key that asks for it;
   - why this layer cannot serve it;
   - the first layer that can.

   It also names:
   - what a webhook destination cannot do: crosspost, open a thread from a
     message on Slack, read back posts for destination markers, and receive
     interactions (ADR-0004);
   - what the chosen store does not offer for the config (ADR-0005);
   - a store whose credentials need more than the job's token, such as the
     repository-variables store, which `GITHUB_TOKEN` cannot write.

   It exits 0 when the layer serves everything, 1 when anything is
   unservable, and 2 when the config is invalid. `--format json` prints the
   same findings as JSON. `--online` also resolves every secret, asks each
   destination whether its credentials are accepted, and checks that the bot
   holds the permissions each route needs.

5. **Every command runs doctor's checks first.** `announce`, `poll` and
   `housekeep` print the same gaps, as warnings on Actions, and then serve
   what this layer can. One config can then run as an Action for releases
   and on a Worker for interactions, and each run says what it is leaving to
   the other. A config error is not a gap. It stops the command before
   anything posts (ADR-0006).

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **A long-lived bot as the only runtime** | Interactivity becomes easy, and everything else becomes hard. Adopting herald would mean running a process, and most adopters want a release posted, not a service to operate. |
| **Separate configs per layer** | The same project would describe its routes twice, and the two copies would drift. One config, with doctor naming what a layer leaves out, keeps one description. |
| **Commands fail on any unservable feature** | A config shared between an Action and a Worker would fail on the Action forever, for features the Worker serves. Doctor fails, because it is a check. The commands warn and serve. |
| **Silently skipping unservable features** | That is the failure this ADR exists to prevent: a setup that looks right and does nothing. |

## Consequences

**Adopting herald needs only a repository.** A workflow, a `herald.toml` and a
secret are enough to announce releases, threads included.

**Growing is additive.** Adding a Worker adds interactions and keeps every
route the Action served.

**A gap is never discovered in a channel.** Doctor names it in CI, and every
run names it again.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): the drivers that serve each layer
- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): webhook limits, interactions and the ActivityPub actor
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the stores each layer can use
- [ADR-0009](0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md): the serverless drivers and the image
