# ADR-0067: A view shows its repositories' advisories to private routes alone

**Status:** Accepted
**Date:** 2026-11-20

## Context

A maintainer's board is more useful with the open security alerts of the
repositories it covers, but an alert names a weakness that is not yet
fixed, and its existence alone tells an attacker where to look. The user
decided that overviews are sections of a view, that the advisories
section is always private, that every choice of which repositories it
reads is a setting with the recommended one the default, and that a
listing which cannot be read is never shown as if clean.

## Decision

1. **A section of a view.** `[views.<name>] overviews = ["advisories"]`
   reads, beside the rows, the repositories' open security alerts: GitHub's
   Dependabot, code scanning and secret scanning alerts (GitHub Enterprise
   Server and GHE.com under their own API), and GitLab's detected and
   confirmed vulnerabilities (self-managed included).
2. **Which repositories.** `advisories_from = "owner"`, the default, reads
   an organization's or group's own listings, keeping only the advisories
   of repositories its membership record holds; a GitHub user and a
   GitLab user namespace have none, so their newest pushed members are
   read one by one. `"shown"` reads the repositories the view shows a
   pull request of; `"members"` the owner's newest pushed. One by one
   reads at most `advisory_repositories` (20, from 1 to 100), and every
   read at most `most_advisories` (1000, at most 10000). A one-repository
   source reads its repository. Forgejo raises no security alerts, so it
   is named unread with that reason. Only the repositories the view holds
   are read one by one, and only their advisories and their unread
   listings are kept: the view's `repositories` filter has one home,
   `View::holds`, which its rows, its capacity and its advisories all
   take. A view with a `repositories` filter never reads an owner's own
   listings, whose pages count the alerts of the repositories it hides
   too: `Advising::within` reads it as `"members"`, each repository it
   holds one by one, at most `advisory_repositories`, the rest named
   unread. It takes them straight from the membership record, the first
   by name, never through the owner's newest-pushed listing, whose order
   the hidden repositories take part in; capacity takes its members the
   same way. So what it shows is the same whatever the hidden
   repositories hold or when they were pushed to. Doctor says so, and
   what it costs.
3. **Unread is never clean.** A listing the forge refuses (401, 403) or
   does not find (404), or GitLab answering no vulnerabilities, is named
   unread with the permission it needs. A read that fails otherwise names
   the source unread with what stopped it, and so does the run's budget
   ending during the read: that never stops the report. A report round
   first sends every report's posts to the routes no advisories section
   can reach, then reads advisories, and only then sends the routes that
   can be shown them, so the reads spend nothing another route's post
   needs; they read only where some route can be shown them (or
   `herald queue --private` asks), leaving one request for each route
   still waiting. Every request goes through the run's budgeted handle,
   and a next page is asked only where it lies under the forge's API (its
   URL followed by `/`) and only as many pages as `most_advisories` can
   fill. Only advisories of a repository the view holds count toward
   `most_advisories`, so another repository's alerts in an owner's
   listing neither use it up nor push a held one out. Where a held
   repository's advisory was left out, the section says more were open,
   never how many; where an owner's listing ran past its pages, it names
   the listing unread with how many pages were read, as it cannot know
   whether the rest held one. An error quoting a vulnerability's page is
   never shown, as an owner's listing may name a project the view hides.
   Members past `advisory_repositories`, and members the
   view shows no open pull request of where it reads those alone, are
   named unread with how many were not read, and an owner whose members
   were never listed is named unread with that reason.
4. **What a row holds.** A row names its repository, the tool that raised
   it, its severity, what it is, its state, its page and when it opened.
   A secret scanning row names the secret's type alone: never the secret,
   its location or a snippet. GitHub's secret scanning listing is asked
   with `hide_secret=true`, so it answers no secret's value, and GitLab's
   listing asks no vulnerability's location but a dependency's package,
   so no secret's file is ever asked.
