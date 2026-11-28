//! A Workflows webhook against its fixture: each card's request as the
//! fixture holds it, and each answer read back into what the ledger
//! records.

use herald_core::{Announcement, Button, Facts, Sent, Step, Text, WaitOptions, Waits};
use herald_dest_teams::{MESSAGE, Teams};
use std::future::{Future, ready};

use herald_ports::{Base, Http, HttpError, Request, Response, Secret};
use herald_testing::{Fixture, Recorder, Replay, Scrub, sample};
use jiff::SignedDuration;

const WEBHOOK: &str = "HERALD_TEAMS_WEBHOOK";

fn url() -> String {
    [
        "https://",
        "default0.12.environment.api.example",
        "/workflows/1/invoke?sig=",
        "s1",
    ]
    .concat()
}

fn teams() -> Teams {
    Teams::new(Secret::new(WEBHOOK, url()))
}

fn send(teams: &Teams, replay: &Replay, step: &Step, announcement: &Announcement) -> Sent {
    pollster::block_on(teams.send(replay, step, announcement)).unwrap()
}

fn refused(said: &str) -> Sent {
    Sent::Refused(said.to_owned())
}

fn replayed() -> Replay {
    Replay::of(Fixture::from_json(include_str!("../../fixtures/webhook.reference.json")).unwrap())
}

#[test]
fn a_webhook_posts_its_card_then_each_overflow_part_as_a_card_of_its_own() {
    let replay = replayed();
    let announcement = sample::announcement();
    let landed = Sent::Landed(Facts::default());
    assert_eq!(send(&teams(), &replay, &Step::Post, &announcement), landed);
    assert_eq!(
        send(&teams(), &replay, &Step::Chunk(1), &announcement),
        landed
    );
    for said in [
        refused(
            "Microsoft Teams refused it with 400: TriggerInputSchemaMismatch: The input body for trigger 'manual' of type 'Request' did not match its schema definition.",
        ),
        refused(
            "Microsoft Teams refused it with 404: workflow not found; check the names and ids the config gives",
        ),
        Sent::RetryAfter(SignedDuration::from_secs(7)),
        refused("Microsoft Teams refused it with 413"),
    ] {
        assert_eq!(send(&teams(), &replay, &Step::Post, &announcement), said);
    }
    replay.finish().unwrap();
}

#[test]
fn a_webhook_takes_no_step_but_posting_and_sends_nothing_for_one() {
    let replay = Replay::of(Fixture::default());
    let announcement = sample::announcement();
    for (step, what) in [
        (Step::Crosspost, "crosspost"),
        (Step::OpenThread, "open a thread"),
        (Step::Rename("t".to_owned()), "open a thread"),
        (Step::Pin, "pin"),
        (Step::Unpin, "pin"),
        (
            Step::Edit {
                revision: "r".to_owned(),
                part: 0,
            },
            "edit a message",
        ),
        (Step::Delete(0), "delete a message"),
        (Step::Deliveries, "deliver to an inbox"),
        (Step::Deliver("i".to_owned()), "deliver to an inbox"),
        (Step::Updates("i".to_owned()), "deliver to an inbox"),
        (Step::Deletes, "deliver to an inbox"),
    ] {
        assert_eq!(
            send(&teams(), &replay, &step, &announcement),
            refused(&format!(
                "A Microsoft Teams Workflows webhook cannot {what}"
            )),
            "{step:?}"
        );
    }
    replay.finish().unwrap();
}

#[test]
fn it_reaches_its_url_s_own_https_origin_alone() {
    assert_eq!(
        teams().base(),
        Base::At("https://default0.12.environment.api.example")
    );
    assert!(teams().addressed());
    let ported = Teams::new(Secret::new(WEBHOOK, "https://hook.example:8443/x"));
    assert_eq!(ported.base(), Base::At("https://hook.example:8443"));
    let shown = format!("{:?}", teams());
    assert!(
        shown.contains("«secret:HERALD_TEAMS_WEBHOOK»") && !shown.contains("example"),
        "{shown}"
    );
}

#[test]
fn a_secret_holding_no_https_url_reaches_nothing_and_its_post_names_the_secret_alone() {
    let announcement = sample::announcement();
    for held in [
        "",
        "not a url",
        "http://hook.example/x?sig=s2",
        "mailto:a@example.org",
    ] {
        let teams = Teams::new(Secret::new(WEBHOOK, held));
        assert_eq!(teams.base(), Base::Among(&[]), "{held}");
        assert!(!teams.addressed(), "{held}");
        let recorder = Recorder::new(Answering);
        pollster::block_on(teams.send(&recorder, &Step::Post, &announcement)).unwrap();
        let fixture = recorder.fixture(&Scrub::default()).unwrap();
        assert_eq!(
            fixture.exchanges[0].request.url,
            "«secret:HERALD_TEAMS_WEBHOOK»"
        );
    }
}

