# ADR-0073: herald reads a CODEOWNERS file as its forge does

**Status:** Accepted
**Date:** 2026-11-22

## Context

A CODEOWNERS file names who carries each path of a repository. Areas
(ADR-0071) can come from it, so a board groups a monorepo's pull
requests by owner with no table in the config. Each forge writes the
file in its own dialect, and anyone who can push to the repository's
branch writes its content: herald reads it as network input, bounded
before it is read, and must reach the owners the forge itself reaches.

The forges document their dialects:

- GitHub: <https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-code-owners>
- GitLab: <https://docs.gitlab.com/user/project/codeowners/>,
  <https://docs.gitlab.com/user/project/codeowners/reference/> and
  <https://docs.gitlab.com/user/project/codeowners/advanced/>, and its
  reading in `ee/lib/gitlab/code_owners/section_parser.rb` and
  `ee/lib/gitlab/code_owners/file.rb`
- Forgejo: <https://forgejo.org/docs/latest/user/pull-requests-and-git-flow/>,
  and its reading in `models/issues/pull.go` (`GetCodeOwnersFromReader`,
  `ParseCodeOwnersLine`, `TokenizeCodeOwnersLine`) and
  `services/issue/pull.go`
- Gitea: <https://docs.gitea.com/usage/code-owners>

## Decision