5. **Advisories reach private routes alone, and nothing widens that.**
   An advisory section reaches a route only where the route says
   `private = true` and herald has checked that its place is not publicly
   readable right now, or the operator has vouched for a place herald
   cannot check. Each kind of destination decides both halves, and a test
   lists every kind, so a new one cannot be added without a decision.
   Where it is static, the kind decides it at load: herald's own
   ActivityPub actor, a herald feed, a push to a peer that takes no
   private events, a Mastodon status posted `public` or `unlisted`, and a
   Discord crosspost post where anyone may read them, and such a route
   saying `private = true` does not load, whatever `[doctor]
   private_to_public` says. Where it is not, it is checked before each
   run's first advisory send, once a run for each place, through the
   run's budgeted handle, and doctor names the check and its requests:
   - a Matrix room is private only where joining takes an invite or a
     knock (`m.room.join_rules` `invite` or `knock`), or a membership of
     rooms that themselves take one, and its history is not
     `world_readable` (`m.room.history_visibility`): a `restricted` or
     `knock_restricted` room's `allow` list is read, at most five rooms,
     each a `m.room_membership` rule whose room's own join rules are read
     in turn and must be `invite` or `knock` (one level: a restricted
     room among them, an unknown kind of rule, more than five rooms, or a
     room the bot cannot read leaves the room unread); doctor's online
     check reads them too, and names a private route in a room anyone
     may join or read a mistake;
   - a Slack channel is private only where `conversations.info` says it
     is private and shared with no other organization through Slack
     Connect; a private channel an Enterprise Grid organization shares
     with its other workspaces counts as public, as members of every
     workspace of the organization may be in it, unless the destination
     says `org_shared = "private"`, the operator accepting the whole
     organization as its audience; the bot token needs `groups:read` (and
     `channels:read` for a public channel), and a refusal for a missing
     scope withholds the section and names the scope;
   - a Discord channel is private only where its server's `@everyone`
     may not view it, every other role that may view it is one the
     route's `viewers` lists by id, no member is let in by a permission
     overwrite of their own unless `viewers` lists that member's id too,
     and the server's onboarding (`GET /guilds/{id}/onboarding`) neither
     shows the channel to whoever joins nor lets them take a listed role.
     A role may view the channel as Discord resolves it: its own
     permissions with `@everyone`'s, Administrator granting all, then the
     channel's overwrite of `@everyone`, then its overwrite of the role;
     a channel synced with its category holds a copy of the category's
     overwrites, so the channel's own are read. Administrator roles and
     the bot's own role view every channel and must be listed too; the
     server's owner sees every channel and is not checked. With nothing
     listed, only a channel no role but `@everyone` may view passes. A
     role a bot or a linked-role connection hands out is the operator's
     to know: listing it accepts whoever holds it. Doctor's online check
     names each role or member that withholds the section by id and
     name. Onboarding the bot cannot read leaves the channel unread. The
     check takes three requests; a thread is not read, so a route to one
     is withheld the section;
   - Mastodon followers-only statuses are private only where the account
     approves each follower (`locked`), as anyone may follow an unlocked
     account.
   What the check accepts by design, as the operator's to govern: the
   Discord server's owner; whoever holds Manage Roles above a listed role,
   who can give themselves that role; roles handed out by reactions or a
   bot, and the server booster role, where `viewers` lists them; and on
   Mastodon, a deleted status, whose deletion other instances apply as
   they receive it, best effort.
   A place whose answer says anyone more may read it, or that cannot be
   read, budget ended included, is sent the withheld line, and a living
   post there that showed the section is brought to that line by its
   next post. A place is checked once a run for each place its route's
   options write; a route whose kind writes no place is checked on its
   own and never shares another's answer. A Discord or Slack route reached by a webhook cannot be asked:
   it is withheld the section unless it also says
   `vouch_unchecked = true`, the operator's word that its place is
   private, which loads only beside `private = true`, only on a route
   herald cannot check, and is listed by doctor for each route sent
   advisories on it. `Config::surface` decides the static half from the
   route's config, and the run's check refines a private route's surface
   to not private, so one function, `advises`, decides from the surface
   alone. No setting widens it (`[doctor] private_to_public` included),
   and neither does who may see the repository, a public repository's
   advisories included. A route that does not say `private = true` is
   never sent an advisory, its count, or that one exists.
