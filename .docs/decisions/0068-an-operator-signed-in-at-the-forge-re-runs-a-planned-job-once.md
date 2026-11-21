# ADR-0068: An operator signed in at the forge re-runs a planned job once

**Status:** Accepted
**Date:** 2026-11-20

## Context

A rule plans a re-run of a flaky check (ADR-0065). This is the first
place herald writes to a forge. The user decided: the re-run lives on a
rule, never a global switch; a standing rule only plans; a re-run needs a
separately named write credential, sent to the source's own API origin
alone and never the read credential in any form; and an apply needs an
operator signed in through the forge's device flow, whose token only
reads, so an apply never runs unattended. The checks that decide a call
are made immediately before it, not when the plan was made, and a job is
re-run at most once.

## Decision

1. **Plan and apply are one command, and the call re-runs one job.**
   `herald rerun [rule]` lists each plan, read now, with the command
   applying it; it reads no write credential. `--apply --job
   <job>@<head>` re-runs the jobs named. An apply reads the plans again
   first and takes only a job they still hold on the head named: a plan
   and an apply are compared by job id and head. The call is the
   narrowest the forge has: GitHub's `POST
   /repos/{owner}/{repo}/actions/jobs/{job}/rerun`, GitLab's `POST
   /projects/{id}/jobs/{job}/retry`; never every failed job of the run.
2. **The write credential makes the call.** `[sources.<name>] write` is
   a token (`{ token = { secret } }`) or a GitHub App (`{ app }`), apart
   from `token` and `app`. It is never among the secrets a poll, a
   report or the server reads; only an apply reads it, and the call is
   its one use. It is sent through a connection bound to the source's
   API origin alone, which follows no redirect. The config does not load
   where `write` names the secret the source reads with, the read App's
   key, the read App under another table (the same issuer), or an App
   minting at another origin than the source reads; nor where its
   secret, or its App's key, is named anywhere a run of any use reads
   (any source, destination, App a source reads as, webhook, the static
   log's token, or the store), another source's `write` names it at
   another origin or as another kind of credential, or any source reads
   as its App under any table (by name or issuer). An apply refuses a
   write secret holding the read secret's value under another name, and
   a write App that is an App herald reads as: the minted token's
   installation id is compared with the installation id of every minting
   any source reads through, and its App id with each of their App ids.
   GitHub names an installation's App id in the lookup herald makes
   before a mint (`GET …/installation`); where no lookup is made, as for
   an installation named by its id, herald asks `GET
   /app/installations/{id}` with that App's own JWT. The App id is the
   identity compared: GitHub gives each App one id, whichever key signs
   for it, whichever table names it and whichever account it is
   installed on, and herald reads it from GitHub's answer to the App's
   own JWT, not from the config; so the same App under another table,
   with another key, or installed on another account is caught.
3. **A write App's token is narrowed and minted last.** It is minted for
   one call, after every check of point 4 passed and after herald's
   record of point 5 is written, with the body
   `{"permissions":{"actions":"write"},"repositories":["<name>"]}`, and
   GitHub's answer must say the token holds `actions: write` (beside
   `metadata: read`, which GitHub adds, and nothing else) on that one
   repository alone; a token holding more, or naming another
   repository, is never used: herald revokes it at once (`DELETE
   /installation/token` carrying that token, over the write connection
   alone, sent whatever the budget holds, as the operator token's
   revocation is), and a failed revocation is named in the refusal. It is never kept in a store. A mint that
   fails or holds more rewrites the record as unsent, so a later apply
   may ask again. An operator refused by a check, an
   operator who declines the sign-in, a moved head, a job already
   recorded and a lost race all end with no request carrying the write
   App's JWT or token. A write token the config names is read from the
   host's secrets before the sign-in, so its value can be compared with
   the read credential's; it is sent only in the call.
4. **The operator signs in and only reads.** The sign-in, and every
   read of a write credential, are made only at the sources of the jobs
   named that the fresh plan holds; the plan itself reads with the read
   credentials, as a listing does. `sign_in` names the client id; it may not be
   the client id of any App a `write` names. `scopes` may name only
   scopes that read, each forge's list kept in one place (GitHub:
   `read:user`, `read:org`, none by default; GitLab: `read_api`,
   `read_user`, `openid`, `profile`, `email`, `read_api` by default);
   any other does not load. At sign-in herald checks what the token may
   do, and refuses it otherwise. No acceptance rests on a read that
   failed. On GitHub, what the token is decides, by the prefix GitHub
   documents for it, kept in one table: the scopes header cannot tell an
   OAuth token holding no scope from a GitHub App's user token, the
   prefix can.
   - GitLab: every scope `/oauth/token/info` names is on the list.
   - GitHub, `gho_` (an OAuth application's token): `GET /user` must
     answer with `X-OAuth-Scopes`, and every scope it names is on the
     list; where the header is absent the token is refused. Its
     installations are not read.
   - GitHub, `ghu_` (a GitHub App's user token, which holds what its
     App holds): whatever the header says, herald reads `GET
     /user/installations` with the token, through the budget, every
     page of 100 (at most ten), then `GET /apps/{app_slug}` once for
     each App they name (the slug through the one segment encoder, so a
     dot segment is refused), and refuses the token where an
     installation or an App holds a permission whose level is not
     `read`, an installation is the write App's (its `app_id` or
     `client_id` is the `app_id` or `client_id` the write App's table
     names), an installation names a `client_id` other than
     `sign_in.client_id`, none names its App, or any of these reads fails (an error answer, a lost answer,
     the budget ended, more pages than herald reads, an answer it cannot
     read). An App's `permissions` there hold its account permissions
     beside its repository and organization ones (GitHub's public Apps
     answer `emails: read` there), so `git_ssh_keys: write` on the
     sign-in App is refused
     (<https://docs.github.com/en/rest/apps/apps#get-an-app>: works with a
     GitHub App user access token; `permissions` is a map of each
     permission to its level).
   - GitHub, any other prefix: refused, the refusal naming no part of
     the token.
   A private GitHub repository needs a GitHub App's client id: its user
   token holds that App's read permissions, and the App must be
   installed on the repository's account.
   `web` defaults to the API's web root, for github.com, GHE.com, GitHub
   Enterprise Server (`/api/v3` stripped, the host kept) and
   self-managed GitLab alike. herald shows the page the forge names to
   enter the code at only where it is on that web root over https, and
   every line `herald rerun` and `herald queue` print, through the CLI's
   one writer of a line, drops every control character and every format
   character of Unicode 16 (category Cf: bidi overrides and isolates,
   zero-width characters, tags, the soft hyphen and the rest), and turns
   a line or paragraph break (U+2028, U+2029 among them) into a space;
   the set is kept in one place. JSON output (`herald queue --format
   json`, `herald doctor --format json`) escapes each of those characters
   as `\uXXXX` instead (a surrogate pair above U+FFFF), so it reads back
   unchanged. Forgejo has no
   device flow and no re-run API, so its sources take neither key. The
   operator's token is an `OperatorToken`: every `Debug` and `Display`
   names it alone, it implements no serialising (so no record, answer
   or log line can hold it), it cannot be cloned, its value is zeroed
   when dropped, as is the raw answer it was read from. It reads the
   operator's login and their role on each repository, nothing else.
   When the apply ends herald asks GitLab to revoke it (`POST
   /oauth/revoke` as a public client) and drops it, whatever came after
   it arrived: refused at sign-in, refused by a check, or used for a
   call; each token is revoked in one place, at the apply's end. The
   revocation is sent only where the apply ends as it should: on a panic
   or a Ctrl-C it is not. GitHub revokes a token only for a client
   holding its secret, which herald never holds. So a GitLab token
   herald did not revoke lives until it expires (two hours), a GitHub
   App's user token until it expires (eight hours where the App has user
   token expiry turned on), an OAuth application's until the operator
   revokes it at the forge.
5. **Checks right before each call**, in order, each refusing in plain
   words with no request carrying the write credential made: the
   operator holds `sign_in.least` ("write": GitHub write or GitLab
   Developer, or above) on the repository; the pull request is open
   with the head named; the job is of the run named, ran on that head,
   reads as failed, and neither it nor its run was re-run (GitHub's
   `run_attempt` of the run; GitLab's retried jobs of that name, every
   page of the pipeline's jobs read, refused where more pages remain
   than herald reads).
6. **The record comes before the call.** A new `Actions` store port keeps
   one record per job and head (operator login, the credential's name,
   time, re-runs seen, outcome), written as new with compare-and-set
   before the call: a crash after it leaves a record that refuses every
   later apply, and of two applies racing only the first writes it. The
   outcome is written after the answer. A lost answer (a transport
   error, a redirect, 408, 429 or 5xx) is never asked again: herald
   reads the job back and records it re-run or unsettled. Only a refusal
   (another 4xx) or a call never sent (the run's budget ended, or the
   App's token could not be minted or held more) may be asked again. A
   record that could not be rewritten after the call, or as unsent,
   still says the call is being made, and the apply says so: herald
   never asks for that job again. The markers store keeps no records,
   so an apply on it refuses.
7. **Every request but a revocation is charged.** The plan, the sign-in
   and the installations and Apps read there, every check, each App's
   lookup and minting, and the call go through the run's budgeted
   handle; the operator token's revocation and that of a token minted
   too wide are sent whatever the budget holds. Each path segment goes through the one encoder, and a run's
   and a job's id are numbers.
8. **Doctor says whether an apply can work** for each source of a rule
   planning re-runs, and why not: no write credential, no sign-in,
   Forgejo, or a store that keeps no records.

## Threats

| Threat | What holds |
|--------|-----------|
| A stolen write credential alone | It re-runs without herald: herald's gate is not the forge's. Doctor and the docs say to give it nothing but re-running (an App installation with `actions: write` alone; a GitLab Developer token on the projects concerned). A write App's minted token holds `actions: write` on one repository for one call. |
| A stolen operator token alone | An OAuth token's scopes, as GitHub or GitLab names them, only read; a GitHub App's user token reaches only installations and an App whose permissions, its account permissions among them, only read, none of them the write App's; herald never calls with it. On GitLab herald revokes it when the apply ends normally, whatever came of the apply; on a panic or a Ctrl-C, and always on GitHub, it lives until it expires or the operator revokes it. |
| An operator token that could write | A scope outside the read-only list does not load and is refused at sign-in. A GitHub token is held to the rule of its documented prefix: an OAuth token (`gho_`) whose scopes GitHub does not name is refused; a GitHub App's user token (`ghu_`) is refused where an installation or its App holds a permission that is not `read` (`git_ssh_keys: write` would let it add an SSH key and push as the operator), an installation is of the write App or names another client id than `sign_in.client_id`, none names its App, or any of those reads fails; a token of any other prefix is refused. A `sign_in.client_id` that is a write App's does not load; a write App named by `app_id` alone is caught at sign-in by its installations. |
| The write secret sent elsewhere | It is named by its `write` alone: any other key naming it, another source's `write` at another origin, or a source reading as its App does not load. Its connection reaches the source's API origin alone and follows no redirect; an App minting elsewhere does not load. |
| The read App under another table, with another key, or on another account | Before the call, the minted write token's installation id is compared with every read minting's, and its App id, which GitHub names, with every read App's. |
| A mint holding more than it was asked for | GitHub's answer must name `actions: write` (and `metadata: read`) on the one repository alone; otherwise the token is never used, is revoked with itself at once over the write connection, whatever the budget holds (a failed revocation is named in the refusal), and the call is recorded unsent. |
| A forge that lies about the job | herald re-runs only a job the forge itself names as failed on the head, of the run named, never re-run; the worst case is one re-run of that job. |
| A forge page or name that would mislead the operator | The sign-in page is shown only on the sign-in's web root over https; control and format characters from the forge, a bidi override among them, never reach the terminal. |
| A plan replayed against a moved head | The apply re-plans and refuses a job not held on the head named, then reads the pull request's head and the job's head right before the call. |
| Two applies racing | The record is written as new with compare-and-set before the call; the second conflicts and makes no call and mints no token. |
| A crash before the record | Nothing was asked. |
| A crash after the record, before or during the call | The record says the call may have been made; every later apply refuses it and says to re-run by hand if it never started. |
| A crash after the call, before the outcome | As above: never asked again. |
| A lost or redirected answer | Not followed, not asked again; the job is read back. |
| An operator without the role, or who declines | Refused before any request carrying the write credential; a token that arrived is revoked on GitLab all the same. |
| A private repository in the output | `herald rerun` prints a private repository's plans, check names and run pages, as `herald queue` prints its rows; plans reach private routes alone (ADR-0065). A CI job's log may be public. |

## Consequences

- The forge's audit log names the write credential (an App or a token's
  owner), not the operator; herald's record of each re-run is the trace
  naming them.
- A job whose call was lost or interrupted is re-run by hand.
- The `Actions` port is kept by every records store and conformed like
  the others; the markers store answers unsupported.
- ADR-0065's planned call becomes the one job's: a plan carries the job
  of the check with its run, on GitLab too, so a GitLab plan reads each
  pipeline's failed jobs and costs at most `runs + 1` requests, as a
  GitHub plan does. The `rerun` event is still announced once per run
  and head of a check; ADR-0065's text of the call and of the GitLab
  cost reads as this note says.
