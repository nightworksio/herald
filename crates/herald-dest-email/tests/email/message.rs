use herald_core::Marker;
use herald_dest_email::{
    AUTO_SUBMITTED, BOUNDARY, ENCODED_LINE, LINE, Letter, MOST_BYTES, MOST_RECIPIENTS, Mailbox,
    MessageId, Oversized, QUOTED_LINE, Recipients, Unrecipiented, subject_line,
};
use jiff::Timestamp;

use crate::read::{as_lines, mailboxes, quoted_printable, read, unstructured};

fn mailbox(text: &str) -> Mailbox {
    Mailbox::parse(text).expect("a mailbox")
}

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).expect("a time")
}

const NOON: i64 = 1_795_853_051;

struct Fixture {
    from: Mailbox,
    recipients: Recipients,
    id: MessageId,
}

impl Fixture {
    fn new() -> Self {
        let from = mailbox("herald <herald@example.org>");
        let id = MessageId::of(&Marker::of(&[0x0f; 4]), 0, from.address());
        let recipients =
            Recipients::new(vec![mailbox("ops@example.org")], vec![], vec![]).expect("one");
        Self {
            from,
            recipients,
            id,
        }
    }

    fn letter<'a>(&'a self, subject: &'a str, text: &'a str) -> Letter<'a> {
        Letter {
            from: &self.from,
            reply_to: None,
            recipients: &self.recipients,
            subject,
            text,
            html: None,
            id: &self.id,
            replying: None,
            date: at(NOON),
        }
    }
}

#[test]
fn a_reply_is_written_with_its_alternatives_and_threading_fields_and_no_blind_copy() {
    let from = mailbox("herald <herald@example.org>");
    let reply_to = mailbox("Maintainers <maintainers@example.org>");
    let recipients = Recipients::new(
        vec![
            mailbox("Release watchers <watchers@example.org>"),
            mailbox("ops@example.org"),
        ],
        vec![mailbox("Ünïcode Team <team@example.org>")],
        vec![mailbox("Hidden <hidden@example.org>")],
    )
    .expect("four recipients");
    let marker = Marker::of(&[0x0f; 4]);
    let root = MessageId::of(&marker, 0, from.address());
    let id = MessageId::of(&marker, 1, from.address());
    let written = Letter {
        from: &from,
        reply_to: Some(&reply_to),
        recipients: &recipients,
        subject: "herald 1.2.0 is out",
        text: "Line one\nLine two",
        html: Some("<p>Line one</p>"),
        id: &id,
        replying: Some(&root),
        date: at(NOON),
    }
    .written(MOST_BYTES)
    .expect("small");
    assert_eq!(
        written.message,
        "Date: Sat, 28 Nov 2026 08:04:11 +0000\r\n\
         From: \"herald\" <herald@example.org>\r\n\
         Reply-To: \"Maintainers\" <maintainers@example.org>\r\n\
         To: \"Release watchers\" <watchers@example.org>, <ops@example.org>\r\n\
         Cc: =?UTF-8?Q?=C3=9Cn=C3=AFcode_Team?= <team@example.org>\r\n\
         Subject: herald 1.2.0 is out\r\n\
         Message-ID: <0f0f0f0f.1@example.org>\r\n\
         In-Reply-To: <0f0f0f0f@example.org>\r\n\
         References: <0f0f0f0f@example.org>\r\n\
         Auto-Submitted: auto-generated\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/alternative; boundary=\"=_herald\"\r\n\
         \r\n\
         --=_herald\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: quoted-printable\r\n\
         \r\n\
         Line one\r\n\
         Line two\r\n\
         \r\n\
         --=_herald\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Transfer-Encoding: quoted-printable\r\n\
         \r\n\
         <p>Line one</p>\r\n\
         \r\n\
         --=_herald--\r\n"
    );
    assert_eq!(written.sender.to_string(), "herald@example.org");
    let to: Vec<String> = written.to.iter().map(ToString::to_string).collect();
    assert_eq!(
        to,
        [
            "watchers@example.org",
            "ops@example.org",
            "team@example.org",
            "hidden@example.org"
        ]
    );
    let message = read(&written.message);
    assert_eq!(
        mailboxes(message.value("Cc")),
        [(
            Some("Ünïcode Team".to_owned()),
            "team@example.org".to_owned()
        )]
    );
    assert_eq!(message.parts.len(), 2);
    assert_eq!(BOUNDARY, "=_herald");
    assert_eq!(AUTO_SUBMITTED, "auto-generated");
}