6. **A mixed report withholds and says so.** A report whose view shows
   advisories beside other sections is rendered once for each audience.
   For a route that is not private, the advisory section is dropped
   before rendering and replaced by one line, the same for every view:
   "A section of this report was withheld from this route because it is
   not private." That line says neither which section nor what it held,
   and it is the same where the view found no advisory at all, so its
   presence leaks nothing. Doctor names each route that is not private
   on a report whose view shows advisories, saying it is sent that line
   in their place, and flags two routes that post to one place where only
   one of them is shown the section.
7. **Fail closed.** Where a route's privacy cannot be read, a route is not
   in the config, or a report cannot be split by audience, the advisory
   section is withheld. A board can hold a section only through
   `Advised::to`, given the surface `Config::surface` derives, or the
   withheld one; a template's dry render at load is given a made-up
   sample that holds nothing read.
   Templates read `view.overviews.advisories` only where it was kept, and
   `view.overviews.withheld`, the line, only where it was dropped.
8. **Every surface is decided.** One function, `advises`, takes the
   surface a view leaves herald through, and only a section it allows
   can be placed on a board. Each surface:
   - a route's post, and a living post's edit: the route rule, and only a
     living post (`living = true` on a destination that edits) carries the
     section, as herald can always find it again to take the section out;
     any other post of a private route is sent the withheld line, and
     doctor says the report must be living to carry advisories.
     `Surface::Route` holds both whether the route is private and whether
     its post is recallable, so `advises` decides from the surface alone;
   - `herald queue`, as a table or JSON: shown only with `--private`,
     which the operator types each time; it has no setting or environment
     variable, and in a public CI job's log it publishes them;
   - a grouped route's digest, a herald feed, the static log of feeds and
     a push route: never, as they carry the event and not its view;
   - a rule's nudges and re-run plans: never, as they show no view;
   - a run's told lines and a round's answer, doctor's output, and the
     app API, which serves no board or report: never. What a run tells of
     a route that is or was sent the section, and of every recall, is one
     fixed line: sent, or stopped, with no step, count of parts or
     deletions, nothing of whether the post changed, and nothing of why it
     stopped: no destination's answer, wait or count of requests.
   A surface not on this list gets nothing. The surfaces are one
   enumeration that a test walks, so a new one cannot be added without a
   decision.