#[test]
fn a_card_past_what_a_message_holds_is_refused_before_it_is_sent() {
    let replay = Replay::of(Fixture::default());
    let mut announcement = sample::announcement();
    let far = format!("https://example.org/{}", "a".repeat(MESSAGE));
    announcement.buttons = vec![Button {
        name: "release".to_owned(),
        label: None,
        url: far,
    }];
    let Sent::Refused(said) = send(&teams(), &replay, &Step::Post, &announcement) else {
        panic!("sent");
    };
    assert!(
        said.starts_with("the card is ")
            && said.ends_with(" bytes, more than the 28000 Microsoft Teams takes in a message"),
        "{said}"
    );
    replay.finish().unwrap();
}

#[test]
fn a_card_with_no_button_shows_its_link_and_an_empty_part_writes_no_text() {
    let mut announcement = sample::announcement();
    announcement.buttons = Vec::new();
    announcement.parts = vec![Text {
        body: String::new(),
        html: None,
    }];
    let recorder = Recorder::new(Answering);
    assert_eq!(
        pollster::block_on(teams().send(&recorder, &Step::Post, &announcement)).unwrap(),
        Sent::Landed(Facts::default())
    );
    let fixture = recorder.fixture(&Scrub::default()).unwrap();
    let card = &fixture.exchanges[0].request.body.as_ref().unwrap()["attachments"][0]["content"];
    assert_eq!(card.get("actions"), None);
    let link = "https://github.com/nightworksio/php-mutation-gate/releases/tag/v0.1.0";
    assert_eq!(
        card["body"],
        serde_json::json!([
            { "type": "TextBlock", "text": "📦 php\\-mutation\\-gate v0\\.1\\.0", "size": "Large", "weight": "Bolder", "wrap": true },
            { "type": "TextBlock", "text": format!("[{link}]({link})"), "wrap": true },
        ])
    );
}

struct Answering;

impl Http for Answering {
    fn send(&self, _: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        ready(Ok(answer(202, b"")))
    }
}

/// An `Http` answering every request with its response.
struct Answered(Response);

impl Http for Answered {
    fn send(&self, _: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        ready(Ok(self.0.clone()))
    }
}

fn answer(status: u16, body: &[u8]) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_vec(),
    }
}

fn answered(teams: &Teams, response: Response) -> Sent {
    pollster::block_on(teams.send(&Answered(response), &Step::Post, &sample::announcement()))
        .unwrap()
}

#[test]
fn a_refusal_says_what_teams_said_however_it_says_it_and_cut_short() {
    let long = "x".repeat(300);
    for (body, said) in [
        (
            br#"{"error":{"code":"OnlyCode"}}"#.to_vec(),
            "OnlyCode".to_owned(),
        ),
        (
            br#"{"error":{"message":"Only a message"}}"#.to_vec(),
            "Only a message".to_owned(),
        ),
        (br#"{"other":1}"#.to_vec(), r#"{"other":1}"#.to_owned()),
        (long.as_bytes().to_vec(), "x".repeat(200)),
    ] {
        assert_eq!(
            answered(&teams(), answer(400, &body)),
            refused(&format!("Microsoft Teams refused it with 400: {said}"))
        );
    }
}

#[test]
fn the_waits_the_config_sets_are_waited_where_teams_names_none() {
    let set: WaitOptions = serde_json::from_value(serde_json::json!({ "limited": "9s" })).unwrap();
    let patient = teams().waiting(Waits::of(Some(set), Waits::DEFAULT));
    assert_eq!(
        answered(&patient, answer(429, b"")),
        Sent::RetryAfter(SignedDuration::from_secs(9))
    );
    assert_eq!(
        answered(&teams(), answer(429, b"")),
        Sent::RetryAfter(SignedDuration::from_secs(5))
    );
}

#[test]
fn a_card_of_exactly_what_a_message_holds_is_sent_and_one_byte_more_is_not() {
    let sized = |extra: usize| {
        let mut announcement = sample::announcement();
        announcement.buttons = vec![Button {
            name: "release".to_owned(),
            label: None,
            url: format!("https://example.org/{}", "a".repeat(extra)),
        }];
        let recorder = Recorder::new(Answering);
        let sent = pollster::block_on(teams().send(&recorder, &Step::Post, &announcement)).unwrap();
        let length = recorder
            .fixture(&Scrub::default())
            .unwrap()
            .exchanges
            .first()
            .and_then(|exchange| exchange.request.body.as_ref())
            .map(|body| body.to_string().len());
        (sent, length)
    };
    let (_, least) = sized(0);
    let fill = MESSAGE - least.unwrap();
    assert_eq!(sized(fill), (Sent::Landed(Facts::default()), Some(MESSAGE)));
    let (over, none) = sized(fill + 1);
    assert_eq!(none, None);
    assert_eq!(
        over,
        refused("the card is 28001 bytes, more than the 28000 Microsoft Teams takes in a message")
    );
}
