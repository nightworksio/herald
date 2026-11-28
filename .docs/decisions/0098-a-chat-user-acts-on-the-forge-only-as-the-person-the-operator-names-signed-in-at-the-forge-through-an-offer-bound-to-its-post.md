# ADR-0098: A chat user acts on the forge only as the person the operator names, signed in at the forge, through an offer bound to its post

**Status:** Accepted
**Date:** 2026-11-27

## Context

herald's commands and its subscribe button read and never write to a
forge (ADR-0016). Who asks is a chat id; nothing ties it to a forge
account. One write path exists: `herald rerun --apply` (ADR-0068), with a
separately named write credential, an App token minted last and narrowed,
an operator who signs in through the forge's device flow with read-only
scopes, the least role re-read right before the call and an `Actions`
record written before it; `herald merge-queue` takes a writer's gesture on
the forge (ADR-0087 decision 3). GitHub refuses an author's approval of
their own pull request; a GitHub App's user access token acts for the user
with only what both the user and the App may do, for 8 hours by default
(<https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app>).

## Decision

1. **Buttons a route offers.** `act = [...]` on a route, none where
   absent: `"rerun"` (a failed run), `"queue"` (add to or remove from a
   merge queue the repository has, ADR-0087), `"snooze"` (a nudge, for a
   day or a week) and `"review"`. Each button but Review opens the
   platform's confirm dialog naming the repository, number, short head and
   what will happen. Answers go to the presser alone, and once done herald
   adds one line in the post's thread. Matrix, which has no buttons, takes
   `!herald rerun`, `!herald queue`, `!herald dequeue` and
   `!herald snooze 1d` as replies to herald's post.
2. **A press names an offer, never a target.** A button's id is
   `herald:<verb>:<offer id>`, 22 random base64url characters. The offer,
   written when the post is made, holds the destination, workspace or
   guild or room, channel, message id, verb, repository, number, head, run
   and job, and an expiry (`offer_lasts`, "7d" where absent, at most "30d",
   never past the platform's own limit). A press is refused unless its
   channel and message are the offer's, its workspace (Slack `team` and
   `enterprise`, Discord `guild_id`) is the destination's, and the offer
   is unexpired. Only a press that passed the platform's verification
   reaches this (ADR-0016 decision 2).
3. **The operator names each person.** `[people.<name>]` holds `chat =
   { <destination> = "<platform user id>" }` and `forge = { <host> =
   "<login>" }`, exact ids (a Matrix id with its server; a Slack id with
   its team, and its enterprise where the destination names one). Nobody
   links themselves. A presser with no entry is refused: "herald does not
   know who you are on GitHub; ask the operator to add you". An email
   address is never matched.
4. **Every press but Review signs in at the forge.** The answer shows a
   device-flow code and the forge's own page; the presser signs in with
   ADR-0068's read-only scopes and presses Done. herald polls the token
   once per Done press, reads the signed-in login and refuses unless it is
   the presser's `[people]` login, so a code an attacker sends a
   maintainer yields the maintainer's login, which is not the attacker's.
   The device code is kept sealed for its `expires_in` alone.
5. **Authorised right before the step.** The presser's role on the
   repository is read with that token and must hold the verb's least role:
   re-run `write` (ADR-0068); queue the queue's `least` (ADR-0087), the
   author may remove their own; snooze the author, a requested reviewer or
   `write`. Every ADR-0068 and ADR-0087 check then runs unchanged. A head
   that moved since the post is refused, and the current head named.
6. **Which credential writes.**
   - Re-run: the source's `write` credential, narrowed to `actions: write`
     on one repository (ADR-0068).
   - Queue: a chat gesture, `enter = { chat = true }` beside `label` and
     `comment` (extending ADR-0087 decision 3): herald records it in the
     `Actions` store with the presser's login, platform, time and the
     offer's head as the vouched head, and `herald merge-queue --apply`, or
     the server where `applied_by = "server"`, takes it as a forge gesture,
     re-reading the presser's role before each step. herald puts no label
     or comment on the forge to stand for a person.
   - Snooze: nothing on the forge.
   - Review, `approve = "page"` (the default): a link to the pull
     request's review page; herald writes nothing.
