# ADR-0021: A kind names at most 24 links, each fitting a button

**Status:** Accepted
**Date:** 2026-11-11

## Context

ADR-0019 makes each link a kind names a button, labelled by the config's
label, else herald's phrase, else the link's name, and bounds a label at 75
characters. It bounds neither how many links a kind names nor the name a
button falls back to. Discord holds at most five buttons in a row and five
rows in a message, takes a label of at most 80 characters and a link
button's address of at most 512, and refuses the whole post where one of
them does not fit.

## Decision

1. **A kind names at most 24 links**: the 25 buttons a Discord post holds,
   less its subscribe button. The config refuses a 25th, and the schema says
   so.
2. **A link with no label has a name of at most 75 characters**, the most a
   label takes. The config refuses a longer one and asks for a label.
3. **Discord draws its buttons five to a row**, in order, the subscribe
   button last.
4. **Discord leaves out a button whose address is longer than 512
   characters**, as it leaves out a subscribe button whose id is too long.
   The post is sent with the rest.

## Alternatives considered

| Option | Why not |
|--------|---------|
| **Discord draws the first 25 buttons and drops the rest** | A config that names more links than any post shows reads as if they were shown. |
| **Refuse a post with an address too long for a button** | An address the event carries is outside the config's reach, and the post is worth sending without that one button. |
| **Shorten a name that is too long to a label** | A cut name reads as a typo; the config can give a label. |

## Consequences

**A declared kind with more than 24 links, or an unlabelled name longer than
75 characters, no longer loads.**

## Related

- [ADR-0019](0019-a-kind-s-links-are-its-buttons-each-labelled-by-the-config-or-by-herald.md): a kind's links as its buttons, extended here
