//! Email through Mailgun: each part's request as `messages.mime` takes it,
//! to its region's host alone, each answer read back into what the ledger
//! records, and what the doctor's probe asks and makes of it.

use herald_core::{Announcement, Delivery, Entry, Facts, Sent, Step, Text, Waits};
use herald_dest_email::{
    Addressing, Email, Letter, MAILGUN_BYTES, Mailbox, Mailgun, MessageId, Recipients, hosted,
};
use herald_kinds::{Probe, Probing, Reach, Recorded, Signing};
use herald_ports::{Base, HttpError, Method, Request, Response, Secret};
use herald_testing::{FakeSigner, ScriptedHttp, sample};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

const NOW: &str = "2026-11-28T12:00:00Z";

const FORM: &str = "=_herald_form";

fn now() -> Timestamp {
    NOW.parse().unwrap()
}

fn mailbox(text: &str) -> Mailbox {
    Mailbox::parse(text).unwrap()
}

fn key() -> String {
    ["key-", &"m".repeat(32)].concat()
}

fn mailgun(region: &str) -> Mailgun {
    Mailgun::new(
        hosted(region).unwrap(),
        "mg.example.org",
        Secret::new("MAILGUN_API_KEY", key()),
    )
}

fn recipients() -> Recipients {
    Recipients::new(
        vec![mailbox("Team <team@example.org>")],
        vec![mailbox("lead@example.org")],
        vec![mailbox("hidden@example.org")],
    )
    .unwrap()
}

fn email_through(mailgun: Mailgun) -> Email {
    Email::new(
        Addressing {
            from: mailbox("herald <releases@example.org>"),
            reply_to: None,
            recipients: recipients(),
            most: 262_144,
        },
        mailgun,
    )
}

fn email() -> Email {
    email_through(mailgun("eu"))
}

fn answer(status: u16, headers: &[(&str, &str)], body: &str) -> Response {
    Response {
        status,
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        body: body.as_bytes().to_vec(),
    }
}

fn ok(body: &Value) -> Response {
    answer(
        200,
        &[("Content-Type", "application/json")],
        &body.to_string(),
    )
}

fn announced() -> Announcement {
    let mut announcement = sample::announcement();
    announcement.link = None;
    announcement.parts = vec![
        Text {
            body: "-- Added the gate\n--=_herald_form".to_owned(),
            html: None,
        },
        Text {
            body: "more".to_owned(),
            html: None,
        },
    ];
    announcement
}

fn sent(email: &Email, http: &ScriptedHttp, step: &Step) -> Sent {
    let announcement = announced();
    let entry = Entry::new(announcement.key);
    pollster::block_on(Reach::send(
        email,
        http,
        Signing {
            signer: &FakeSigner,
            now: now(),
        },
        step,
        &announcement,
        &Delivery::default(),
        &entry,
    ))
    .unwrap()
}

fn header(request: &Request, name: &str) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.redacted())
}

fn body(request: &Request) -> String {
    String::from_utf8(request.body.clone()).unwrap()
}

/// The message part `part` of [`announced`] is, as the writer writes it.
fn expected(part: usize, text: &str) -> String {
    let from = mailbox("herald <releases@example.org>");
    let recipients = recipients();
    let first = MessageId::of(&sample::marker(), 0, from.address());
    let id = MessageId::of(&sample::marker(), part, from.address());
    Letter {
        from: &from,
        reply_to: None,
        recipients: &recipients,
        subject: "📦 php-mutation-gate v0.1.0",
        text,
        html: None,
        id: &id,
        replying: (part > 0).then_some(&first),
        date: now(),
    }
    .written(262_144)
    .unwrap()
    .message
}

/// The form fields of `form`, each its name and value, split at the form's
/// own delimiter alone.
fn fields(form: &str) -> Vec<(String, String)> {
    let delimiter = format!("--{FORM}");
    let rest = form.strip_prefix(&format!("{delimiter}\r\n")).unwrap();
    let rest = rest
        .strip_suffix(&format!("\r\n{delimiter}--\r\n"))
        .unwrap();
    rest.split(&format!("\r\n{delimiter}\r\n"))
        .map(|part| {
            let (head, value) = part.split_once("\r\n\r\n").unwrap();
            let name = head
                .split("name=\"")
                .nth(1)
                .and_then(|named| named.split('"').next())
                .unwrap();
            (name.to_owned(), value.to_owned())
        })
        .collect()
}

