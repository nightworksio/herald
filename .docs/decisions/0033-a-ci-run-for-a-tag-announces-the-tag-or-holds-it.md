# ADR-0033: A CI run for a tag announces the tag, or holds it

**Status:** Accepted
**Date:** 2026-11-15

## Context

`herald announce --from <forge>` announces the release a CI run is for:
the run's release event, the tag `--release` names, or on GitLab the tag
the pipeline is for (ADR-0003). A source that announces tags (ADR-0031)
is also run by a tag's push, which publishes no Release, and a tag that
waits for its Release (ADR-0032) must wait here too.

## Decision

1. **A run for a tag reads it.** Where the source announces tags of the
   repository, a GitHub Actions run whose event is a `push` of
   `refs/tags/<tag>` is for that tag, as `--release <tag>` and a GitLab tag
   pipeline are. Where the source announces releases, the Release of the
   tag is asked first; where the forge has none and the source announces
   tags, the tag is read in full (its reference and tag object or commit
   on GitHub, the tag on GitLab and Forgejo) and announced as a tag. A
   source announcing tags alone asks for no Release.
2. **A run holds a tag as a poll does.** Where the source holds tags, the
   run writes the tag's held record and says when it is due; the next poll
   or housekeeping round announces it unless its Release came first. A
   Release a run announces lets its held tag go.
3. A run whose source announces neither its event nor its tag says so and
   announces nothing.

## Consequences

- A release train that pushes tags from CI announces each version once,
  without polling.
- A run for a tag reads up to three answers from its forge.
