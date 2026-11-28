# ADR-0091: `herald init` writes a first config and its workflow, and posts one test message

**Status:** Accepted
**Date:** 2026-11-27

## Context

A newcomer reads the README, writes `herald.toml` by hand, writes a
workflow, stores secrets and runs `herald doctor`. Every piece exists:
`herald setup merge-queue` asks questions that are each a flag, takes
defaults with `--yes`, prints with `--print`, loads the candidate before it
writes and edits with `toml_edit` so comments survive (ADR-0087 decision
15); `herald doctor --online --post` posts one test message per route
apart from every record a real announcement reads; a destination's `Setup`
is a trait without I/O (ADR-0024 decision 5). `herald init` printed a new
marker secret (ADR-0011 decision 3). No release of herald exists, so the
GitHub Action builds herald from its own checkout at a commit.

A Slack or Discord webhook, a Teams workflow webhook and email cannot read
herald's posts back, so the markers store cannot keep "sent once" for them
(ADR-0011) and doctor names the gap.

## Decision

1. **`herald init` is the first-run wizard.** It asks, writes a config and
   a workflow, and offers one test post. Printing a new marker secret is
   `herald token marker`, beside `herald token new`; this supersedes in
   part ADR-0011 decision 3, which named `herald init` for it.
2. **It refuses where the config exists.** Where `--config` (`herald.toml`
   where absent) exists, it writes nothing and says to add to it with
   `herald setup …` and to judge it with `herald doctor`.
3. **The questions, each a flag, each with its recommended default.**
   - forge (`--forge github|gitlab|forgejo`) and repository
     (`--repository owner/name`), read from the `origin` URL in
     `.git/config` as a file (no `git` process, no shell); an unknown host
     or no remote is asked, never guessed. A host that is not the forge's
     public one is written as the source's API base, so GitHub Enterprise
     Server, GHE.com, GitLab self-managed and every Forgejo host are first
     runs too;
   - chat (`--chat <kind>`): the destination kinds the build carries, read
     from the destination registry; the CLI names none;
   - that chat's own questions, asked through a hook without I/O on
     `DestinationKind`: its zero-setup variant first, the place it posts
     to, the secret's name with its recommended default
     (`HERALD_<KIND>_WEBHOOK` or `_TOKEN`), and its enterprise or sovereign
     variant where it has one. This fills ADR-0024 decision 5's installer
     clause: the questions sit at the kind, since no table exists yet;
   - what to announce (`--announce`, several allowed): the shipped presets
     (ADR-0092), one `[presets.*]` table each;
   - where it runs (`--runs actions|forgejo-actions|gitlab-ci|none`): the
     forge's own CI where absent.
   `--yes` takes every default and writes without asking; `--print` prints
   every file and writes none; three refused answers or Ctrl-C write
   nothing, as in the merge-queue wizard.
4. **The wizard picks the store.**
   - A bot variant: `markers`, which needs nothing.
   - A webhook or email variant on GitHub: `actions-cache`, sealed with a
     key the wizard makes from the system's random generator and prints
     once, for the adopter to store as `HERALD_STORE_KEY`.
   - A webhook or email variant on GitLab or Forgejo: the git-branch store
     of that forge, with a token that may push named as a secret.
   It says the choice and its reason in one line.
5. **What it writes, after one confirmation that shows each file in
   full.** `herald.toml` (the `#:schema` line, `version = 1`, `[store]`,
   one `[destinations.*]`, one `[sources.*]`, the preset tables) and one
   workflow file, never over an existing one (refused, with `--print`
   offered):
   - GitHub: `.github/workflows/herald.yml`, on `release: published`,
     `permissions: contents: read`, `concurrency: herald`,
     `persist-credentials: false`, `nightworksio/herald@<commit>` with the
     secrets as `env`, and a `schedule` where a preset polls;
   - Forgejo: `.forgejo/workflows/herald.yml`, using the same Action by its
     full URL pinned by commit
     (<https://forgejo.org/docs/latest/user/actions/reference/>);
   - GitLab: a job in `.gitlab/herald.yml` included from `.gitlab-ci.yml`,
     running the release container image pinned by digest. No release
     exists, so `--runs gitlab-ci` is refused and says so; `--runs none`
     writes the config alone.
   No secret's value is written to any file.
6. **After writing it says, per secret,** its name, where the forge keeps
   it (GitHub repository secret, `gh secret set NAME`; GitLab masked CI/CD
   variable; Forgejo repository secret) and one link to herald's page on
   making that credential.
7. **The test post.** It shows the test message through `herald preview`'s
   text output (ADR-0095) and offers to post it: `herald doctor --online
   --post`'s trial, held to the config's first route, so one message is
   posted. The credential comes
   from the environment where its variable is set; otherwise, on a
   terminal, from a prompt that does not echo. The value is held in memory
   for that one post, joins the run's scrubber as it is read (ADR-0089),
   and is never written or printed. With `--yes`, or with no terminal, no
   prompt is shown: it prints the command to run later. A refused
   credential leaves the files written and names the secret to fix and the
   command to retry. The exit code is the doctor's.
8. **The native CLI alone runs it**, since it writes files. `--runs none`
   prints the `herald doctor --layer <host>` line for a host chosen by
   `--host`.

## Alternatives

| Option | Why not |
|--------|---------|
| **Keep `herald init` for the marker secret, and name the wizard `herald setup first-run`** | `init` is the name a newcomer types, and no release exists whose users rely on the old meaning. |
| **Add to an existing config** | A first-run tool that edits a working config can break it; `herald setup …` already adds to one. |
| **Steer every newcomer to a bot so `markers` always works** | Webhooks are the zero-setup way into Slack, Discord and Teams. |
| **Always ask which store** | The answer follows from the chat variant and the forge; asking puts a question to a newcomer that the wizard can answer. |
| **Environment only for the test post** | A newcomer who has the credential on the clipboard would have to export it first; a prompt that does not echo keeps it out of argv, files and the screen alike. |
| **A GitLab job that builds herald from source** | Minutes per run; the release image needs no build. |

## Consequences

- A first run is one command, a few answers, one secret stored, one test
  message seen.
- `herald token marker` is the marker secret's command.
- The git-branch store must keep records on GitLab and Forgejo for the
  wizard to pick it there.
- The GitLab writer runs once a release image exists.

## Related

- [ADR-0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md): the marker key, superseded in part here
- [ADR-0024](0024-a-host-is-described-once-and-every-driver-composes-through-one-composition.md): a destination sets itself up; extended here
- [ADR-0087](0087-a-pull-request-a-writer-queues-goes-to-its-forge-s-merge-queue-or-herald-s-own-proves-it-on-its-base-and-merges-it.md): the setup wizard's model
- [ADR-0089](0089-herald-s-log-takes-a-level-and-a-format-and-every-secret-herald-resolves-or-sends-is-scrubbed.md): the scrubber
- [ADR-0092](0092-a-preset-is-shipped-or-local-toml-a-table-adopts-by-name-and-overrides-key-by-key.md): presets
- [ADR-0095](0095-preview-renders-every-kind-as-its-destination-sends-it-reading-nothing-and-a-gallery-is-generated-from-it.md): preview
