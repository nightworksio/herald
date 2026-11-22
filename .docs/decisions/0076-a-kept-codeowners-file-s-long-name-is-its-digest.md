# ADR-0076: a kept CODEOWNERS file's long name is its digest

**Status:** Accepted
**Date:** 2026-11-22

## Context

ADR-0074 keeps each CODEOWNERS file a source reads in one record per
source, repository and branch, named by those three written as one
segment of a path. The segment escapes every byte but letters, digits,
`.`, `_` and `-` as three, and nothing bounds its length: a branch name
of a hundred non-ASCII letters makes a name of more than 600 bytes. A
file system holds at most 255 bytes in a name, and the file and
git-branch stores keep each record as a file. Such a record is never
written, and as a write that fails leaves the file read in full, every
round downloads and reads the file again with nothing saying why.

## Decision

This amends ADR-0074's decision 1.

1. **A long name is its digest.** A record whose segment is longer than
   128 bytes is named by the segment's SHA-256 in lowercase hex instead,
   64 bytes. The segment of a kept CODEOWNERS file always holds two `~`,
   for the line breaks between source, repository and branch, and a
   digest holds none, so a digest never names a record a shorter key is
   named by. 128 bytes keeps every name, with `.json` and its directory,
   well within the 255 bytes of a name and the 260 of a whole path where
   a host bounds that.
2. **A record is read under its own key.** The record already holds its
   key; a record read whose key is not the one asked for reads as none,
   so two keys sharing a digest can never serve one repository another's
   file. Such a record is not written over either: the file is read in
   full, as any doubt reads it (ADR-0074 decision 3).
3. **Doctor says when a file is not kept.** Where the store keeps pull
   requests, `herald doctor --online` says when the CODEOWNERS file it
   reads would not be kept, and so is read in full every round: its
   forge answers with no validator, or its record would be longer than
   the source's `most_codeowners`.

## Consequences

- A kept CODEOWNERS file's record is written whatever its branch is
  named, on every store.
- A record kept under a name between 129 and 255 bytes before this is
  not read again: its file is read in full once and kept under its
  digest, and the old record is forgotten at the next expiry.
- The store conformance suite checks that a branch whose escaped name
  passes 255 bytes is kept and read back.
