# ADR-0074: a CODEOWNERS file read is kept, and a source sets its most areas

**Status:** Accepted
**Date:** 2026-11-22

## Context

ADR-0073 reads each repository's CODEOWNERS file once a round per
repository and branch: every round downloads the file again and reads
every line again, though a file changes rarely. Forges answer a raw-file
request with a validator (`ETag`) and answer a request that sends it
back with `304 Not Modified` while the file is unchanged. herald runs on
hosts whose process lives for one round (wasm runtimes, functions), so a
read kept in memory is gone by the next round; the store is what lasts.

ADR-0073 also refuses a file naming more than 200 areas, a table's
bound, fixed for every source. A monorepo whose file names more owners
than that reads no areas at all, with no way to raise it.

## Decision

This amends ADR-0073's decisions 6 and 8.

1. **Kept beside the pull requests.** Where the store keeps pull
   requests (ADR-0061), each CODEOWNERS file a source reads in full is
   kept there, one record per source, repository (its full path in lower
   case) and branch, with the place it was found at, its forge's
   validator, when it was read and the bounds it was read under. The
   record holds the file as herald read it: each line's owners, section
   and the pattern it matches by, the sections, the lines skipped and why
   a file is refused. It is written with compare-and-set, through the
   same record store every store backend shares, so it works wherever
   kept mode works, and the store conformance suite checks it. It is
   sealed as every record of the store is (ADR-0005), as the
   repository's kept pull requests are, so where the store has a key a
   record herald did not write reads as corrupt.
2. **Asked conditionally.** Each round still asks the places in order,
   a place that is not there passed, so a file added at an earlier place
   is found. The place the kept file was found at is asked with its
   validator; a `304` answer is the kept file: no download and no read of
   its lines. Only its patterns are compiled again, as a compiled set
   cannot be stored.
3. **Any doubt reads in full.** A store that keeps no pull requests,
   cannot read or write the record, or holds none, a record of another
   form, dialect, `most_codeowners` or `most_areas`, one whose lines and
   patterns do not pair, holding more lines than a file is read with or
   naming more areas than its `most_areas`, or one whose patterns do not
   compile again, leaves the file read in full, as ADR-0073 reads it. A record is kept only where it is at most
   `most_codeowners` bytes, the file's own bound, so a store holds no
   more for a file than the file itself may be. A record not read in
   full since an expiry is forgotten with the pull requests closed
   before it, and read in full again once.
4. **The form is pinned.** The record names its form, `KEPT_CODEOWNERS`;
   a test pins what a known file is kept as, so a change to how a file
   is read changes the form with it and old records are read again in
   full.
5. **The download is bounded by the file's bound.** A request for a
   CODEOWNERS file reads at most `most_codeowners` bytes of the answer
   (128 KiB where absent, at most 3 MiB), or 128 KiB where it is set
   lower so a forge's error answer still reads, not the source's `parse`
   (4 MiB where absent): the HTTP port's body bound, which every
   runtime's transport keeps, refusing an answer whose length says it is
   longer before reading it and stopping as soon as more arrives. Such
   an answer is the port's `Larger` error, and the file is refused
   unread with the refusal naming `most_codeowners`, as a file found
   longer is, not a failed read of the source.
6. **`most_areas` per source.** A source's `most_areas` is the most
   areas a repository's table or CODEOWNERS file may name: 200 where
   absent, 1 to 2000. Each area is a group of a board and is matched
   against every changed path, so 2000 bounds a round's matching and a
   board's groups. A table naming more is refused as the config is read;
   a file naming more is refused whole as it is read, its refusal naming
   the setting, which `herald doctor --online` repeats. An event's
   `origin.areas` names at most 2000, the most any source may read.

## Consequences

- An unchanged CODEOWNERS file costs one `304` answer a round per
  repository and branch, which GitHub does not count against the rate
  limit, and no parse.
- The store holds one more record family under pull requests; a host
  with no kept mode reads each file in full each round, as before.
- A monorepo with more owners than 200 raises `most_areas` instead of
  losing its areas.
- A file at a CODEOWNERS place is never held in memory beyond its bound,
  however large the forge answers it.
- A round's reads and GitLab `@name` lookups belong to one source: each
  source's are read and looked up apart, as one name may be a person on
  one GitLab host and a group on another.
