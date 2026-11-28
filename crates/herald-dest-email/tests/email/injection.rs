use herald_core::Marker;
use herald_dest_email::{AddrSpec, Letter, MOST_BYTES, Mailbox, MessageId, Recipients, Unparsed};
use jiff::Timestamp;

use crate::judge::{Given, judge};
use crate::read::{mailboxes, read, unstructured};

/// What an attacker puts in the text an announcement carries to add a field,
/// a recipient or a part: line breaks of every kind, a fold, an encoded line
/// break, a blank line, and the message's own boundary.
const ATTACKS: &[&str] = &[
    "x\r\nBcc: victim@example.org",
    "x\nBcc: victim@example.org",
    "x\rBcc: victim@example.org",
    "x\r\n Bcc: victim@example.org",
    "x\r\n\tBcc: victim@example.org",
    "x\r\n\r\nBcc: victim@example.org",
    "x\u{85}Bcc: victim@example.org",
    "x\u{2028}Bcc: victim@example.org",
    "x\u{0}\r\nBcc: victim@example.org",
    "=?UTF-8?Q?x=0D=0ABcc:_victim@example.org?=",
    "=?UTF-8?B?eA0KQmNjOiB2aWN0aW1AZXhhbXBsZS5vcmc=?=",
    "x\r\nTo: victim@example.org, <victim@example.org>",
    "x\r\nContent-Type: text/html\r\n\r\n<script>",
    "\r\n--=_herald\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>added</p>\r\n--=_herald--\r\n",
    "--=_herald--",
    "\r\n.\r\nMAIL FROM:<victim@example.org>\r\n",
];

#[test]
fn no_text_of_an_announcement_adds_a_field_a_recipient_or_a_part_through_the_subject_or_a_body() {
    for attack in ATTACKS {
        for html in [None, Some(*attack)] {
            for replying in [false, true] {
                judge(&Given {
                    subject: attack,
                    text: attack,
                    html,
                    sender: "herald",
                    recipient: "Watchers",
                    address: "to@example.org",
                    replying,
                });
            }
        }
    }
}

#[test]
fn a_subject_carrying_a_line_break_is_one_line_and_names_no_one() {
    let from = Mailbox::parse("herald <herald@example.org>").expect("a mailbox");
    let recipients = Recipients::new(
        vec![Mailbox::parse("to@example.org").expect("a mailbox")],
        vec![],
        vec![],
    )
    .expect("one");
    let id = MessageId::of(&Marker::of(&[1]), 0, from.address());
    for attack in ATTACKS {
        let written = Letter {
            from: &from,
            reply_to: None,
            recipients: &recipients,
            subject: attack,
            text: "Body",
            html: None,
            id: &id,
            replying: None,
            date: Timestamp::UNIX_EPOCH,
        }
        .written(MOST_BYTES)
        .expect("small");
        let head = written.message.split("\r\n\r\n").next().expect("a header");
        assert!(
            head.split("\r\n").all(
                |line| !line.starts_with("Bcc") && !line.starts_with("Content-Type: text/html")
            ),
            "{attack:?}: {head}"
        );
        let message = read(&written.message);
        assert_eq!(
            message.names().iter().filter(|name| **name == "To").count(),
            1
        );
        assert_eq!(
            mailboxes(message.value("To")),
            [(None, "to@example.org".to_owned())]
        );
        assert!(!unstructured(message.value("Subject")).contains(['\r', '\n']));
        assert_eq!(
            written.to,
            [AddrSpec::parse("to@example.org").expect("an address")]
        );
    }
}

#[test]
fn no_address_or_display_name_carrying_a_line_break_or_a_second_address_is_taken() {
    let address = AddrSpec::parse("a@example.org").expect("an address");
    for attack in ATTACKS {
        assert!(AddrSpec::parse(attack).is_err(), "{attack:?}");
        assert!(
            Mailbox::parse(&format!("Name <{attack}>")).is_err(),
            "{attack:?}"
        );
        if attack.contains(char::is_control) {
            assert_eq!(
                Mailbox::parse(&format!("{attack} <a@example.org>")),
                Err(Unparsed::Control)
            );
        }
        if attack.contains(|character: char| {
            character.is_control() || matches!(character, '\u{2028}' | '<' | '>')
        }) {
            assert_eq!(
                Mailbox::new(Some(attack), address.clone()),
                Err(Unparsed::Name),
                "{attack:?}"
            );
        }
    }
    for attack in [
        "a@example.org,victim@example.org",
        "a@example.org;victim@example.org",
        "a@example.org victim@example.org",
        "a@example.org\tvictim@example.org",
        "a@example.org>, <victim@example.org",
        "a@example.org\r\nRCPT TO:<victim@example.org>",
        "a@example.org\0",
        "a(comment)@example.org",
        "\"a@b\"@example.org",
    ] {
        assert!(AddrSpec::parse(attack).is_err(), "{attack:?}");
        assert!(Mailbox::parse(attack).is_err(), "{attack:?}");
    }
}

#[test]
fn a_name_that_looks_like_an_encoded_line_break_is_shown_as_it_is_written() {
    judge(&Given {
        subject: "S",
        text: "B",
        html: None,
        sender: "=?UTF-8?Q?x=0D=0ABcc:_victim@example.org?=",
        recipient: "\"Quoted\" <victim@example.org>, Second",
        address: "to@example.org",
        replying: false,
    });
}
