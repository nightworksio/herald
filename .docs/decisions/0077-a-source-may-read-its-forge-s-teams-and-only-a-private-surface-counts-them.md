# ADR-0077: a source may read its forge's teams, and only a private surface counts them

**Status:** Accepted
**Date:** 2026-11-22

## Context

ADR-0075 counts a view's load per team from the teams the config names,
and left reading a forge's own teams to a later decision. Naming every
team in `herald.toml` repeats what the forge already holds, and goes
stale as people join and leave. A team, group or role area of a
CODEOWNERS file only shows when its members are known; otherwise it
folds, and a config that names no team folds them all.

Who is in an organization's teams is the organization's data, not a
repository's. A secret team's name, its size and its members are not
visible to everyone who can see a public repository, so a public
repository's row does not make them public. Reading them takes a
permission a repository read does not: GitHub's Members (read) or
read:org, GitLab's read_api as a member who may see the group's members,
Forgejo's read:organization.

## Decision

1. **A source says where its teams come from.** A GitHub, GitLab or
   Forgejo source takes `teams = "config"`, where absent, or
   `teams = "forge"`. Under "forge", a view showing its load reads the
   teams of the organization or group owning the source's repositories:
   GitHub's and Forgejo's teams as `org/team`, GitLab's group and each
   group under it by full path with its direct members. The read is under
   the source's own API, so a GitHub Enterprise Server, a GHE.com tenant,
   a self-managed or Dedicated GitLab and a Gitea instance are read where
   the source points. A source of another kind has no such key.
2. **The read is bounded and counts only.** At most 100 teams a source,
   each with at most 500 members, one request a page, none conditional:
   herald keeps no team, so there is no validator to keep. Each member is
   held as a login on the forge's host for the round alone, to tell which
   team a review request or a review counts for, and is never shown,
   logged or kept. Two sources of one owner in one view read its teams
   once. The threshold of ADR-0075 applies unchanged: a forge team counts
   as a team only where its known members are `least_team` or more, and
   otherwise folds.
3. **The config wins.** A forge team named as a config team, compared as
   a review request names a team, is the config's, members and all; of
   two forge teams of one name the first in name order is kept. A forge
   team gives members to a team, group or role area of its name, as a
   config team does.
4. **Only a private surface counts them.** A forge team's name and every
   figure it changes reach a route only where the route says
   `private = true` and is private as herald can tell, and `herald queue`
   only with `--private` or `[cli] private`; never a public route, a
   digest, a feed, a push, a rule or doctor. This holds for a public
   repository's rows too. `teams_reach` in herald-core, beside `Printed`,
   is the one place this is decided. `herald queue` without it does not
   read them at all; a view's read for reports reads them only where some
   route of the config may count them, and each report parts its routes
   into those that may and the rest, counting the rest's load from the
   config's teams alone.
5. **A refusal folds; an unread forge stops.** A forge answering 401, 403
   or 404 to a listing leaves the load to the config's teams, so what only
   the forge's teams would show folds, never guessed. `herald doctor
   --online` names the permission the source's token needs. A forge that
   cannot be reached or answers what herald cannot read stops the view's
   read as any unread source does, said in full only where the output is
   private.

## Consequences

- `herald doctor` states, for each source reading its forge's teams in a
  view showing its load, the most requests a round, the permission per
  forge, and that only private surfaces count them.
- A team read from a forge costs a round requests every time the view is
  read; a source that does not say "forge" costs none.
- The flows test
  `a_forge_s_teams_reach_a_private_route_alone_read_once_an_owner_a_config_team_kept`
  shows a forge team on the private route and not on the public one, and
  the `herald` test
  `a_forge_s_team_is_named_and_counted_only_where_said_private_even_of_a_public_repository`
  shows none in table or JSON of a public repository without
  `--private`.

## Alternatives

- **Show forge teams wherever a row's repository may be seen.** A public
  repository would publish its organization's secret teams; refused.
- **Keep the forge's teams between rounds and ask conditionally.** Fewer
  requests, but every member's login would be kept in the store; refused.
- **Count members alone, never logins.** A request naming a person could
  not be told to count for the team holding them, so a team would count
  only requests naming the team itself; refused for undercounting.