#[test]
fn a_part_is_sent_whole_as_a_form_with_every_recipient_beside_it_and_tracking_off() {
    let http = ScriptedHttp::new(vec![ok(&json!({
        "id": "<20261128120000.1.2@mg.example.org>",
        "message": "Queued. Thank you.",
    }))]);
    assert_eq!(
        sent(&email(), &http, &Step::Post),
        Sent::Landed(Facts {
            message: Some("<20261128120000.1.2@mg.example.org>".to_owned()),
            thread: None,
            permalink: None,
        })
    );
    let request = &http.requests()[0];
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.url.redacted(),
        "https://api.eu.mailgun.net/v3/mg.example.org/messages.mime"
    );
    assert_eq!(
        header(request, "Content-Type").as_deref(),
        Some("multipart/form-data; boundary=\"=_herald_form\"")
    );
    let form = body(request);
    let message = expected(0, "-- Added the gate\n--=_herald_form");
    let named = |name: &str| -> Vec<String> {
        fields(&form)
            .into_iter()
            .filter(|(field, _)| field == name)
            .map(|(_, value)| value)
            .collect()
    };
    assert_eq!(
        named("to"),
        ["team@example.org", "lead@example.org", "hidden@example.org"]
    );
    assert_eq!(named("o:tracking-clicks"), ["no"]);
    assert_eq!(named("o:tracking-opens"), ["no"]);
    assert_eq!(named("message"), std::slice::from_ref(&message));
    assert_eq!(fields(&form).len(), 6);
    assert!(form.contains(
        "Content-Disposition: form-data; name=\"message\"; filename=\"message.mime\"\r\nContent-Type: message/rfc822\r\n\r\n"
    ));
    for unwritten in ["Bcc", "hidden@"] {
        assert!(!message.contains(unwritten), "{unwritten}\n{message}");
    }
    assert_eq!(form.matches(&format!("\r\n--{FORM}")).count(), 6);
}

#[test]
fn a_later_part_replies_to_the_first() {
    let http = ScriptedHttp::new(vec![ok(&json!({})), ok(&json!({}))]);
    for step in [Step::Post, Step::Chunk(1)] {
        assert_eq!(sent(&email(), &http, &step), Sent::Landed(Facts::default()));
    }
    let later = &http.requests()[1];
    let message = fields(&body(later))
        .into_iter()
        .find(|(name, _)| name == "message")
        .unwrap()
        .1;
    assert_eq!(message, expected(1, "more"));
}

#[test]
fn a_request_carries_the_key_as_basic_credentials_shown_by_its_name_alone() {
    let http = ScriptedHttp::new(vec![ok(&json!({}))]);
    sent(&email(), &http, &Step::Post);
    let request = &http.requests()[0];
    assert_eq!(
        header(request, "Authorization").as_deref(),
        Some("Basic «secret:MAILGUN_API_KEY»")
    );
    let given = request
        .headers
        .iter()
        .find(|(name, _)| name == "Authorization")
        .unwrap()
        .1
        .expose();
    assert_eq!(
        given,
        format!("Basic {}", herald_core::basic("api", &key()))
    );
    let shown = format!("{:?} {request} {:?}", email(), mailgun("us"));
    assert!(!shown.contains(&key()), "{shown}");
    assert!(
        !shown.contains(&herald_core::basic("api", &key())),
        "{shown}"
    );
}

#[test]
fn each_region_is_reached_at_its_own_host_alone() {
    assert_eq!(hosted("us"), Ok("https://api.mailgun.net"));
    assert_eq!(hosted("eu"), Ok("https://api.eu.mailgun.net"));
    for unknown in ["", "US", "au", "eu-west-1"] {
        assert!(hosted(unknown).is_err(), "{unknown}");
    }
    assert_eq!(email().base(), Base::At("https://api.eu.mailgun.net"));
    assert_eq!(
        email_through(mailgun("us")).base(),
        Base::At("https://api.mailgun.net")
    );
    assert_eq!(MAILGUN_BYTES, 25_000_000);
}

