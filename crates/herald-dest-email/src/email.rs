use herald_core::{
    Announcement, Bullet, Declined, Emphases, Escape, Facts, Features, Fences, Heading, Limits,
    Links, Markup, Measure, MentionStyle, Pins, Profile, Sent, Step, Symbols, Threads, Waits,
    safe_url, web_url,
};
use herald_kinds::Probe;
use herald_ports::{Base, DynHttp, HttpError, Response};
use jiff::Timestamp;

use crate::address::Mailbox;
use crate::mailgun::{self, Mailgun};
use crate::message::{Letter, MessageId, Recipients, Written};
use crate::ses::{self, Ses};

/// Email, as herald names it.
pub(crate) const EMAIL: &str = "Email";

/// An email's bodies: plain text, each link spelled out, beside the
/// sanitised HTML of the same text, whose links lead to the web or mail
/// alone and show their URLs (ADR-0094).
pub const MARKUP: Markup = Markup {
    escape: Escape::Neutral,
    links: Links::Spelled,
    heading: Heading::Bare,
    marks: Emphases {
        emphasis: "",
        strong: "",
        strike: "",
    },
    fences: Fences::None,
    bullet: Bullet::Symbol('-'),
    html: true,
    pills: None,
};

/// The longest subject, in bytes.
const TITLE: usize = 256;

/// The longest message id herald keeps of an answer.
const ID: usize = 256;

/// How many times a part's text fits in its message: each byte of its
/// plain text written as up to three in quoted-printable, its HTML as up to
/// twice the bytes of its text again and those as up to three each, beside
/// its header.
const FILLS: usize = 12;

/// Why herald cannot check who may read what a route mails.
pub const UNCHECKED: &str = "herald cannot ask who reads the addresses a route mails; say vouch_unchecked = true on the route where only those meant to may read them";

/// What an email destination can do with messages of at most `most`
/// bytes: send each part as a message, every later part a reply to the
/// first; nothing it sent is edited, pinned or read back.
#[must_use]
pub const fn profile(most: usize) -> Profile {
    let text = most / FILLS;
    Profile {
        markup: MARKUP,
        measure: Measure::Bytes,
        limits: Limits {
            title: TITLE,
            text,
            followup: text,
            thread_name: TITLE,
        },
        features: Features {
            threads: Threads::Replies,
            crosspost: false,
            pins: Pins::None,
            reads_back: false,
            edits: false,
            mentions: MentionStyle::None,
            interactions: false,
        },
        symbols: Symbols::DEFAULT,
    }
}

/// Who a destination's messages are from, whom each goes to, and how
/// long one may be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addressing {
    /// Who each message is from.
    pub from: Mailbox,
    /// Where replies go, where not to `from`.
    pub reply_to: Option<Mailbox>,
    /// Whom each message goes to.
    pub recipients: Recipients,
    /// The most bytes of a message.
    pub most: usize,
}

/// The provider carrying each message whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Carrier {
    /// Amazon SES.
    Ses(Box<Ses>),
    /// Mailgun.
    Mailgun(Mailgun),
}

impl From<Ses> for Carrier {
    fn from(ses: Ses) -> Self {
        Self::Ses(Box::new(ses))
    }
}

impl From<Mailgun> for Carrier {
    fn from(mailgun: Mailgun) -> Self {
        Self::Mailgun(mailgun)
    }
}

impl Carrier {
    /// The base URL its requests reach, and no other.
    #[must_use]
    pub fn url(&self) -> &str {
        match self {
            Self::Ses(ses) => ses.url(),
            Self::Mailgun(mailgun) => mailgun.url(),
        }
    }

    /// Sends `written` to `recipients` at `now`, and reads the answer.
    async fn send(
        &self,
        http: &dyn DynHttp,
        (written, recipients): (&Written, &Recipients),
        now: Timestamp,
        waits: Waits,
    ) -> Result<Sent, HttpError> {
        match self {
            Self::Ses(ses) => ses.send(http, (written, recipients), now, waits).await,
            Self::Mailgun(mailgun) => mailgun.send(http, written, waits).await,
        }
    }

    /// What its provider says, asked at `now`, sending nothing.
    pub(crate) async fn probe(&self, http: &dyn DynHttp, now: Timestamp) -> Vec<Probe> {
        match self {
            Self::Ses(ses) => ses.probe(http, now).await,
            Self::Mailgun(mailgun) => mailgun.probe(http).await,
        }
    }

    /// Whether `answer`, a client error, is its provider's own.
    pub(crate) fn declines(&self, answer: &Response) -> bool {
        match self {
            Self::Ses(_) => ses::declines(answer),
            Self::Mailgun(_) => mailgun::declines(answer),
        }
    }
}

