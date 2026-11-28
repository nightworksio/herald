//! Email through Amazon SES: each part's request as `SendEmail` takes it,
//! signed for the region's endpoint alone, each answer read back into what
//! the ledger records, and what the doctor's probe asks and makes of it.

use std::fmt::Debug;

use herald_core::{
    Announcement, Chore, Delivery, Entry, Facts, Sent, Step, Text, Waits, base64_of,
};
use herald_dest_email::{
    Addressing, Credentials, Email, Letter, Mailbox, MessageId, Recipients, Ses, regional,
};
use herald_kinds::{Audience, Probe, Probing, Reach, Recorded, Signing};
use herald_ports::{Base, HttpError, Method, Request, Response, Secret};
use herald_testing::{FakeSigner, ScriptedHttp, sample};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

const NOW: &str = "2026-11-28T12:00:00Z";

fn now() -> Timestamp {
    NOW.parse().unwrap()
}

fn mailbox(text: &str) -> Mailbox {
    Mailbox::parse(text).unwrap()
}

fn credentials(token: bool) -> Credentials {
    Credentials {
        access_key_id: Secret::new("AWS_ACCESS_KEY_ID", ["id-", "for-", "tests"].concat()),
        secret_access_key: Secret::new("AWS_SECRET_ACCESS_KEY", "k".repeat(40)),
        session_token: token.then(|| Secret::new("AWS_SESSION_TOKEN", "t".repeat(24))),
    }
}

fn ses(url: &str, token: bool, set: Option<&str>) -> Ses {
    Ses::new(url, "eu-west-1", credentials(token), set.map(str::to_owned))
}

fn regional_ses() -> Ses {
    ses(&regional("eu-west-1", false, false).unwrap(), false, None)
}

fn email_through(ses: Ses, most: usize) -> Email {
    Email::new(
        Addressing {
            from: mailbox("herald <releases@example.org>"),
            reply_to: Some(mailbox("desk@example.org")),
            recipients: recipients(),
            most,
        },
        ses,
    )
}

fn email() -> Email {
    email_through(regional_ses(), 262_144)
}

fn ok(body: &Value) -> Response {
    Response {
        status: 200,
        headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
        body: body.to_string().into_bytes(),
    }
}

fn refused(status: u16, headers: &[(&str, &str)], body: &str) -> Response {
    Response {
        status,
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        body: body.as_bytes().to_vec(),
    }
}