#[test]
fn each_answer_is_read_by_its_status_and_no_message_of_it_is_repeated() {
    let waits = Waits {
        outage: SignedDuration::from_secs(11),
        limited: SignedDuration::from_secs(13),
    };
    let email = email().waiting(waits);
    let json = [("Content-Type", "application/json")];
    for (given, said) in [
        (
            answer(400, &json, r#"{"message":"to parameter is not a valid address: team@example.org"}"#),
            Sent::Refused("Mailgun refused it with 400".to_owned()),
        ),
        (
            answer(401, &[], "Forbidden"),
            Sent::Refused(
                "Mailgun refused it with 401; check the credential herald sends, and that it has not expired"
                    .to_owned(),
            ),
        ),
        (
            answer(420, &json, r#"{"message":"limit"}"#),
            Sent::RetryAfter(SignedDuration::from_secs(13)),
        ),
        (
            answer(429, &[("Retry-After", "7")], ""),
            Sent::RetryAfter(SignedDuration::from_secs(7)),
        ),
        (
            answer(429, &[], ""),
            Sent::RetryAfter(SignedDuration::from_secs(13)),
        ),
        (
            answer(500, &json, r#"{"message":"oops"}"#),
            Sent::RetryAfter(SignedDuration::from_secs(11)),
        ),
    ] {
        let http = ScriptedHttp::new(vec![given]);
        assert_eq!(sent(&email, &http, &Step::Post), said);
        assert_eq!(http.requests().len(), 1);
    }
}

#[test]
fn a_redirect_is_never_followed_with_the_key() {
    let http = ScriptedHttp::new(vec![answer(
        307,
        &[(
            "Location",
            "https://elsewhere.example/v3/mg.example.org/messages.mime",
        )],
        "",
    )]);
    let said = sent(&email(), &http, &Step::Post);
    assert!(matches!(said, Sent::Refused(_)), "{said:?}");
    assert_eq!(http.requests().len(), 1);
}

#[test]
fn a_message_id_herald_would_not_keep_is_dropped() {
    for id in [json!("has space"), json!("x".repeat(257)), json!(7)] {
        let http = ScriptedHttp::new(vec![ok(&json!({ "id": id }))]);
        assert_eq!(
            sent(&email(), &http, &Step::Post),
            Sent::Landed(Facts::default())
        );
    }
    let http = ScriptedHttp::new(vec![ok(&json!({ "id": "x".repeat(256) }))]);
    assert_eq!(
        sent(&email(), &http, &Step::Post),
        Sent::Landed(Facts {
            message: Some("x".repeat(256)),
            thread: None,
            permalink: None,
        })
    );
}

#[test]
fn a_refusal_is_mailgun_s_own_where_it_carries_mailgun_s_message() {
    let json = [("Content-Type", "application/json")];
    assert!(email().declines(&answer(400, &json, r#"{"message":"no"}"#)));
    assert!(!email().declines(&answer(400, &[], r#"{"message":"no"}"#)));
    assert!(!email().declines(&answer(400, &json, r#"{"message":7}"#)));
    assert!(!email().declines(&answer(400, &json, r#"{"detail":"no"}"#)));
    assert!(!email().declines(&answer(403, &json, "not json")));
}

fn probed(answers: Vec<Response>) -> (Vec<Probe>, Vec<Request>) {
    let http = ScriptedHttp::new(answers);
    let probes = pollster::block_on(email().probe(
        &http,
        &Delivery::default(),
        Probing {
            recorded: Recorded::Kept,
            now: now(),
        },
    ));
    (probes, http.requests())
}

#[test]
fn the_doctor_reads_the_domain_with_the_key_and_sends_nothing() {
    let (probes, requests) = probed(vec![ok(&json!({
        "domain": { "is_disabled": false, "type": "custom", "state": "active" }
    }))]);
    assert_eq!(probes, [Probe::Taken]);
    let request = &requests[0];
    assert_eq!(request.method, Method::Get);
    assert_eq!(
        request.url.redacted(),
        "https://api.eu.mailgun.net/v4/domains/mg.example.org"
    );
    assert_eq!(request.body.len(), 0);
    assert_eq!(
        header(request, "Authorization").as_deref(),
        Some("Basic «secret:MAILGUN_API_KEY»")
    );
}

#[test]
fn the_doctor_says_where_the_domain_is_disabled_or_a_sandbox() {
    let (probes, _) = probed(vec![ok(&json!({
        "domain": { "is_disabled": true, "type": "sandbox" }
    }))]);
    assert_eq!(
        probes,
        [
            Probe::Taken,
            Probe::Route {
                key: None,
                why: "Mailgun has the domain disabled".to_owned(),
            },
            Probe::Said(
                "the domain is a Mailgun sandbox domain: it sends only to the recipients the account authorised (https://documentation.mailgun.com/docs/mailgun/user-manual/domains/domains-sandbox)"
                    .to_owned()
            ),
        ]
    );
    let (unsaid, _) = probed(vec![ok(&json!({}))]);
    assert_eq!(unsaid, [Probe::Taken]);
}

#[test]
fn the_doctor_says_a_key_that_may_not_read_the_domain_may_still_send() {
    for status in [401, 403] {
        let (probes, _) = probed(vec![answer(status, &[], "Forbidden")]);
        assert_eq!(
            probes,
            [Probe::Said(format!(
                "Mailgun does not let its key read the domain, answering {status}: a domain sending key may only send, so whether this one sends is not asked; a key Mailgun does not take is answered so too"
            ))]
        );
    }
    let (missing, _) = probed(vec![answer(404, &[], r#"{"message":"Domain not found"}"#)]);
    assert_eq!(
        missing,
        [Probe::Credential(
            "Mailgun knows no such domain in its region, answering 404; check domain and region"
                .to_owned()
        )]
    );
    let (failed, _) = probed(vec![answer(500, &[], "")]);
    assert_eq!(
        failed,
        [Probe::Credential(
            "Mailgun could not be asked, answering 500".to_owned()
        )]
    );
    let (unasked, _) = probed(Vec::new());
    assert_eq!(
        unasked,
        [Probe::Credential(format!(
            "Mailgun could not be asked: {}",
            HttpError::TimedOut
        ))]
    );
}