7. **Approving as the person, a setting off by default.** With
   `approve = "as-person"` on a GitHub source, Approve signs in through an
   App of its own, `approve_sign_in = { client_id }`, whose installation
   and App permissions herald reads as ADR-0068 decision 4 does and which
   must hold `pull_requests: write` and `metadata: read` and nothing else.
   This supersedes in part ADR-0068 decision 4, under which a sign-in
   token only reads: here, and only here, the sign-in token writes.
   - Scope: one `POST /repos/{o}/{r}/pulls/{n}/reviews` with
     `event = "APPROVE"` and `commit_id` the offer's head; it reaches only
     what both the person and that App may do.
   - Lifetime: held in memory for that one call and dropped; never
     stored, sealed, logged or put in an offer. GitHub revokes a user
     token only for a client holding the App's secret, which herald does
     not hold, so it lives until GitHub expires it (eight hours) or the
     person revokes it; doctor says so.
   - Checks before use, each refusing with no call made: the token's
     answer names an expiry, so the App has user token expiry turned on;
     the signed-in login is the presser's `[people]` login; the App's
     permissions are those two; the pull request is open, its head is the
     offer's, and the
     presser is not its author; the presser's role holds `write`.
   - On GitLab, a setting chooses how Approve acts as the person, and
     where it is unset herald approves as itself, as before: an operator
     turns one of two modes on, never both, and neither is a default.
     - `stored-token`: each person stores a fine-grained personal access
       token for herald, limited to the permission "Merge Request", action
       "Approve", on the chosen projects
       ([fine-grained access tokens, REST](https://docs.gitlab.com/auth/tokens/fine_grained_access_tokens_rest/));
       GitLab offers such tokens on every tier from 19.2, made by hand,
       living until midnight UTC on their expiry date, 365 days at most
       where the instance does not set another bound
       ([fine-grained access tokens](https://docs.gitlab.com/auth/tokens/fine_grained_access_tokens/)).
     - `sign-in`: the person signs in to GitLab through OAuth with the
       `api` scope at the press, and the token is held in memory for the
       one approval and dropped, never stored.
     - Either way the one call is `POST /projects/:id/merge_requests/:iid/approve`
       with `sha` the offer's head, which GitLab refuses with `409` where
       the head moved; it needs the person to be an eligible approver,
       takes `approval_password` where the project asks for
       re-authentication, and fails where SAML sign-in is enforced
       ([merge request approvals API](https://docs.gitlab.com/api/merge_request_approvals/),
       "Approve merge request"). The checks before use are GitLab's
       counterparts of GitHub's above.
     - Not decided, and built only once the owner answers: where a
       person's stored token is sealed and under which key; who may make
       herald use it (the person alone, through a press their verified
       chat identity makes); how it is revoked (by the person in GitLab,
       and in herald by a command that deletes it); what a store that
       leaks exposes (sealed tokens and their projects, readable only with
       the store's key); whether the sign-in mode may use the `api` scope,
       which reaches all the person may do through the API for the life of
       the token, and whether herald revokes that token after the call;
       and how an instance's own bound on token lifetime is read. Nothing of either mode is built until then.
   - On Forgejo, Approve is the review page.
8. **Replay and double presses.** The offer moves from offered to pressed
   by compare-and-set; a second press is told it is in hand; ADR-0068's
   record per job and head stays the last guard.
9. **Refuse when unsure.** A read that fails, a missing mapping, a login
   that differs, a role below the least, an expired offer, a store that
   keeps no records, or a destination without interactions: refused in
   plain words, no write credential read.
10. **Every press is recorded** in the activity record (ADR-0099): time,
    destination, workspace, chat id, mapped and signed-in login, verb,
    repository, number, head, job, and outcome or refusal; never a token.
11. **Privacy.** Buttons are added only on routes the event reaches; the
    thread line names the presser's forge login only on private routes or
    where `act_says_login = true`.
12. **Hosts.** Presses need an interaction transport: `herald server`, or
    the serverless and `wasi:http` hosts over HTTP. Actions has no
    endpoint, and doctor says so. Doctor names each host that reads a
    `write` credential for presses.

## Alternatives

| Option | Why not |
|--------|---------|
| **People link themselves by a page on herald's server** | herald's server would hold an OAuth client secret, and a tricked maintainer could still approve the link. |
| **People link themselves by device code** | A maintainer who enters a code an attacker sent links the attacker's chat account to their forge account. |
| **One sign-in a day** | A chat account taken over could act for the rest of the day. |
| **No sign-in** | A chat account taken over acts within its person's role. |
| **herald approves with its own credential** | The forge would record herald, and herald's access would stand in for the person's. |
| **No Approve button** | A link to the review page costs nothing and saves a search. |

## Consequences

| Threat | What holds |
|--------|------------|
| Forged press | Platform signature, replay window, admitted once |
| Copied or forwarded button | Offer bound to channel, message and workspace |
| A crafted value naming another pull request | The press names only an offer |
| Chat account taken over | A sign-in at the forge for each press |
| Device-code phishing | Signed-in login must be the presser's `[people]` login |
| Presser without the role | Role read right before the step |
| herald's credential standing in for a person | Approve by the page or the person's own token; a queue gesture names the person |
| Replay or double press | Offer and ADR-0068 records by compare-and-set |
| Head moved after the post | The offer pins the head |
| Federated Matrix impostor | Exact Matrix ids in `[people]` |
| A person's approval token stolen | Held in memory for one call; reaches only `pull_requests: write` where both person and App hold it, until GitHub expires it |

## Related

- [ADR-0016](0016-commands-are-carried-every-way-a-destination-offers-and-answered-from-what-herald-keeps.md): commands; extended with buttons that act
- [ADR-0068](0068-an-operator-signed-in-at-the-forge-re-runs-a-planned-job-once.md): the write path; decision 4 superseded in part here
- [ADR-0087](0087-a-pull-request-a-writer-queues-goes-to-its-forge-s-merge-queue-or-herald-s-own-proves-it-on-its-base-and-merges-it.md): gestures; decision 3 extended here
- [ADR-0099](0099-herald-keeps-what-it-did-with-each-event-never-what-the-event-said-and-shows-it-to-the-operator-alone.md): the activity record