fn sent(email: &Email, http: &ScriptedHttp, step: &Step, announcement: &Announcement) -> Sent {
    let signer = FakeSigner;
    let entry = Entry::new(announcement.key);
    pollster::block_on(Reach::send(
        email,
        http,
        Signing {
            signer: &signer,
            now: now(),
        },
        step,
        announcement,
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

fn recipients() -> Recipients {
    Recipients::new(
        vec![mailbox("Team <team@example.org>")],
        vec![mailbox("lead@example.org")],
        vec![mailbox("hidden@example.org"), mailbox("audit@example.org")],
    )
    .unwrap()
}

/// The message part `part` of [`announced`] is, as the writer writes it.
fn expected(part: usize, (text, html): (&str, &str)) -> String {
    let from = mailbox("herald <releases@example.org>");
    let reply_to = mailbox("desk@example.org");
    let recipients = recipients();
    let first = MessageId::of(&sample::marker(), 0, from.address());
    let id = MessageId::of(&sample::marker(), part, from.address());
    Letter {
        from: &from,
        reply_to: Some(&reply_to),
        recipients: &recipients,
        subject: "📦 php-mutation-gate v0.1.0",
        text,
        html: Some(html),
        id: &id,
        replying: (part > 0).then_some(&first),
        date: now(),
    }
    .written(262_144)
    .unwrap()
    .message
}

fn data(body: &Value) -> &str {
    body["Content"]["Raw"]["Data"].as_str().unwrap()
}

const LINK: &str = "https://github.com/nightworksio/php-mutation-gate/releases/tag/v0.1.0";

fn type_named(named: &str) -> [(&'static str, &str); 1] {
    [("x-amzn-ErrorType", named)]
}

fn announced() -> Announcement {
    let mut announcement = sample::announcement();
    announcement.parts = vec![
        Text {
            body: "Added the gate".to_owned(),
            html: Some("<p>Added the gate</p>".to_owned()),
        },
        Text {
            body: "- more".to_owned(),
            html: Some("<ul><li>more</li></ul>".to_owned()),
        },
    ];
    announcement
}

#[test]
fn a_part_is_sent_whole_by_send_email_with_its_recipients_beside_it_and_tracking_off() {
    let http = ScriptedHttp::new(vec![ok(&json!({ "MessageId": "0102-abc" }))]);
    let announcement = announced();
    assert_eq!(
        sent(&email(), &http, &Step::Post, &announcement),
        Sent::Landed(Facts {
            message: Some("0102-abc".to_owned()),
            thread: None,
            permalink: None,
        })
    );
    let request = &http.requests()[0];
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.url.redacted(),
        "https://email.eu-west-1.amazonaws.com/v2/email/outbound-emails"
    );
    let body = &http.bodies()[0];
    assert_eq!(
        body["Destination"],
        json!({
            "ToAddresses": ["team@example.org"],
            "CcAddresses": ["lead@example.org"],
            "BccAddresses": ["hidden@example.org", "audit@example.org"],
        })
    );
    assert_eq!(body["FromEmailAddress"], json!("releases@example.org"));
    assert_eq!(
        body["ConfigurationOverrides"],
        json!({ "Tracking": { "ClickTrackingEnabled": "DISABLED", "OpenTrackingEnabled": "DISABLED" } })
    );
    assert_eq!(body.get("ConfigurationSetName"), None);
    let message = expected(
        0,
        (
            &format!("Added the gate\n\n{LINK}"),
            &format!("<p>Added the gate</p>\n\n<p><a href=\"{LINK}\">{LINK}</a></p>"),
        ),
    );
    assert_eq!(data(body), base64_of(message.as_bytes()));
    for unwritten in ["Bcc", "hidden@", "audit@", "In-Reply-To"] {
        assert!(!message.contains(unwritten), "{unwritten}\n{message}");
    }
}

#[test]
fn a_later_part_replies_to_the_first_and_carries_no_link() {
    let http = ScriptedHttp::new(vec![ok(&json!({})), ok(&json!({}))]);
    let announcement = announced();
    for step in [Step::Post, Step::Chunk(1)] {
        assert_eq!(
            sent(&email(), &http, &step, &announcement),
            Sent::Landed(Facts::default())
        );
    }
    let bodies = http.bodies();
    let later = expected(1, ("- more", "<ul><li>more</li></ul>"));
    assert_eq!(data(&bodies[1]), base64_of(later.as_bytes()));
    let first = MessageId::of(
        &sample::marker(),
        0,
        mailbox("releases@example.org").address(),
    );
    assert!(
        later.contains(&format!("In-Reply-To:\r\n {first}\r\n")),
        "{later}"
    );
    assert!(!later.contains(LINK), "{later}");
}

#[test]
fn a_link_that_is_no_web_address_is_left_out_and_an_ampersand_is_written_for_html() {
    let http = ScriptedHttp::new(vec![ok(&json!({})), ok(&json!({}))]);
    let mut announcement = announced();
    announcement.link = Some("javascript:alert(1)".to_owned());
    sent(&email(), &http, &Step::Post, &announcement);
    announcement.link = Some("https://example.org/?a=1&b=2".to_owned());
    announcement.parts[0] = Text {
        body: String::new(),
        html: Some(String::new()),
    };
    sent(&email(), &http, &Step::Post, &announcement);
    let bodies = http.bodies();
    assert_eq!(
        data(&bodies[0]),
        base64_of(expected(0, ("Added the gate", "<p>Added the gate</p>")).as_bytes())
    );
    let linked = "https://example.org/?a=1&amp;b=2";
    assert_eq!(
        data(&bodies[1]),
        base64_of(
            expected(
                0,
                (
                    "https://example.org/?a=1&b=2",
                    &format!("<p><a href=\"{linked}\">{linked}</a></p>")
                )
            )
            .as_bytes()
        )
    );
}

#[test]
fn a_request_is_signed_for_the_region_s_ses_with_the_key_s_id_alone_shown() {
    let http = ScriptedHttp::new(vec![ok(&json!({})), ok(&json!({}))]);
    let announcement = announced();
    sent(&email(), &http, &Step::Post, &announcement);
    let tokened = email_through(
        ses(
            &regional("eu-west-1", false, false).unwrap(),
            true,
            Some("herald"),
        ),
        262_144,
    );
    sent(&tokened, &http, &Step::Post, &announcement);
    let requests = http.requests();
    let (plain, signed) = (&requests[0], &requests[1]);
    assert_eq!(
        header(plain, "Content-Type").as_deref(),
        Some("application/json")
    );
    assert_eq!(
        header(plain, "X-Amz-Date").as_deref(),
        Some("20261128T120000Z")
    );
    let authorization = header(plain, "Authorization").unwrap();
    assert!(
        authorization.starts_with(
            "AWS4-HMAC-SHA256 Credential=«secret:AWS_ACCESS_KEY_ID»/20261128/eu-west-1/ses/aws4_request, SignedHeaders=content-type;host;x-amz-date, Signature="
        ),
        "{authorization}"
    );
    assert_eq!(header(plain, "X-Amz-Security-Token"), None);
    let authorization = header(signed, "Authorization").unwrap();
    assert!(
        authorization.contains("SignedHeaders=content-type;host;x-amz-date;x-amz-security-token,"),
        "{authorization}"
    );
    assert_eq!(
        header(signed, "X-Amz-Security-Token").as_deref(),
        Some("«secret:AWS_SESSION_TOKEN»")
    );
    assert_eq!(http.bodies()[1]["ConfigurationSetName"], json!("herald"));
    let shown = format!("{tokened:?} {}", requests[1]);
    for secret in ["kkkk", "tttt", "id-for-tests"] {
        assert!(!shown.contains(secret), "{shown}");
    }
}

#[test]
fn each_answer_is_read_as_amazon_ses_names_it_and_no_message_of_it_is_repeated() {
    let waits = Waits {
        outage: SignedDuration::from_secs(11),
        limited: SignedDuration::from_secs(13),
    };
    let email = email().waiting(waits);
    for (answer, said) in [
        (
            refused(
                400,
                &type_named("MessageRejected:http://internal.amazon.com/coral/com.amazon.coral.validate/"),
                r#"{"message":"Email address is not verified: team@example.org"}"#,
            ),
            Sent::Refused("Amazon SES refused it with 400: MessageRejected".to_owned()),
        ),
        (
            refused(
                403,
                &[],
                r#"{"__type":"com.amazon.coral.service#UnrecognizedClientException","message":"no"}"#,
            ),
            Sent::Refused(
                "Amazon SES refused it with 403: UnrecognizedClientException; check what herald's credential is allowed to do there"
                    .to_owned(),
            ),
        ),
        (
            refused(400, &[], r#"{"code":"BadRequestException"}"#),
            Sent::Refused("Amazon SES refused it with 400: BadRequestException".to_owned()),
        ),
        (
            refused(400, &type_named("Not a name!"), r#"{"message":"team@example.org"}"#),
            Sent::Refused("Amazon SES refused it with 400".to_owned()),
        ),
        (
            refused(400, &type_named(&"A".repeat(65)), ""),
            Sent::Refused("Amazon SES refused it with 400".to_owned()),
        ),
        (
            refused(400, &type_named(&"A".repeat(64)), ""),
            Sent::Refused(format!("Amazon SES refused it with 400: {}", "A".repeat(64))),
        ),
        (
            refused(400, &type_named("ThrottlingException"), ""),
            Sent::RetryAfter(SignedDuration::from_secs(13)),
        ),
        (
            refused(429, &[("Retry-After", "7")], r#"{"__type":"TooManyRequestsException"}"#),
            Sent::RetryAfter(SignedDuration::from_secs(7)),
        ),
        (
            refused(503, &[], ""),
            Sent::RetryAfter(SignedDuration::from_secs(11)),
        ),
    ] {
        let http = ScriptedHttp::new(vec![answer]);
        assert_eq!(sent(&email, &http, &Step::Post, &announced()), said);
        assert_eq!(http.requests().len(), 1);
    }
}

#[test]
fn a_redirect_is_never_followed_with_the_signature() {
    let http = ScriptedHttp::new(vec![refused(
        307,
        &[(
            "Location",
            "https://elsewhere.example/v2/email/outbound-emails",
        )],
        "",
    )]);
    let said = sent(&email(), &http, &Step::Post, &announced());
    assert!(matches!(said, Sent::Refused(_)), "{said:?}");
    assert_eq!(http.requests().len(), 1);
}

#[test]
fn a_message_id_herald_would_not_keep_is_dropped() {
    for id in [json!("has space"), json!("x".repeat(257)), json!(7)] {
        let http = ScriptedHttp::new(vec![ok(&json!({ "MessageId": id }))]);
        assert_eq!(
            sent(&email(), &http, &Step::Post, &announced()),
            Sent::Landed(Facts::default())
        );
    }
    let http = ScriptedHttp::new(vec![ok(&json!({ "MessageId": "x".repeat(256) }))]);
    assert_eq!(
        sent(&email(), &http, &Step::Post, &announced()),
        Sent::Landed(Facts {
            message: Some("x".repeat(256)),
            thread: None,
            permalink: None,
        })
    );
}

#[test]
fn a_message_longer_than_its_bound_is_refused_before_any_request() {
    let http = ScriptedHttp::new(Vec::new());
    let said = sent(
        &email_through(regional_ses(), 600),
        &http,
        &Step::Post,
        &announced(),
    );
    let Sent::Refused(why) = said else {
        panic!("{said:?}");
    };
    assert!(why.ends_with("more than the 600 its route allows"), "{why}");
    assert_eq!(http.requests().len(), 0);
}

#[test]
fn email_takes_no_step_but_sending_a_part_and_sends_nothing_for_one() {
    let http = ScriptedHttp::new(Vec::new());
    for (step, what) in [
        (Step::Crosspost, "crosspost"),
        (
            Step::OpenThread,
            "open or name a thread: a later part replies to the first",
        ),
        (
            Step::Rename("t".to_owned()),
            "open or name a thread: a later part replies to the first",
        ),
        (Step::Pin, "pin"),
        (Step::Unpin, "pin"),
        (
            Step::Edit {
                revision: "r".to_owned(),
                part: 0,
            },
            "edit or delete a message once it is sent",
        ),
        (Step::Delete(0), "edit or delete a message once it is sent"),
        (Step::Deliveries, "deliver to an inbox"),
        (Step::Deliver("i".to_owned()), "deliver to an inbox"),
        (Step::Updates("i".to_owned()), "deliver to an inbox"),
        (Step::Deletes, "deliver to an inbox"),
    ] {
        assert_eq!(
            sent(&email(), &http, &step, &announced()),
            Sent::Refused(format!("Email cannot {what}")),
            "{step:?}"
        );
    }
    assert_eq!(
        pollster::block_on(email().housekeep(&http, Chore::Archive, "t")).unwrap(),
        Sent::Refused("Email cannot archive or lock a thread".to_owned())
    );
    assert_eq!(http.requests().len(), 0);
}

#[test]
fn it_reaches_its_endpoint_alone_and_reads_nothing_back() {
    assert_eq!(
        email().base(),
        Base::At("https://email.eu-west-1.amazonaws.com")
    );
    let given = email_through(ses("https://vpce.example/ses", false, None), 262_144);
    assert_eq!(Reach::base(&given), Base::At("https://vpce.example/ses"));
    let http = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        pollster::block_on(email().audience(&http)),
        Audience::Unread(_)
    ));
    assert!(email().keeps_edits());
    let reached = email();
    let posts = reached.posts(&http);
    assert!(pollster::block_on(posts.entry_boxed(&announced().key, &sample::marker())).is_err());
    let typed = refused(400, &[("x-amzn-ErrorType", "MessageRejected")], "");
    assert!(email().declines(&typed));
    assert!(!email().declines(&refused(403, &[], "<html>blocked</html>")));
    assert!(!email().declines(&refused(400, &type_named(""), "")));
    assert!(!email().declines(&refused(400, &type_named(":http://x"), "")));
    assert!(!email().declines(&refused(403, &[], r#"{"detail":"blocked"}"#)));
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
fn the_doctor_asks_for_the_account_signed_and_sends_nothing() {
    let (probes, requests) = probed(vec![ok(&json!({
        "SendingEnabled": true,
        "ProductionAccessEnabled": true,
    }))]);
    assert_eq!(probes, [Probe::Taken]);
    let request = &requests[0];
    assert_eq!(request.method, Method::Get);
    assert_eq!(
        request.url.redacted(),
        "https://email.eu-west-1.amazonaws.com/v2/email/account"
    );
    assert_eq!(request.body.len(), 0);
    assert_eq!(header(request, "Content-Type"), None);
    let authorization = header(request, "Authorization").unwrap();
    assert!(
        authorization.contains("/eu-west-1/ses/aws4_request, SignedHeaders=host;x-amz-date, "),
        "{authorization}"
    );
}

#[test]
fn the_doctor_says_where_the_account_sends_to_verified_addresses_alone_or_not_at_all() {
    let (probes, _) = probed(vec![ok(&json!({
        "SendingEnabled": false,
        "ProductionAccessEnabled": false,
    }))]);
    assert_eq!(
        probes,
        [
            Probe::Taken,
            Probe::Route {
                key: None,
                why: "Amazon SES has sending paused for the account in eu-west-1".to_owned(),
            },
            Probe::Said(
                "the account is in Amazon SES's sandbox in eu-west-1: it sends only to the addresses and domains it verified"
                    .to_owned()
            ),
        ]
    );
    let (unsaid, _) = probed(vec![ok(&json!({}))]);
    assert_eq!(unsaid, [Probe::Taken]);
}

#[test]
fn the_doctor_names_a_refused_credential_by_its_error_alone() {
    let (denied, _) = probed(vec![refused(
        403,
        &[("x-amzn-ErrorType", "AccessDeniedException")],
        r#"{"message":"User arn:aws:iam::1:user/x is not authorized"}"#,
    )]);
    assert_eq!(
        denied,
        [Probe::Said(
            "Amazon SES takes its credential, which may not read the account (ses:GetAccount), so whether it may send in eu-west-1 is not asked"
                .to_owned()
        )]
    );
    let (unknown, _) = probed(vec![refused(
        403,
        &[("x-amzn-ErrorType", "UnrecognizedClientException")],
        r#"{"message":"The security token included in the request is invalid."}"#,
    )]);
    assert_eq!(
        unknown,
        [Probe::Credential(
            "Amazon SES refuses its credential, answering 403: UnrecognizedClientException"
                .to_owned()
        )]
    );
    let (unnamed, _) = probed(vec![refused(500, &[], "")]);
    assert_eq!(
        unnamed,
        [Probe::Credential(
            "Amazon SES refuses its credential, answering 500".to_owned()
        )]
    );
    let (unasked, _) = probed(Vec::new());
    assert_eq!(
        unasked,
        [Probe::Credential(format!(
            "Amazon SES could not be asked: {}",
            HttpError::TimedOut
        ))]
    );
}

#[test]
fn its_debug_form_names_its_secrets_alone() {
    shows_no_secret(&email());
    shows_no_secret(&credentials(true));
}

fn shows_no_secret(shown: &impl Debug) {
    let shown = format!("{shown:?}");
    for secret in ["kkkk", "tttt", "id-for-tests"] {
        assert!(!shown.contains(secret), "{shown}");
    }
}