/// One email destination, for one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email {
    addressing: Addressing,
    carrier: Carrier,
    waits: Waits,
}

impl Email {
    /// The destination mailing as `addressing` says through `carrier`.
    #[must_use]
    pub fn new(addressing: Addressing, carrier: impl Into<Carrier>) -> Self {
        Self {
            addressing,
            carrier: carrier.into(),
            waits: Waits::DEFAULT,
        }
    }

    /// The destination, waiting `waits` where its transport names no wait.
    #[must_use]
    pub fn waiting(self, waits: Waits) -> Self {
        Self { waits, ..self }
    }

    /// What its `Http` may reach: its transport's endpoint alone
    /// (ADR-0007).
    #[must_use]
    pub fn base(&self) -> Base<'_> {
        Base::At(self.carrier.url())
    }

    /// What it can do.
    #[must_use]
    pub const fn profile(&self) -> Profile {
        profile(self.addressing.most)
    }

    /// Its transport.
    #[must_use]
    pub const fn carrier(&self) -> &Carrier {
        &self.carrier
    }

    /// Sends `step` of `announcement`, written at `now`, and reads the
    /// answer.
    ///
    /// # Errors
    ///
    /// When the request got no response at all.
    pub async fn send(
        &self,
        http: &dyn DynHttp,
        step: &Step,
        announcement: &Announcement,
        now: Timestamp,
    ) -> Result<Sent, HttpError> {
        let part = match step {
            Step::Post => 0,
            Step::Chunk(index) => *index,
            other => return Ok(Sent::Refused(cannot(other).to_string())),
        };
        let (text, html) = bodies(announcement, part);
        let marker = &announcement.marks.announcement;
        let from = self.addressing.from.address();
        let first = MessageId::of(marker, 0, from);
        let id = MessageId::of(marker, usize::try_from(part).unwrap_or(usize::MAX), from);
        let letter = Letter {
            from: &self.addressing.from,
            reply_to: self.addressing.reply_to.as_ref(),
            recipients: &self.addressing.recipients,
            subject: announcement.title.plain(),
            text: &text,
            html: html.as_deref(),
            id: &id,
            replying: (part > 0).then_some(&first),
            date: now,
        };
        match letter.written(self.addressing.most) {
            Ok(written) => {
                self.carrier
                    .send(
                        http,
                        (&written, &self.addressing.recipients),
                        now,
                        self.waits,
                    )
                    .await
            }
            Err(oversized) => Ok(Sent::Refused(oversized.to_string())),
        }
    }
}

/// What email cannot do of `step`.
const fn cannot(step: &Step) -> Declined {
    let what = match step {
        Step::Edit { .. } | Step::Delete(_) => "edit or delete a message once it is sent",
        Step::Crosspost => "crosspost",
        Step::Pin | Step::Unpin => "pin",
        Step::Deliver(_) | Step::Deliveries | Step::Updates(_) | Step::Deletes => {
            "deliver to an inbox"
        }
        Step::OpenThread | Step::Rename(_) | Step::Post | Step::Chunk(_) => {
            "open or name a thread: a later part replies to the first"
        }
    };
    Declined::Cannot {
        platform: EMAIL,
        what,
    }
}

/// Part `part` of `announcement` as its message's plain text and HTML: the
/// first part followed by the web link the announcement leads to, which the
/// HTML links and shows.
fn bodies(announcement: &Announcement, part: u32) -> (String, Option<String>) {
    let text = announcement.part(part);
    let mut plain = text.map(|text| text.body.clone()).unwrap_or_default();
    let mut html = text.and_then(|text| text.html.clone());
    let link = announcement.link.as_deref();
    if let Some(link) = link.filter(|link| part == 0 && web_url(link)) {
        let shown = safe_url(link).replace('&', "&amp;");
        appended(&mut plain, link);
        if let Some(html) = &mut html {
            appended(html, &format!("<p><a href=\"{shown}\">{shown}</a></p>"));
        }
    }
    (plain, html)
}

/// `line` after `body`, a blank line between them where `body` holds any.
fn appended(body: &mut String, line: &str) {
    if !body.is_empty() {
        body.push_str("\n\n");
    }
    body.push_str(line);
}

/// Landed, keeping the message id the JSON body of `answer` names at
/// `field` where herald keeps it: at most [`ID`] bytes of printable ASCII.
pub(crate) fn landed(answer: &Response, field: &str) -> Sent {
    let id = serde_json::from_slice::<serde_json::Value>(&answer.body)
        .ok()
        .and_then(|body| body.get(field)?.as_str().map(str::to_owned))
        .filter(|id| id.len() <= ID && id.bytes().all(|byte| byte.is_ascii_graphic()));
    Sent::Landed(Facts {
        message: id,
        thread: None,
        permalink: None,
    })
}
