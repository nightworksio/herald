//! herald's email destination (ADR-0094).
//!
//! It writes each part of an announcement as one RFC 5322 message: a date, a
//! sender, the recipients the config names, a one-line subject, an id made
//! from the announcement's marker, the id of the thread's first message that
//! a later part replies to, and its body as plain text with an HTML
//! alternative, in the quoted-printable encoding. Every address and display
//! name comes from the config, parsed as it loads; the only text of an
//! announcement in a header is the subject, whose control characters are
//! removed and which is written as encoded-words where it is not plain
//! ASCII. A blind copy's address stands in the envelope alone.

mod address;
mod body;
mod header;
mod message;

pub use address::{ADDRESS_MOST, AddrSpec, LABEL_MOST, LOCAL_MOST, Mailbox, NAME_MOST, Unparsed};
pub use body::QUOTED_LINE;
pub use header::{ENCODED_LINE, ENCODED_WORD, LINE};
pub use message::{
    AUTO_SUBMITTED, BOUNDARY, Letter, MOST_BYTES, MOST_RECIPIENTS, MessageId, Oversized,
    Recipients, Unrecipiented, Written, subject_line,
};
