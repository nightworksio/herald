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
//!
//! Amazon SES carries each message whole, by `SendEmail` of its API version
//! 2, each request signed with AWS Signature Version 4 to the region's
//! endpoint alone, the recipients beside the message and tracking off.

mod address;
mod body;
mod config;
mod email;
mod first_run;
mod header;
mod message;
mod options;
mod reach;
mod ses;

pub use address::{ADDRESS_MOST, AddrSpec, LABEL_MOST, LOCAL_MOST, Mailbox, NAME_MOST, Unparsed};
pub use body::QUOTED_LINE;
pub use config::{EmailKind, FEWEST_BYTES, KIND};
pub use email::{Addressing, Email, MARKUP, UNCHECKED, profile};
pub use header::{ENCODED_LINE, ENCODED_WORD, LINE};
pub use message::{
    AUTO_SUBMITTED, BOUNDARY, Letter, MOST_BYTES, MOST_RECIPIENTS, MessageId, Oversized,
    Recipients, Unrecipiented, Written, subject_line,
};
pub use options::{Addresses, DestinationOptions, RouteOptions, Transport};
pub use ses::{Credentials, SES_BYTES, SES_RECIPIENTS, Ses, regional};
