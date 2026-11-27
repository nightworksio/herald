# ADR-0092: A preset is shipped or local TOML a table adopts by name and overrides key by key

**Status:** Accepted
**Date:** 2026-11-27

## Context

`herald.toml` is one file with no include or layering (ADR-0006 decision
1); every table denies unknown keys. Announcing releases, reminding of
reviews, a small team's merge queue and security alerts to a private
channel are each a handful of tables an adopter writes by hand. herald
already ships named content under a `herald:` prefix: the templates
`herald:queue` and `herald:nudge` and the view `queue`. A template's
`file` is read from the config's directory.

## Decision

1. **`[presets.<name>]` adopts a preset by `use`.** The loader expands it
   into the tables it stands for before it reads the config, so every
   check that holds for a hand-written table holds for an expanded one,
   ADR-0067's privacy rules among them.

   ```toml
   [presets.releases]
   use = "herald:announce-releases"
   destination = "slack"
   sources = ["app"]
   ```

2. **Parameters and overrides.** Each preset declares its parameters, each
   with a type, a recommended default and a description. Any key the
   preset writes may be given in the adopting table, and the table's
   value wins. A key the preset neither takes nor writes is an error that
   lists the keys it does. What a preset makes is named
   `<table name>-<part>`, so two uses of one preset never collide; a clash
   with a hand-written name is a load error.
3. **The shipped presets** are `herald:announce-releases`,
   `herald:review-reminders`, `herald:merge-queue-small-team` and
   `herald:security-alerts-private`; the last writes `private = true`, and
   ADR-0067's rules hold for what it expands to as for a hand-written
   route.
   They are data in the binary, never fetched: a config gets the presets
   of the herald that reads it.
4. **A local preset is a file in the config's directory.**
   `use = "./presets/team.toml"` reads the file with the reader a
   template's `file` uses, and more closed:
   - the path is relative, starts with `./`, names no `..` segment, and
     resolves, every link followed, to a file inside the config's
     directory; any other path is refused;
   - the file declares `version = 1`, the config format it was written
     for, and a version the reading herald does not read is refused. It is
     read with the reading herald's own schema: a key that herald does not
     know is an error naming the preset file, its line and the key;
   - it may hold `routes`, `templates`, `views`, `reports` and `rules`
     only, and no secret reference anywhere: no destination, source,
     store, host or API base, merge queue or person;
   - of each table it writes only the keys that pick and shape what
     herald announces from what `herald.toml` already lets it read, and
     where `herald.toml` already lets it post. A route: `id`, `sources`,
     `kinds`, `prereleases`, `versions`, `match`, `area_paths`,
     `template`, `templates` and `group`. A template: `title` and `body`.
     A view: what it reads and how (`sources`, `states`, `bots`,
     `repositories`, `authors`, `labels`, `areas`, `stale`, `stale_only`,
     `group_by`, `bumps`, `rules`, `release`, `flaky`, `deciding`,
     `conflicts`, `queue`, `mode`, `reconcile`, `most_open`, `most_pulls`,
     `broken_at`). A report: its view and window (`view`, `every`, `at`,
     `days`, `zone`). A rule: its view, window, `stages`, `most`,
     `dequeued` and `release`;
   - every other key is refused with the key, the line and why: where a
     route posts (`destination`, `crosspost`, `thread`, `living` and each
     destination kind's own route options, such as `channel`, so a new
     kind opens nothing), whom it calls on (`mentions`), what it does to
     its posts (`pin`, `edits`, `withdraw`, `late_release`), what herald
     trusts of its place (`private`, `vouch_unchecked`), acting (`act`),
     a template's `file`, a view's overviews of advisories, members and
     teams and its `decide`, which hands pull requests to a model, and a
     rule's `flaky` re-runs. A key a new herald adds to these tables is
     refused until it is allowed. The adopting table in `herald.toml`
     gives any of them, so where herald writes, whom it reaches, what it
     trusts and what it does stays in `herald.toml`. A shipped `herald:`
     preset is herald's own data and is not read against this list;
   - a local preset does not `use` another preset.
   - Every other file the config names is contained as decision 8 says.
