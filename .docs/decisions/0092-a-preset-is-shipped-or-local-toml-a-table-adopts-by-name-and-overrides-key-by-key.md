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
     only. It may not hold a secret reference, a destination, a source, a
     store, a host or API base, `private`, `vouch_unchecked`, a merge
     queue, a person or a route's `act`; each of these is refused with the
     key and the line. The adopting table in `herald.toml` may give any
     of them, so what herald trusts and where it writes stays in
     `herald.toml`;
   - a local preset does not `use` another preset.
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

## Alternatives

| Option | Why not |
|--------|---------|
| **A preset is only a wizard template written out in full** | A config of one line per preset would not get a later release's improved presets, and every adopter would carry tables they did not write. `eject` gives the written-out form to anyone who wants it. |
| **Shipped presets only** | A team with several repositories repeats the same routes and templates; a local preset keeps them once, beside the config. |
| **A local preset with the full config's reach** | A preset file is reviewed less closely than `herald.toml`; secrets, hosts, privacy statements and acting rights in it would let a change to a shared file widen where herald writes or what it trusts. |

## Consequences

- An adopter writes one table per preset, and sees its expansion with
  `herald preset show`.
- A local preset written for another config format stops the load, named.
- Privacy statements, credentials and hosts live only in `herald.toml`.

## Related

- [ADR-0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md): one file; extended here
- [ADR-0067](0067-a-view-shows-its-repositories-advisories-to-private-routes-alone.md): privacy checks after expansion
- [ADR-0091](0091-herald-init-writes-a-first-config-and-its-workflow-and-posts-one-test-message.md): the wizard offers presets
