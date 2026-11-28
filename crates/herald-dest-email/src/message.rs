use std::fmt;

use herald_core::Marker;
use jiff::Timestamp;

use crate::address::{AddrSpec, Mailbox};
use crate::body::quoted_printable;
use crate::header::Head;

/// The most bytes of a message where the config bounds it no tighter.
pub const MOST_BYTES: usize = 262_144;
/// The most recipients a message is sent to: the fewest an SMTP server must
/// take (RFC 5321 §4.5.3.1.8).
pub const MOST_RECIPIENTS: usize = 100;
/// The boundary between a message's alternatives (RFC 2046 §5.1.1). It holds
/// `=_`, which no quoted-printable body holds (RFC 2045 §6.7), so no body
/// holds the boundary.
pub const BOUNDARY: &str = "=_herald";
/// What an announcement's message says made it (RFC 3834 §5.2).
pub const AUTO_SUBMITTED: &str = "auto-generated";

/// Why the recipients a route names are not recipients herald sends to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unrecipiented {
    /// No recipient at all.
    None,
    /// More than [`MOST_RECIPIENTS`].
    Many,
    /// One address named twice.
    Twice,
}

impl fmt::Display for Unrecipiented {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => {
                f.write_str("names no recipient: give `to`, `cc` or `bcc` at least one address")
            }
            Self::Many => write!(
                f,
                "names more than the {MOST_RECIPIENTS} recipients every mail server takes in \
                 one message"
            ),
            Self::Twice => f.write_str("names one address twice among `to`, `cc` and `bcc`"),
        }
    }
}

impl std::error::Error for Unrecipiented {}

/// Whom a message goes to: `to` and `cc` written in its header, `bcc` in its
/// envelope alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipients {
    to: Vec<Mailbox>,
    cc: Vec<Mailbox>,
    bcc: Vec<Mailbox>,
}

impl Recipients {
    /// The recipients a route names.
    ///
    /// # Errors
    ///
    /// Where they name none, more than [`MOST_RECIPIENTS`], or one address
    /// twice.
    pub fn new(
        to: Vec<Mailbox>,
        cc: Vec<Mailbox>,
        bcc: Vec<Mailbox>,
    ) -> Result<Self, Unrecipiented> {
        let recipients = Self { to, cc, bcc };
        let addresses: Vec<&AddrSpec> = recipients.envelope().collect();
        if addresses.is_empty() {
            return Err(Unrecipiented::None);
        }
        if addresses.len() > MOST_RECIPIENTS {
            return Err(Unrecipiented::Many);
        }
        for (at, address) in addresses.iter().enumerate() {
            if addresses
                .iter()
                .skip(at + 1)
                .any(|other| address.same(other))
            {
                return Err(Unrecipiented::Twice);
            }
        }
        Ok(recipients)
    }

    /// Those written in the `To` field.
    #[must_use]
    pub fn to(&self) -> &[Mailbox] {
        &self.to
    }

    /// Those written in the `Cc` field.
    #[must_use]
    pub fn cc(&self) -> &[Mailbox] {
        &self.cc
    }

    /// Those the message goes to unnamed in it.
    #[must_use]
    pub fn bcc(&self) -> &[Mailbox] {
        &self.bcc
    }

    /// Every address the message goes to: `to`, then `cc`, then `bcc`.
    pub fn envelope(&self) -> impl Iterator<Item = &AddrSpec> {
        self.to
            .iter()
            .chain(&self.cc)
            .chain(&self.bcc)
            .map(Mailbox::address)
    }
}

/// A message's id (RFC 5322 §3.6.4), written between angle brackets.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageId(String);

impl MessageId {
    /// The id of part `part` of the announcement `marker` marks, sent from
    /// `from`: `<marker@domain>` for the first part, `<marker.N@domain>` for
    /// part N after it.
    #[must_use]
    pub fn of(marker: &Marker, part: usize, from: &AddrSpec) -> Self {
        let left = match part {
            0 => marker.as_str().to_owned(),
            later => format!("{marker}.{later}"),
        };
        Self(format!("{left}@{}", from.domain()))
    }

    /// The id as written inside its angle brackets.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}>", self.0)
    }
}