5. **Nothing is hidden.** `herald preset show <table>` prints the TOML a
   table expands to; `herald preset eject <table>` writes that expansion
   into `herald.toml` in place of the table, through `toml_edit`, after one
   confirmation. Load errors name the line of the `[presets.*]` table and
   the preset's key, never a line of shipped TOML the operator cannot see.
6. **The schema and the settings reference** hold `presets.<name>`, with
   `use` and each shipped preset's parameters keyed on `use`, and a
   "Presets" section showing each shipped preset's expansion, generated
   from the same declarations.
7. **This extends ADR-0006 decision 1**: one file, which a preset table
   expands as it loads.
8. **Every file the config names lies beside it or in a directory
   `[files] allowed` names.** A template's `file`, a source's
   `members.file`, an ActivityPub `lists[].file` and `http.ca_file` are
   read only where the path is a path of names under `herald.toml`'s
   directory with no `..` step, or an absolute path inside a directory
   `[files] allowed` lists. Each listed directory is absolute with no `..`
   step. The list lives in `herald.toml` alone: a preset cannot write it,
   and it has no environment variable or flag, so what a config may reach
   is read in the one file a reviewer reads. herald's native binary and the
   JavaScript hosts' `deploy/files.mjs` resolve every link of the file and
   of the directory before the check, so a link inside the directory that
   leads out of it is refused; Fastly's Config Store and a JavaScript object
   of files hold names, not paths. A file elsewhere stops the load, and the
   error names the file, its key and `[files] allowed` and nothing the file
   holds; `herald doctor` names each such file. A config that named a file
   outside its directory stops loading until the file is moved beside it or
   its directory is listed.
   - `http.ca_file` is held to the same rule: a PEM file that does not parse
     is told by its reader with the offending line (rustls-pki-types 1.15.1,
     `src/pem.rs`, `IllegalSectionStart`), so what it holds can reach an
     error. A CA bundle in a system directory is reached by listing that
     directory, and herald names a PEM section it cannot read by the file
     alone.
   - A store's `path`, the SQLite file or the Deno KV database, is herald's
     own records, opened and written by herald, and no text of it reaches a
     post or an error; it keeps its own rule.
   - A file that does not parse is told by its file, line and, where the
     parser tells one, column, never by what it holds: a member file's TOML
     or JSON, a template file that does not compile (minijinja tells the
     line alone), and a block list row that names no domain.

## Alternatives

| Option | Why not |
|--------|---------|
| **A preset is only a wizard template written out in full** | A config of one line per preset would not get a later release's improved presets, and every adopter would carry tables they did not write. `eject` gives the written-out form to anyone who wants it. |
| **Shipped presets only** | A team with several repositories repeats the same routes and templates; a local preset keeps them once, beside the config. |
| **Files read from any path the config names** | Whoever may change `herald.toml` but not reach the host that runs herald could make herald post any file the process can read into a channel they read, and read some of it back from load errors. |
| **A local preset with the full config's reach** | A preset file is reviewed less closely than `herald.toml`; secrets, hosts, privacy statements and acting rights in it would let a change to a shared file widen where herald writes or what it trusts. |

## Consequences

- An adopter writes one table per preset, and sees its expansion with
  `herald preset show`.
- A local preset written for another config format stops the load, named.
- Privacy statements, credentials and hosts live only in `herald.toml`.
- A config naming a template, member list, block list or CA file outside its
  directory lists that file's directory in `[files] allowed`.

## Related

- [ADR-0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md): one file; extended here
- [ADR-0067](0067-a-view-shows-its-repositories-advisories-to-private-routes-alone.md): privacy checks after expansion
- [ADR-0091](0091-herald-init-writes-a-first-config-and-its-workflow-and-posts-one-test-message.md): the wizard offers presets