9. **Living posts are recalled.** Before a living post is sent the
   section, the report's cursor records where it is: the route, its
   destination and the route's own options for that destination as its
   kind writes them, never a secret or what the section held. Before and
   after that, one store record names the report: names alone, never a
   route, a place, a count or what was shown, and never shown anywhere
   but doctor's output to the operator. It is written with
   compare-and-set and a new version each time, so a run dropping a name
   meanwhile fails and keeps it; a name whose cursor keeps no such post
   is harmless and dropped at the next round. Before any living post is
   sent the section, even one the cursor already records, herald reads
   the record and writes the report's name back where it is missing, as
   two overlapping runs can leave a cursor recording a post whose report
   the record no longer names. It holds at most 10000
   names. Where it or the cursor cannot be written, conflicts three times,
   is full, or the store keeps no such record (the markers store), the
   post is sent the withheld line instead, and doctor says why.
   At every report round, once every route no section reaches is sent its
   post and before any section is sent, herald checks each recorded post of every report that record names, whether or not
   the config still holds the report, its board or its view: where its
   route is gone from the config, no longer keeps a living post, no longer
   names the report, has moved to another place, or is no longer shown
   the section, or the report's view no longer reads advisories, herald
   edits the post, through the ledger entry it still
   holds, to one fixed line: "The advisory section of this report was
   removed because this route no longer receives it." The template
   `herald:recalled` is that line, as the post's title and body alike,
   and cannot be replaced: it names no repository, advisory, count or
   report, and it goes only where the section already was. A place is
   compared as its destination's kind reads it, so an option the kind
   newly writes with its default moves no post. A recorded place is read
   back through the kind's own checks, as a route's options are at load,
   and one they refuse is never reached. Every id a destination's request
   names in a path is first checked by its kind's shape (a Discord
   snowflake of digits; a Mastodon status or account id of letters,
   digits, `-` and `_`; a Matrix event id of `$` and printable
   characters), then passed through one segment encoder,
   `herald_core::segment`, which percent-encodes it and refuses a segment
   that is empty, `.` or `..`: a URL parser takes those as a step through
   the path however their dots are encoded (`%2E%2E` included), so
   encoding alone cannot keep them in place. A record changed in the
   store reaches no other path of the platform's API. An edit that
   lands drops the record, unless a part of the post is a stray. A post
   whose own message was sent and never recorded, or whose deletion is
   under way, is still recalled. Where the destination is gone, cannot edit,
   no route of the run reaches it, or the ledger no longer holds the
   post, the record is kept, retried once a round, and named by doctor;
   such a post is not recallable, and is deleted by hand. Each run also
   checks the audience of every recorded post's place again, once a run
   for each place, whether or not the config changed, and recalls each
   post whose place no longer passes; a route this run does not reach is
   left for a run that does. Until the recall, a run keeps two of its
   requests for each recorded post it may recall, one to delete the post
   itself and one to post the fixed line, the same however long the
   section was; the chunks' deletions take what is left after every other
   post is sent. A run whose budget ends before even that leaves the
   section up until a run reaches it, and doctor names each post whose
   deletion and post anew a run left part way. Matrix and Mastodon keep what an edit
   replaced: a Matrix `m.replace` leaves the original event and each
   replacement in the room's timeline, and Mastodon shows a status's
   edit history. So on a kind that keeps edits (each kind decides it,
   `Reach::keeps_edits`) a living post that is or was sent the section
   (its route shown it this round, or the cursor recording it) is never
   edited: each new revision redacts or deletes every event herald sent
   for it, the post itself first and then its chunks, and posts the
   revision anew. The new post and each chunk go under a transaction key
   (Matrix) or `Idempotency-Key` (Mastodon) made of the post's key, the
   revision and when the replacement began, so the homeserver or instance
   never answers with an earlier, deleted post, even of the same revision.
   Every other request that names one goes under a key no earlier request
   used: a post or chunk outside a replacement under its key and when it
   was first sent, so the post of an entry housekeeping forgot is never
   taken for a new one; an edit under its key, revision, part and when it
   was first sent, so a board back at an earlier revision is not answered with that
   revision's earlier edit; and a redaction under its key, part and a
   digest of the event it redacts, as a homeserver may keep transactions
   per device and endpoint (the Conduit family does) and would answer the
   redaction of another event with an earlier one's. Mastodon deletes a
   status by its id, with no key. A send again after one whose answer was
   lost goes under the first one's key, so the server answers with what
   it already made rather than making it twice. The ledger keeps each
   event's id until its deletion lands, and the revision is recorded only
   once every step landed; a replacement that stopped part way is resumed
   under the same keys, deleting only what it had not, and each post or
   chunk it sent but never recorded is read back before it is sent again.
   A post read back is taken as the replacement's own only when that same
   replacement resumes: the marker that finds it is the post's key alone,
   so it cannot tell one revision's post from another's. A new
   replacement first reads back each post or chunk any earlier run sent
   and never recorded, and deletes each one found with the rest. One it
   cannot find (the read reaches back only so far) may still be there:
   herald keeps it among the entry's strays, keeps the post's recall
   record whatever later edits land, and doctor names the post, its
   route and its report as possibly orphaned, to be deleted by hand. A
   part sent again under the key it was first sent under is never a
   stray: the server answers with what it already made. Only a first
   send declined outright drops its attempt, as the destination made
   nothing: the answer to the step's first request that is not a read,
   the one that makes the post, is a client error other than a timeout,
   408, or a request to wait, 429, sent as JSON in the platform's own
   error envelope (Matrix's `errcode`, Mastodon's `error`, Discord's
   numeric `code`, Slack's `"ok": false` with an `error`; each kind of
   destination decides it, and ActivityPub and herald's own kinds take
   none). A page a proxy or firewall answers with, which may come after
   the platform kept the post, a wait, a server error (a proxy's 502 or
   504 may come after the write landed), a 408, an answer that lands but
   names no id, and any answer to a send again keep the step attempted,
   so the next run reads it back, and a new replacement that cannot find
   it keeps it as a stray. A stray is dropped once a send under its own
   key lands. `herald orphans clear <report>` lists a report's strays,
   one route's with `--route <id>`: each route's parts (the post or a
   chunk, and how many times it was sent under a key of its own), a
   short digest of those strays' parts and keys, and the command that
   clears them, and it ends saying that clearing makes herald stop
   tracking them while any that still exists may go on showing the
   section, possibly in a public place. Doctor names each route's
   listing. Only `herald orphans clear <report> --route <id> --deleted
   --seen <digest>`, the operator saying the parts listed were deleted by
   hand, drops them, one route at a time, and only where that route's
   strays still hash to the digest listed; a stray a run added since is
   never cleared unseen, as the command then refuses, saying the parts
   changed since they were listed and to list them again. It prints what
   it cleared, or that nothing was (or, where the write's outcome is
   unknown, that it may or may not have been), and the next round drops
   the post's recall record and the requests kept for it. It refuses
   where there are none. A recall there is the same
   deletion, the fixed line posted anew. Between runs nothing reacts: a room or
   channel opened after a run keeps the post until the next run's check,
   and what a reader already saw or fetched cannot be unsent. A route
   re-added with the same id is checked like any other: not private, its
   post is recalled; private, it is shown the section afresh. A cursor
   written before this record reads as holding none; an older herald
   ignores the record and drops it the next time it writes the cursor, so
   the posts it named are no longer recalled.
