# ADR-0075: a view's load is counted per team and per area, never per person

**Status:** Accepted
**Date:** 2026-11-22

## Context

A maintainer asks where review work piles up: which team or which part
of a repository holds the open review requests, and who has been giving
reviews. Areas name the parts of a repository (ADR-0071, ADR-0073), and
since ADR-0073 an area can be a person: a login a CODEOWNERS line names.
A count per area would then be a count per person, and so would a count
per team of one or two members. Load is about people, and no person is
counted by name before they opt in, which herald does not offer yet.

Team membership is personal and can be secret. A pull request's review
requests and its areas are its repository's data, and a private or
internal repository's rows reach only the surfaces that may see them
(ADR-0038 decision 3, ADR-0069).

## Decision

1. **Teams come from the config.** A top-level `[teams.<name>]` names its
   members, each a login or `login@host`, at most 500 a team and 200
   teams. Its name is the one a review request and a CODEOWNERS file
   give it, such as `org/team` for `@org/team`, compared without case
   and without leading `@`s. Reading a forge's own teams is a later
   decision.
2. **A view shows its load as an overview.** `load = { window, least_team,
   unknown_teams }` on a view counts, per team and per area, the review
   requests open now on the rows it shows, and the reviews given on them
   over `window` (1 to 90 days, 7 where absent) where every source is
   read kept, from the reviews each kept record holds. A live read keeps
   no review, so it shows requests alone and says so; JSON writes
   `reviewed` as null. A request naming a team counts for it, a request
   naming a login for each team holding it, and a review given by a
   member for the team; a member written `login@host` counts only on
   that host. An area counts every request and review of its rows.
3. **No row is one person's.** `least_team`, 3 where absent, may be raised
   to 50 and is refused below 3. A team of fewer members, a person's
   area, a GitLab `@name` no lookup told apart, and a team, group or role
   area whose `[teams.<name>]` has fewer members fold into "other". A
   team, group or role area of unknown members folds too, and counts no
   person; `unknown_teams = true` shows such an area as it is, its doc
   and doctor saying it may be one person, and it never unfolds a
   person's area. Path and section areas name no people and always
   show.
4. **"other" is as large as a team.** It counts each request and review
   its folded rows count, once however many count it, and shows only
   where the people known to be folded into it are `least_team` or more;
   otherwise there is no "other", and doctor says why and names each
   team below the number.
5. **A count is made only from rows the surface shows.** `herald queue`
   counts the rows it prints, after `Printed` withholds a private or
   internal repository's rows, so nothing of one is counted without
   `--private` or `[cli] private`; a report counts the rows each route is
   shown, after its audiences are parted, so a public route is shown the
   load of public repositories' rows alone. `Printed` stays the one place
   a row is withheld, and no load is carried past the rows it was counted
   from. A withheld pull request changes no figure: the flows test
   `a_public_route_is_shown_the_load_of_the_public_rows_alone` counts a
   team asked on both a public and a private pull request as 2 on the
   private route and 1 on the public one, and the `herald` test
   `a_private_repository_s_load_names_no_team_area_or_login_and_counts_nothing_unless_said_private`
   shows an empty load without `--private`.

## Consequences

- A team's or an area's name, and each figure, reach only a surface
  shown a row it counts.
- Reviews given are counted only on open pull requests a view shows, and
  only those kept since each record's last full read.
- A config that names no team shows path and section areas, and folds
  every other area.