1. **Three dialects in the core.** `Codeowners::read` reads a file for
   GitHub, GitLab or Forgejo (Gitea's dialect is Forgejo's) and is the
   one way a file is read. Each line becomes a pattern, the regular
   expression a path is matched by, and its owners; paths are matched
   case-sensitively, as every forge matches them.
   - **GitHub** follows gitignore. A `/` first or within anchors the
     pattern at the root, and otherwise it matches at any depth; `**`
     stands for any directories; a `/` last takes everything within; a
     last segment `*` takes only the files directly within (`docs/*`);
     any other pattern takes the path it names and everything within it.
     The last matching line wins, and a line naming no owner leaves its
     paths with none. Owners are `@login`, `@org/team` and email
     addresses, and an inline `#` begins a comment. A line GitHub does
     not read is skipped: one starting with `!` or `\#`, one holding
     `[`, or one naming an owner GitHub does not read.
   - **GitLab** reads each pattern as GitLab does: `*` alone is every
     path, one not starting with `/` matches at any depth, one ending in
     `/` takes everything within, and the rest is `fnmatch` with
     `FNM_PATHNAME` and `FNM_DOTMATCH` (`*`, `?` and `[...]` within one
     segment, `**/` any directories). `\` escapes, so `path\ with\
     spaces/` is one pattern. A heading is read as GitLab's
     `SectionParser::HEADER_REGEX` reads one: `[Name]` or `^[Name]`
     (optional), the name running to its first `]`; then an approvals
     count in brackets only where it holds digits and spaces alone
     (`[2]`, `[ 1]`, `[]`); then default owners as far as `@`, letters,
     digits, `_`, `.`, `-`, `/` and spaces run, the rest of the line
     unread. So `[Name][@owner] @a` and `[Name][x] @a` start the section
     `Name` with no default owners, as GitLab does. Sections of one name,
     ignoring case, are one, named as first written; entries before any
     heading are GitLab's default section, `codeowners`, which a heading
     of that name goes on with. A section whose name cannot name an area
     (blank, padded, too long, or holding a control character) still
     groups its entries, as GitLab's does, and names no area. In each
     section the last matching entry wins, an entry naming no owner takes
     the section's defaults, and a matching `!` entry takes the path out
     of that section alone; every section applies. Owners are `@name`
     (a person or a top-level group, which the file does not tell
     apart), `@group/subgroup`, `@@role` and email addresses; another
     word is ignored, as GitLab ignores it. A
     line starting with `[` or `^[` whose name never closes is no heading
     to GitLab: it is skipped, and the entries after it belong to the
     section before it.
   - **Forgejo** reads each line as a Go regular expression between `^`
     and `$`, ungrouped, as Forgejo writes it; `\` escapes a space or a
     `#` and is dropped, and an unescaped `#` ends the line. Every
     matching line applies; a line starting with `!` applies to each path
     its expression does not match. Owners are `login` and `org/team`,
     with or without `@`; a line naming no owner Forgejo reads is
     skipped, as Forgejo skips it.
2. **Linear-time matching.** Every pattern, translated or written, is
   matched by the `regex` crate, which never backtracks: matching a path
   costs time linear in its length. Go's and Rust's syntax are both
   RE2's; a Forgejo line that does not compile in Rust is skipped, and
   `\d`, `\w` and `\s` read Unicode classes where Go reads ASCII ones,
   which differ only on paths outside ASCII.
3. **Bounded before it is read.** A file longer than 3 MiB, GitHub's own
   limit, or of more than 4000 lines is refused whole: no path falls to
   any owner, as GitHub does not read a file past its limit. A pattern
   longer than 1024 characters, a line naming more than 100 owners, and
   a pattern compiling alone to more than 256 KiB (a counted repetition
   such as `a{1000}` or a Unicode class such as `\w`, about 50 KiB, grows
   a short pattern) are skipped. The file's patterns are compiled
   together, into one set matched once for each path, within 16 MiB (a
   file of 4000 gitignore patterns takes about 10 MiB), or the file is
   refused whole; the lazy automaton matching them keeps at most 16 MiB of
   states, what a file of 4000 lines needs to match each path in about
   a millisecond.
   An owner or a section's name is an area's name (ADR-0071): one that
   could not name an area is not read. A changed path longer than 1024
   characters falls to no one, as it falls in no area.
4. **A skipped line matches nothing.** A line herald does not read is
   skipped with its number and why, so it can be named, and gives no
   owner: on GitHub and GitLab the line before it that matches wins
   instead, which is what the forge does with a line it skips.
5. **Fuzzed.** One fuzz target reads every input in each of the three
   dialects and holds the bounds: reading ends, names only lines the
   file has, in order, and a refused file, a path past its bound and
   unread paths fall to no one.
6. **Areas from CODEOWNERS.** A source's or an override's
   `areas = "codeowners"` names a repository's areas by its file in
   place of a table. The areas of a pull request are each owner the
   lines its changed paths fall to name, and each GitLab section they
   fall in: teams, GitLab groups, roles and logins, never an email.
   `Codeowners::areas` is the one place that says so. Each area carries
   its kind (`paths` for a table's, `team`, `group`, `person`, `role`,
   `section`, or `name`), which a row's JSON shows, so a consumer tells
   a person from a team. A file naming more than 200 areas, a table's
   bound, is refused whole.
7. **GitLab names looked up.** A GitLab top-level `@name` is a person or
   a group, which the file does not tell. Each distinct name a pull
   request's lines name is looked up once a round per source, among the
   first 50 such names of the file: GitLab's users by that username,
   then its group of that path, through the source's token, API base
   (self-managed GitLab) and the run's request budget, each a read any
   `read_api` or `read_user` token, or none on a public instance, may
   make. A name no lookup tells, one past the 50, and one whose lookup
   failed or was refused is an area of kind `name`, never dropped.
8. **Read where the forge reads it.** `codeowners` lists the places a
   file is looked for, the forge's own by default in the forge's own
   order, the first found read; `codeowners_ref` is the branch: by
   default the one the forge applies, the base branch on GitHub and
   GitLab and the default branch on Forgejo, whose `forgejo` kind also
   reads Gitea, which applies the base branch (doctor tells Gitea users
   to set `"base"`). Where a pull request's base is not known, the
   default branch is read. Each file is read once a round per repository
   and branch by the forge's raw-file request, at most
   `most_codeowners` (128 KiB by default, at most 3 MiB) read, a longer
   one refused before it is parsed. A place that is not there (404) is
   passed; any other answer stops the read.
9. **Private as rows are.** Areas ride the row (ADR-0069) and the event's
   origin (ADR-0072) as a table's do, so a private or internal
   repository's owners, logins and section names are printed only where
   the output is said private, and a failed read is said by its source
   and kind alone unless the output is said private. Views count each
   pull request once however many owners' groups show it. An event's
   areas are herald's alone, read from the kept or delivered paths as a
   table's are, and the file read the same way.
10. **Checked as far as can be.** A view's or route's `areas` naming a
    source that reads CODEOWNERS is taken as written, since the names
    are known only as each file is read. `herald doctor` names each
    source's places, branch, cost and lookups; online, it reads a
    repository source's file at the default branch and names each line
    herald skips, by number and why (the first 20, the rest counted),
    and a file herald reads none of.

## Consequences

- herald's owners are the forge's own: a file that works on the forge
  works in herald, and one line the forge rejects is rejected alone.
- A file past a bound is refused whole rather than read in part, since
  a part read under last-match-wins can give owners the forge does not.
- A GitHub line using `[ ]`, `!` or `\#`, which GitHub documents as not
  working, is skipped rather than guessed at.
- Each round reads each file again, one request per repository and
  branch; a file that cannot be read stops the view rather than showing
  its pull requests in no area.
- A person is an area only by a login a file writes, so a board groups
  by person only where a repository says who owns what by login.