10. **Caches and kept records.** Advisories are read live each round and
    are never kept in a record; the recall record holds where a post is,
    never what it showed. The round's read of a view holds what the
    forge answered, and every audience's board is built from it through
    `advises`; no rendering is cached, and a ledger entry and its
    revision are keyed by the route.
11. **Order leaks nothing.** A report round reads every report's rows and
    checks the place of every route that could be shown a section first;
    every part no section reaches, each route found public included, is
    sent next, before any advisory is read, any recorded post recalled or
    any section sent, so neither what those routes are sent nor whether
    the run's budget lets them out depends on the advisories. A route
    whose cursor records a section sent to it waits for the recall. Where
    a route sent the section stops, the report's cursor remembers the
    window for every other route and keeps that route pending: the next
    run sends it alone, so whether a route no section reaches is sent
    again never depends on how long the section was. Then
    the recall, then the advisories are read, keeping as many requests as
    each post still waiting takes in steps for a post of one part, the
    same however long the section is, and the sections are sent.
12. **Counts leak too.** Titles, totals, counts and an event's visibility
    are worked out from the rows alone, never from advisories, so a route
    that is not private is sent the same bytes whether the view found
    advisories or none.

## Consequences

- An advisories section costs three requests a holder on GitHub and one
  on GitLab, one more for each further page of 100, and one more for the
  order of the newest pushed members where the view holds them all;
  doctor names it, the permission
  each forge's token needs, and the routes sent the withheld line.
- An operator who wants advisories on a channel must say that channel is
  private; nothing else will put them there.