#[test]
fn a_message_without_html_is_one_plain_part_and_names_only_the_fields_it_fills() {
    let fixture = Fixture::new();
    let hidden = Recipients::new(vec![], vec![], vec![mailbox("hidden@example.org")]).expect("one");
    let letter = Letter {
        recipients: &hidden,
        ..fixture.letter("Plain", "Body")
    };
    let written = letter.written(MOST_BYTES).expect("small");
    let message = read(&written.message);
    assert_eq!(
        message.names(),
        [
            "Date",
            "From",
            "Subject",
            "Message-ID",
            "Auto-Submitted",
            "MIME-Version",
            "Content-Type",
            "Content-Transfer-Encoding"
        ]
    );
    assert!(written.message.ends_with(
        "Content-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: quoted-printable\r\n\
         \r\n\
         Body\r\n"
    ));
    assert_eq!(message.value("Message-ID"), "<0f0f0f0f@example.org>");
    assert!(!written.message.contains("hidden"));
    assert_eq!(written.to.len(), 1);
}

#[test]
fn a_message_is_dated_in_utc_with_a_two_digit_day() {
    let fixture = Fixture::new();
    let december = Letter {
        date: at(1_796_083_200),
        ..fixture.letter("Dated", "Body")
    };
    let message = read(&december.written(MOST_BYTES).expect("small").message);
    assert_eq!(message.value("Date"), "Tue, 01 Dec 2026 00:00:00 +0000");
}

#[test]
fn a_message_longer_than_its_bound_is_refused_by_its_length_alone() {
    let fixture = Fixture::new();
    let letter = fixture.letter("Bounded", "Body");
    let length = letter.written(MOST_BYTES).expect("small").message.len();
    assert_eq!(
        letter.written(length).expect("at its bound").message.len(),
        length
    );
    let refused = letter.written(length - 1).expect_err("one byte over");
    assert_eq!(
        refused,
        Oversized {
            bytes: length,
            most: length - 1
        }
    );
    assert_eq!(
        refused.to_string(),
        format!(
            "the message would be {length} bytes, more than the {} its route allows",
            length - 1
        )
    );
    assert_eq!(MOST_BYTES, 256 * 1024);
}

#[test]
fn recipients_name_at_least_one_and_at_most_a_hundred_addresses_each_once() {
    let many = |count: usize| -> Vec<Mailbox> {
        (0..count)
            .map(|n| mailbox(&format!("r{n}@example.org")))
            .collect()
    };
    assert_eq!(
        Recipients::new(vec![], vec![], vec![]),
        Err(Unrecipiented::None)
    );
    let most = Recipients::new(
        many(MOST_RECIPIENTS - 2),
        many(0),
        vec![mailbox("x@example.org"), mailbox("y@example.org")],
    )
    .expect("a hundred");
    assert_eq!(most.envelope().count(), MOST_RECIPIENTS);
    assert_eq!(
        most.bcc(),
        [mailbox("x@example.org"), mailbox("y@example.org")]
    );
    assert_eq!(
        Recipients::new(
            many(MOST_RECIPIENTS),
            vec![],
            vec![mailbox("x@example.org")]
        ),
        Err(Unrecipiented::Many)
    );
    assert_eq!(
        Recipients::new(
            vec![mailbox("A <a@Example.org>")],
            vec![],
            vec![mailbox("a@example.ORG")]
        ),
        Err(Unrecipiented::Twice)
    );
    assert_eq!(
        Recipients::new(
            vec![],
            vec![mailbox("a@example.org"), mailbox("a@example.org")],
            vec![]
        ),
        Err(Unrecipiented::Twice)
    );
    let cased = Recipients::new(
        vec![mailbox("A@example.org")],
        vec![mailbox("a@example.org")],
        vec![],
    )
    .expect("a local part keeps its case");
    assert_eq!(cased.to(), [mailbox("A@example.org")]);
    assert_eq!(cased.cc(), [mailbox("a@example.org")]);
    assert_eq!(cased.bcc(), &[] as &[Mailbox]);
    assert_eq!(MOST_RECIPIENTS, 100);
    for (why, said) in [
        (
            Unrecipiented::None,
            "names no recipient: give `to`, `cc` or `bcc` at least one address",
        ),
        (
            Unrecipiented::Many,
            "names more than the 100 recipients every mail server takes in one message",
        ),
        (
            Unrecipiented::Twice,
            "names one address twice among `to`, `cc` and `bcc`",
        ),
    ] {
        assert_eq!(why.to_string(), said);
    }
}

