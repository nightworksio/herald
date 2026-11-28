//! What every message herald writes holds, whatever text it is given: it
//! reads back to exactly the fields, recipients and parts herald meant, and
//! no text adds a field, a recipient or a part. The property test and the
//! fuzz target `email` judge through this file.

use herald_core::Marker;
use herald_dest_email::{
    AddrSpec, Letter, Mailbox, MessageId, NAME_MOST, Recipients, subject_line,
};
use jiff::Timestamp;

use crate::read::{Seen, as_lines, mailboxes, message_id, read, unstructured};

/// The text a message is written from.
#[derive(Debug, Clone, Copy)]
pub struct Given<'a> {
    /// The rendered title.
    pub subject: &'a str,
    /// The plain-text body.
    pub text: &'a str,
    /// The HTML body, where there is one.
    pub html: Option<&'a str>,
    /// A display name for the sender.
    pub sender: &'a str,
    /// A display name for the recipient and the reply address.
    pub recipient: &'a str,
    /// An address to send to.
    pub address: &'a str,
    /// Whether the message replies to a thread's first.
    pub replying: bool,
}

const HIDDEN: &str = "hidden@example.org";

/// `given` written and read back, every property checked.
pub fn judge(given: &Given<'_>) {
    let from = named(given.sender, "herald@example.org");
    let reply_to = named(given.recipient, "replies@example.org");
    let named_to = named(given.recipient, "named@example.org");
    let cc = Mailbox::parse("copied@example.org").expect("an address");
    let bcc = Mailbox::parse(HIDDEN).expect("an address");
    let mut to = address(given.address);
    if [&named_to, &cc, &bcc]
        .iter()
        .any(|other| other.address().same(&to))
    {
        to = AddrSpec::parse("to@example.org").expect("an address");
    }
    let to = Mailbox::new(None, to).expect("no name is always taken");
    let recipients = Recipients::new(
        vec![to.clone(), named_to.clone()],
        vec![cc.clone()],
        vec![bcc.clone()],
    )
    .expect("four distinct recipients");
    let marker = Marker::of(&[0xa5; 32]);
    let root = MessageId::of(&marker, 0, from.address());
    let id = MessageId::of(&marker, 1, from.address());
    let letter = Letter {
        from: &from,
        reply_to: Some(&reply_to),
        recipients: &recipients,
        subject: given.subject,
        text: given.text,
        html: given.html,
        id: if given.replying { &id } else { &root },
        replying: given.replying.then_some(&root),
        date: Timestamp::from_second(1_795_853_051).expect("a time"),
    };
    let written = letter.written(usize::MAX).expect("no bound");
    let message = read(&written.message);

    let mut names = vec![
        "Date",
        "From",
        "Reply-To",
        "To",
        "Cc",
        "Subject",
        "Message-ID",
    ];
    if given.replying {
        names.extend(["In-Reply-To", "References"]);
    }
    names.extend(["Auto-Submitted", "MIME-Version", "Content-Type"]);
    if given.html.is_none() {
        names.push("Content-Transfer-Encoding");
    }
    assert_eq!(message.names(), names);

    assert_eq!(
        unstructured(message.value("Subject")),
        subject_line(given.subject)
    );
    assert_eq!(mailboxes(message.value("From")), [seen(&from)]);
    assert_eq!(mailboxes(message.value("Reply-To")), [seen(&reply_to)]);
    assert_eq!(mailboxes(message.value("To")), [seen(&to), seen(&named_to)]);
    assert_eq!(mailboxes(message.value("Cc")), [seen(&cc)]);
    assert!(
        !written.message.contains(HIDDEN),
        "a blind copy stands in no field"
    );
    assert_eq!(written.sender, *from.address());
    assert_eq!(
        written.to,
        [
            to.address(),
            named_to.address(),
            cc.address(),
            bcc.address()
        ]
        .map(Clone::clone)
    );
    if given.replying {
        assert_eq!(message_id(message.value("Message-ID")), id.as_str());
        assert_eq!(message_id(message.value("In-Reply-To")), root.as_str());
        assert_eq!(message_id(message.value("References")), root.as_str());
    } else {
        assert_eq!(message_id(message.value("Message-ID")), root.as_str());
    }
    assert_eq!(message.value("Auto-Submitted"), "auto-generated");

    let mut parts = vec![("text/plain".to_owned(), as_lines(given.text))];
    if let Some(html) = given.html {
        parts.push(("text/html".to_owned(), as_lines(html)));
    }
    assert_eq!(message.parts, parts);
}

/// The address `text` writes where herald takes it, checked to hold nothing
/// but what an address may; another address where it is refused.
fn address(text: &str) -> AddrSpec {
    match AddrSpec::parse(text) {
        Ok(address) => {
            assert_eq!(address.to_string(), text);
            assert!(text.bytes().all(
                |byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~.@".contains(&byte)
            ));
            assert_eq!(text.matches('@').count(), 1);
            address
        }
        Err(_) => AddrSpec::parse("to@example.org").expect("an address"),
    }
}

/// `address` shown as `name` where herald takes the name, else unnamed, the
/// name checked to be refused only where it holds what a name may not.
fn named(name: &str, address: &str) -> Mailbox {
    let address = AddrSpec::parse(address).expect("an address");
    if let Ok(mailbox) = Mailbox::new(Some(name), address.clone()) {
        return mailbox;
    }
    let name = name.trim();
    assert!(
        name.len() > NAME_MOST
            || name.chars().any(|character| character.is_control()
                || matches!(character, '\u{2028}' | '\u{2029}' | '"' | '\\' | '<' | '>')),
        "a name is refused only for what it holds"
    );
    Mailbox::new(None, address).expect("no name is always taken")
}

fn seen(mailbox: &Mailbox) -> Seen {
    (
        mailbox.name().map(str::to_owned),
        mailbox.address().to_string(),
    )
}
