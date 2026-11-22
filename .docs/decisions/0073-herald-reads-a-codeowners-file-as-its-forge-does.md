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

## Consequences

- herald's owners are the forge's own: a file that works on the forge
  works in herald, and one line the forge rejects is rejected alone.
- A file past a bound is refused whole rather than read in part, since
  a part read under last-match-wins can give owners the forge does not.
- A GitHub line using `[ ]`, `!` or `\#`, which GitHub documents as not
  working, is skipped rather than guessed at.