#[test]
fn a_message_id_names_its_part_after_the_announcement_s_marker_at_the_sender_s_domain() {
    let from = mailbox("a@lists.example.org");
    let marker = Marker::of(&[0xab, 0xcd]);
    assert_eq!(
        MessageId::of(&marker, 0, from.address()).to_string(),
        "<abcd@lists.example.org>"
    );
    assert_eq!(
        MessageId::of(&marker, 2, from.address()).as_str(),
        "abcd.2@lists.example.org"
    );
}

#[test]
fn a_subject_is_one_line_with_no_control_character() {
    for (title, line) in [
        ("  herald  1.2\t\tout  ", "herald 1.2 out"),
        ("a\r\nBcc: v@example.org", "a Bcc: v@example.org"),
        ("bell\u{7}ed\u{0} \u{1b}[31m", "belled [31m"),
        ("next\u{85}line\u{2028}sep", "next line sep"),
        ("\u{7f}\u{9b}", ""),
        ("", ""),
    ] {
        assert_eq!(subject_line(title), line, "{title:?}");
    }
}

#[test]
fn a_long_plain_subject_folds_between_its_words_within_78_characters() {
    let fixture = Fixture::new();
    let subject = ["release"; 40].join(" ");
    let written = fixture
        .letter(&subject, "Body")
        .written(MOST_BYTES)
        .expect("small");
    let head = written.message.split("\r\n\r\n").next().expect("a header");
    let folded: Vec<&str> = head
        .split("\r\n")
        .skip_while(|line| !line.starts_with("Subject:"))
        .take_while(|line| line.starts_with("Subject:") || line.starts_with(' '))
        .collect();
    assert!(folded.len() > 3, "{folded:?}");
    assert!(
        folded
            .iter()
            .all(|line| line.len() <= LINE && !line.contains("=?")),
        "{folded:?}"
    );
    assert_eq!(
        unstructured(read(&written.message).value("Subject")),
        subject
    );
}

#[test]
fn a_subject_is_encoded_where_it_is_not_plain_ascii_or_reads_as_an_encoded_word_or_has_a_long_word()
{
    let fixture = Fixture::new();
    let long_word = "x".repeat(LINE);
    let fitting_word = "y".repeat(LINE - 1);
    for (subject, encoded) in [
        ("Zoë ships 🚀 v2 — finally", true),
        ("=?UTF-8?Q?spoofed?=", true),
        ("a =? b", true),
        (long_word.as_str(), true),
        (fitting_word.as_str(), false),
        ("under_score = equals?", false),
        (&"日本語のリリースノート".repeat(12), true),
        (&"🚀".repeat(40), true),
    ] {
        let written = fixture
            .letter(subject, "Body")
            .written(MOST_BYTES)
            .expect("small");
        let message = read(&written.message);
        let value = message.value("Subject");
        assert_eq!(
            value.starts_with("=?UTF-8?Q?"),
            encoded,
            "{subject:?}: {value}"
        );
        assert_eq!(unstructured(value), subject, "{subject:?}");
        let head = written.message.split("\r\n\r\n").next().expect("a header");
        for line in head.split("\r\n") {
            assert!(
                line.len()
                    <= if line.contains("=?") {
                        ENCODED_LINE
                    } else {
                        LINE
                    },
                "{line}"
            );
        }
    }
}

#[test]
fn a_name_is_quoted_where_it_is_plain_and_fits_a_line_and_encoded_where_not() {
    let address = || mailbox("n@example.org").address().clone();
    let plain = "p".repeat(LINE - 3);
    let long = "q".repeat(LINE - 2);
    for (name, quoted) in [
        ("Plain Name", true),
        (plain.as_str(), true),
        (long.as_str(), false),
        ("Looks =?UTF-8?Q?x?=", false),
        ("Zoë", false),
        ("O'Brien, Ops (on call)", true),
    ] {
        let from = Mailbox::new(Some(name), address()).expect("a name");
        let fixture = Fixture::new();
        let written = Letter {
            from: &from,
            ..fixture.letter("S", "B")
        }
        .written(MOST_BYTES)
        .expect("small");
        let message = read(&written.message);
        assert_eq!(message.value("From").starts_with('"'), quoted, "{name}");
        assert_eq!(
            mailboxes(message.value("From")),
            [(Some(name.to_owned()), "n@example.org".to_owned())],
            "{name}"
        );
    }
}