/// What a message says and whom it goes to.
#[derive(Debug, Clone, Copy)]
pub struct Letter<'a> {
    /// Who sends it.
    pub from: &'a Mailbox,
    /// Where replies go, where not to `from`.
    pub reply_to: Option<&'a Mailbox>,
    /// Whom it goes to.
    pub recipients: &'a Recipients,
    /// Its subject: a rendered title, written as [`subject_line`] gives it.
    pub subject: &'a str,
    /// Its plain-text body.
    pub text: &'a str,
    /// Its HTML body, written after the plain text as its alternative.
    pub html: Option<&'a str>,
    /// Its id.
    pub id: &'a MessageId,
    /// The message it replies to, a thread's first.
    pub replying: Option<&'a MessageId>,
    /// When it is written.
    pub date: Timestamp,
}

/// A message as written and whom it goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The message: CRLF lines of printable ASCII, a space or a tab, none
    /// longer than 998 characters.
    pub message: String,
    /// The address it is sent from.
    pub sender: AddrSpec,
    /// Every address it goes to, `bcc` among them.
    pub to: Vec<AddrSpec>,
}

/// A message longer than its bound, by its length; nothing of it is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Oversized {
    /// How many bytes it would be.
    pub bytes: usize,
    /// How many it may be.
    pub most: usize,
}

impl fmt::Display for Oversized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the message would be {} bytes, more than the {} its route allows",
            self.bytes, self.most
        )
    }
}

impl std::error::Error for Oversized {}

impl Letter<'_> {
    /// The message as it is sent, at most `most` bytes. Each header field
    /// herald writes stands once, its value from the config, herald's own
    /// constants, or the subject, which is encoded where it is not plain
    /// ASCII; `bcc` stands in no field.
    ///
    /// # Errors
    ///
    /// Where the message would be longer than `most` bytes.
    pub fn written(&self, most: usize) -> Result<Written, Oversized> {
        let mut head = Head::default();
        head.field("Date").token(&date(self.date));
        head.field("From").mailbox(self.from, true);
        if let Some(reply_to) = self.reply_to {
            head.field("Reply-To").mailbox(reply_to, true);
        }
        listed(&mut head, "To", self.recipients.to());
        listed(&mut head, "Cc", self.recipients.cc());
        head.field("Subject")
            .unstructured(&subject_line(self.subject));
        head.field("Message-ID").token(&self.id.to_string());
        if let Some(parent) = self.replying {
            head.field("In-Reply-To").token(&parent.to_string());
            head.field("References").token(&parent.to_string());
        }
        head.field("Auto-Submitted").token(AUTO_SUBMITTED);
        head.field("MIME-Version").token("1.0");
        let mut message = head.into_text();
        match self.html {
            Some(html) => {
                appended(
                    &mut message,
                    &[
                        "Content-Type: multipart/alternative; boundary=\"",
                        BOUNDARY,
                        "\"\r\n\r\n",
                    ],
                );
                for (media, body) in [("text/plain", self.text), ("text/html", html)] {
                    appended(&mut message, &["--", BOUNDARY, "\r\n"]);
                    message.push_str(&part(media, body));
                    message.push_str("\r\n");
                }
                appended(&mut message, &["--", BOUNDARY, "--\r\n"]);
            }
            None => message.push_str(&part("text/plain", self.text)),
        }
        if message.len() > most {
            return Err(Oversized {
                bytes: message.len(),
                most,
            });
        }
        Ok(Written {
            message,
            sender: self.from.address().clone(),
            to: self.recipients.envelope().cloned().collect(),
        })
    }
}

fn listed(head: &mut Head, name: &'static str, mailboxes: &[Mailbox]) {
    if let Some((last, rest)) = mailboxes.split_last() {
        let mut field = head.field(name);
        for mailbox in rest {
            field.mailbox(mailbox, false);
        }
        field.mailbox(last, true);
    }
}

fn appended(message: &mut String, pieces: &[&str]) {
    for piece in pieces {
        message.push_str(piece);
    }
}

/// A body part's fields and its body in the quoted-printable encoding.
fn part(media: &str, body: &str) -> String {
    format!(
        "Content-Type: {media}; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n{}",
        quoted_printable(body)
    )
}

/// `at` as RFC 5322 §3.3 writes a date, in UTC as `+0000`.
fn date(at: Timestamp) -> String {
    at.strftime("%a, %d %b %Y %H:%M:%S +0000").to_string()
}

/// `title` as a subject: one line, every run of whitespace one space, every
/// other control character removed, no space at either end.
#[must_use]
pub fn subject_line(title: &str) -> String {
    title
        .split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