#[test]
fn many_recipients_fold_after_their_commas() {
    let to: Vec<Mailbox> = (0..30)
        .map(|n| mailbox(&format!("Watcher number {n} <watcher-{n}@example.org>")))
        .collect();
    let recipients = Recipients::new(to.clone(), vec![], vec![]).expect("thirty");
    let fixture = Fixture::new();
    let written = Letter {
        recipients: &recipients,
        ..fixture.letter("S", "B")
    }
    .written(MOST_BYTES)
    .expect("small");
    let head = written.message.split("\r\n\r\n").next().expect("a header");
    assert!(head.split("\r\n").all(|line| line.len() <= LINE));
    let seen = mailboxes(read(&written.message).value("To"));
    let meant: Vec<_> = to
        .iter()
        .map(|mailbox| {
            (
                mailbox.name().map(str::to_owned),
                mailbox.address().to_string(),
            )
        })
        .collect();
    assert_eq!(seen, meant);
}

#[test]
fn a_body_is_quoted_printable_within_76_characters_a_line_and_reads_back_line_for_line() {
    let fixture = Fixture::new();
    let exact = "e".repeat(QUOTED_LINE);
    let over = "o".repeat(QUOTED_LINE + 1);
    let equals = "=".repeat(40);
    let edge = format!("{}é", "a".repeat(QUOTED_LINE - 3));
    for (text, encoded) in [
        (exact.as_str(), exact.clone()),
        (
            over.as_str(),
            format!("{}=\r\noo", "o".repeat(QUOTED_LINE - 1)),
        ),
        (
            "trailing space \nand tab\t",
            "trailing space=20\r\nand tab=09".to_owned(),
        ),
        ("a = b", "a =3D b".to_owned()),
        ("crlf\r\ncr\rlf\n", "crlf\r\ncr\r\nlf".to_owned()),
        ("", String::new()),
        ("\n\n", "\r\n".to_owned()),
        (
            edge.as_str(),
            format!("{}=\r\n=C3=A9", "a".repeat(QUOTED_LINE - 3)),
        ),
        (
            equals.as_str(),
            format!("{}=\r\n{}", "=3D".repeat(25), "=3D".repeat(15)),
        ),
        (
            "--=_herald\r\n--=_herald--",
            "--=3D_herald\r\n--=3D_herald--".to_owned(),
        ),
    ] {
        let written = fixture
            .letter("S", text)
            .written(MOST_BYTES)
            .expect("small");
        let body = written.message.split_once("\r\n\r\n").expect("a body").1;
        assert_eq!(body, format!("{encoded}\r\n"), "{text:?}");
        assert!(
            body.split("\r\n").all(|line| line.len() <= QUOTED_LINE),
            "{text:?}"
        );
        assert_eq!(quoted_printable(body), as_lines(text), "{text:?}");
    }
}

#[test]
fn encoded_words_fill_the_room_left_on_their_line_and_fold_only_where_none_is_left() {
    let fixture = Fixture::new();
    let written = fixture
        .letter(
            "Grüße from Köln: herald 2.0 is out, with many improvements for everyone",
            "B",
        )
        .written(MOST_BYTES)
        .expect("small");
    let message = read(&written.message);
    let head = written.message.split("\r\n\r\n").next().expect("a header");
    let subject: Vec<&str> = head
        .split("\r\n")
        .skip_while(|line| !line.starts_with("Subject:"))
        .take_while(|line| line.starts_with("Subject:") || line.starts_with(' '))
        .collect();
    assert_eq!(
        subject,
        [
            "Subject: =?UTF-8?Q?Gr=C3=BC=C3=9Fe_from_K=C3=B6ln=3A_herald_2=2E0_is_out?=",
            " =?UTF-8?Q?=2C_with_many_improvements_for_everyone?=",
        ]
    );
    assert_eq!(
        unstructured(message.value("Subject")),
        "Grüße from Köln: herald 2.0 is out, with many improvements for everyone"
    );
    let recipients = Recipients::new(
        vec![
            mailbox("Plain <watcher-number-one@lists.example.org>"),
            mailbox("Zoë <z@example.org>"),
        ],
        vec![],
        vec![],
    )
    .expect("two");
    let written = Letter {
        recipients: &recipients,
        ..fixture.letter("S", "B")
    }
    .written(MOST_BYTES)
    .expect("small");
    let to: Vec<&str> = written
        .message
        .split("\r\n")
        .skip_while(|line| !line.starts_with("To:"))
        .take_while(|line| line.starts_with("To:") || line.starts_with(' '))
        .collect();
    assert_eq!(
        to,
        [
            "To: \"Plain\" <watcher-number-one@lists.example.org>, =?UTF-8?Q?Zo=C3=AB?=",
            " <z@example.org>",
        ]
    );
}
