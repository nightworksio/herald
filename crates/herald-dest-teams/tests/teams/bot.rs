//! A Teams bot against its fixtures: its sign-in, each step's request as
//! the fixture holds it, each answer read back into what the ledger
//! records, and its table and routes as the config reads them.

use herald_core::{Announcement, Delivery, Entry, Facts, Features, Sent, Step, Threads};
use herald_dest_teams::{
    Bot, Cloud, DestinationOptions, RouteOptions, Teams, TeamsKind, bot_profile,
};
use herald_kinds::{DestinationConfig, DestinationKind, Fault, Placed, Placing, Probe, Recorded};
use herald_ports::{Base, Secret};
use herald_testing::{Fixture, Replay, sample};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

const PASSWORD: &str = "HERALD_TEAMS_APP_PASSWORD";

fn table(extra: &Value) -> Value {
    let mut table = json!({
        "app_id": "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
        "app_password": { "secret": PASSWORD },
        "tenant": "contoso.onmicrosoft.com",
        "login": "https://login.example",
        "service": "https://smba.example/teams/",
    });
    if let (Some(table), Some(extra)) = (table.as_object_mut(), extra.as_object()) {
        table.extend(extra.clone());
    }
    table
}

fn one(key: &str, value: &str) -> Value {
    Value::Object(serde_json::Map::from_iter([(key.to_owned(), json!(value))]))
}

fn options(extra: &Value) -> DestinationOptions {
    serde_json::from_value(table(extra)).unwrap()
}

fn route(threads: Option<bool>) -> RouteOptions {
    RouteOptions {
        conversation: Some("19:abc@thread.tacv2".to_owned()),
        threads,
        ..RouteOptions::default()
    }
}

fn password() -> Secret {
    Secret::new(PASSWORD, ["app-", "password-", "1"].concat())
}

fn reached(threads: Option<bool>) -> herald_kinds::Reached {
    options(&json!({}))
        .reach(&Placed::new(route(threads)), "", password())
        .unwrap()
}

fn teams(threads: Option<bool>) -> Teams {
    reached(threads).of::<Teams>().unwrap().clone()
}

fn now() -> Timestamp {
    Timestamp::UNIX_EPOCH
}

fn send(teams: &Teams, replay: &Replay, step: &Step, entry: &Entry, at: Timestamp) -> Sent {
    let announcement = sample::announcement();
    pollster::block_on(teams.send(replay, (step, &announcement, entry), at)).unwrap()
}

fn landed(id: &str) -> Sent {
    Sent::Landed(Facts {
        message: Some(id.to_owned()),
        ..Facts::default()
    })
}

fn fixture(json: &str) -> Replay {
    Replay::of(Fixture::from_json(json).unwrap())
}

fn posted(announcement: &Announcement) -> Entry {
    Entry::new(announcement.key)
        .landed(
            Step::Post,
            Facts {
                message: Some("1700000000001".to_owned()),
                ..Facts::default()
            },
        )
        .landed(
            Step::Chunk(1),
            Facts {
                message: Some("1700000000002".to_owned()),
                ..Facts::default()
            },
        )
}

#[test]
fn a_bot_signs_in_once_then_posts_replies_in_the_thread_edits_and_deletes() {
    let replay = fixture(include_str!("../../fixtures/bot.reference.json"));
    let teams = teams(Some(true));
    let announcement = sample::announcement();
    let first = Entry::new(announcement.key);
    assert_eq!(
        send(&teams, &replay, &Step::Post, &first, now()),
        landed("1700000000001")
    );
    let entry = posted(&announcement);
    assert_eq!(
        send(&teams, &replay, &Step::Chunk(1), &entry, now()),
        landed("1700000000002")
    );
    let edit = Step::Edit {
        revision: "r".to_owned(),
        part: 0,
    };
    assert_eq!(
        send(&teams, &replay, &edit, &entry, now()),
        landed("1700000000001")
    );
    assert_eq!(
        send(&teams, &replay, &Step::Delete(1), &entry, now()),
        Sent::Landed(Facts::default())
    );
    for said in [
        Sent::RetryAfter(SignedDuration::from_secs(3)),
        Sent::RetryAfter(SignedDuration::from_secs(5)),
        Sent::RetryAfter(SignedDuration::from_secs(5)),
        Sent::Refused(
            "Microsoft Teams refused it with 413: MessageSizeTooBig: Message size too big"
                .to_owned(),
        ),
        Sent::Refused("Microsoft Teams refused it with 403: BotNotInConversationRoster: The bot is not part of the conversation roster.; check what herald's credential is allowed to do there".to_owned()),
    ] {
        assert_eq!(send(&teams, &replay, &Step::Post, &first, now()), said);
    }
    replay.finish().unwrap();
}

#[test]
fn a_bot_signs_in_again_once_its_token_nears_its_end() {
    let replay = fixture(include_str!("../../fixtures/bot-renewed.reference.json"));
    let teams = teams(None);
    let first = Entry::new(sample::announcement().key);
    let after = |seconds| {
        now()
            .checked_add(SignedDuration::from_secs(seconds))
            .unwrap()
    };
    for at in [now(), after(3299), after(3300)] {
        assert_eq!(
            send(&teams, &replay, &Step::Post, &first, at),
            landed("1700000000001")
        );
    }
    let longer = self::teams(None);
    for at in [now(), after(3299), after(3300)] {
        assert_eq!(
            send(&longer, &replay, &Step::Post, &first, at),
            landed("1700000000001")
        );
    }
    replay.finish().unwrap();
}

#[test]
fn a_refused_sign_in_posts_nothing_and_names_microsoft_entra_s_code_alone() {
    let replay = fixture(include_str!("../../fixtures/bot-signed-out.reference.json"));
    let teams = teams(None);
    let first = Entry::new(sample::announcement().key);
    let refused = send(&teams, &replay, &Step::Post, &first, now());
    assert_eq!(
        refused,
        Sent::Refused("Microsoft's sign-in service refused it with 401: invalid_client (AADSTS7000215); check the credential herald sends, and that it has not expired".to_owned())
    );
    assert!(!format!("{refused:?}").contains("Invalid client secret"));
    assert_eq!(
        send(&teams, &replay, &Step::Post, &first, now()),
        Sent::Refused(
            "Microsoft's sign-in service answered with no token herald can send".to_owned()
        )
    );
    assert_eq!(
        send(&teams, &replay, &Step::Post, &first, now()),
        Sent::RetryAfter(SignedDuration::from_secs(5))
    );
    replay.finish().unwrap();
}

#[test]
fn a_bot_takes_no_step_it_has_no_way_to_and_sends_nothing_for_one() {
    let replay = Replay::of(Fixture::default());
    let first = Entry::new(sample::announcement().key);
    for (step, said) in [
        (Step::Crosspost, "Microsoft Teams cannot crosspost"),
        (
            Step::OpenThread,
            "Microsoft Teams cannot open a thread; its replies are the thread",
        ),
        (
            Step::Rename("t".to_owned()),
            "Microsoft Teams cannot open a thread; its replies are the thread",
        ),
        (Step::Pin, "Microsoft Teams cannot pin"),
        (Step::Unpin, "Microsoft Teams cannot pin"),
        (
            Step::Deliveries,
            "Microsoft Teams cannot deliver to an inbox",
        ),
        (
            Step::Deliver("i".to_owned()),
            "Microsoft Teams cannot deliver to an inbox",
        ),
        (
            Step::Updates("i".to_owned()),
            "Microsoft Teams cannot deliver to an inbox",
        ),
        (Step::Deletes, "Microsoft Teams cannot deliver to an inbox"),
        (
            Step::Chunk(1),
            "the announcement's message is not recorded, so this step cannot be taken yet",
        ),
        (
            Step::Delete(1),
            "the message is not recorded, so this step cannot be taken yet",
        ),
    ] {
        assert_eq!(
            send(&teams(Some(true)), &replay, &step, &first, now()),
            Sent::Refused(said.to_owned()),
            "{step:?}"
        );
    }
    replay.finish().unwrap();
}

#[test]
fn a_bot_reaches_its_sign_in_and_service_hosts_alone_and_names_its_preview() {
    let reached = reached(Some(true));
    assert_eq!(
        reached.base(),
        Base::Among(&[
            "https://login.example".to_owned(),
            "https://smba.example".to_owned()
        ])
    );
    assert!(reached.keeps_edits());
    assert_eq!(reached.profile(), bot_profile(true));
    assert_eq!(
        bot_profile(true).features,
        Features {
            threads: Threads::Replies,
            edits: true,
            ..bot_profile(false).features
        }
    );
    assert_eq!(bot_profile(false).features.threads, Threads::None);
    assert_eq!(reached_unthreaded().profile(), bot_profile(false));
    let envelope = herald_ports::Response {
        status: 403,
        headers: Vec::new(),
        body: br#"{"error":{"code":"BotNotInConversationRoster","message":"m"}}"#.to_vec(),
    };
    assert!(reached.declines(&envelope));
    let replay = fixture(include_str!("../../fixtures/bot.reference.json"));
    let probes = pollster::block_on(reached.probe(&replay, &Delivery::default(), Recorded::Kept));
    assert_eq!(probes[0], Probe::Taken);
    assert!(matches!(&probes[1], Probe::Said(said) if said.contains("replyChains")));
    let unthreaded = fixture(include_str!("../../fixtures/bot.reference.json"));
    assert_eq!(
        pollster::block_on(reached_unthreaded().probe(
            &unthreaded,
            &Delivery::default(),
            Recorded::Kept
        )),
        [Probe::Taken]
    );
    let refused = fixture(include_str!("../../fixtures/bot-signed-out.reference.json"));
    let probes = pollster::block_on(reached.probe(&refused, &Delivery::default(), Recorded::Kept));
    assert!(matches!(&probes[0], Probe::Credential(why) if why.contains("AADSTS7000215")));
    let unthreaded = reached_unthreaded();
    let probe =
        || pollster::block_on(unthreaded.probe(&refused, &Delivery::default(), Recorded::Kept));
    assert_eq!(
        probe(),
        [Probe::Credential(
            "Microsoft's sign-in service answered with no token herald can send".to_owned()
        )]
    );
    assert_eq!(
        probe(),
        [Probe::Credential(
            "Microsoft's sign-in service refused it with 503".to_owned()
        )]
    );
    refused.finish().unwrap();
    let unreached = pollster::block_on(unthreaded.probe(
        &Replay::of(Fixture::default()),
        &Delivery::default(),
        Recorded::Kept,
    ));
    assert!(
        matches!(&unreached[..], [Probe::Credential(why)] if why.starts_with("Microsoft's sign-in service")),
        "{unreached:?}"
    );
}

fn reached_unthreaded() -> herald_kinds::Reached {
    reached(None)
}

#[test]
fn a_bot_s_table_names_its_cloud_s_hosts_where_it_names_none() {
    let public = Teams::bot(Bot::new(
        "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
        password(),
        Cloud::Public.hosts(),
        &route(None),
    ));
    let written = serde_json::from_value::<DestinationOptions>(json!({
        "app_id": "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
        "app_password": { "secret": PASSWORD },
    }))
    .unwrap()
    .reach(&Placed::new(route(None)), "", password())
    .unwrap();
    assert_eq!(written.base(), public.base());
    let high = serde_json::from_value::<DestinationOptions>(json!({
        "app_id": "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
        "app_password": { "secret": PASSWORD },
        "cloud": "gcc-high",
    }))
    .unwrap()
    .reach(&Placed::new(route(None)), "", password())
    .unwrap();
    assert_eq!(
        high.base(),
        Base::Among(&[
            "https://login.microsoftonline.us".to_owned(),
            "https://smba.infra.gov.teams.microsoft.us".to_owned()
        ])
    );
}

#[test]
fn a_bot_s_table_takes_a_bot_or_a_webhook_whole_and_each_key_as_written() {
    let options = options(&json!({}));
    assert_eq!(options.bot(), Ok(true));
    assert_eq!(
        options.credential().map(|secret| secret.secret.as_str()),
        Some(PASSWORD)
    );
    assert_eq!(options.secret_refs().len(), 1);
    assert_eq!(
        options.profile(&Placed::new(route(Some(true)))),
        Some((bot_profile(true), Some(bot_profile(true))))
    );
    let bot = |table: Value| {
        serde_json::from_value::<DestinationOptions>(table)
            .unwrap()
            .bot()
    };
    for (table, said) in [
        (
            json!({}),
            "a Microsoft Teams destination needs a webhook, or a bot's app_id and app_password; give one",
        ),
        (
            json!({ "app_id": "a" }),
            "a bot needs app_password, the secret holding its client secret; give it",
        ),
        (
            json!({ "app_password": { "secret": "P" } }),
            "a bot needs app_id, its app registration's application id; give it",
        ),
        (
            json!({ "webhook": { "secret": "W" }, "app_id": "a" }),
            "a Microsoft Teams destination takes a webhook or a bot, not both; keep one",
        ),
        (
            json!({ "webhook": { "secret": "W" }, "cloud": "dod" }),
            "a Workflows webhook signs in nowhere, so cloud is a bot's alone; drop it",
        ),
        (
            json!({ "webhook": { "secret": "W" }, "service": "https://s.example" }),
            "a Workflows webhook signs in nowhere, so service is a bot's alone; drop it",
        ),
    ] {
        assert_eq!(bot(table.clone()), Err(said.to_owned()), "{table}");
    }
    assert_eq!(
        bot(json!({ "webhook": { "secret": "W" }, "interactions": {} })),
        Err(
            "a Workflows webhook signs in nowhere, so interactions is a bot's alone; drop it"
                .to_owned()
        )
    );
    for key in ["tenant", "login", "scope", "openid", "issuer"] {
        assert_eq!(
            bot(json!({ "webhook": { "secret": "W" } })
                .as_object()
                .map_or_else(Value::default, |table| {
                    let mut table = table.clone();
                    table.extend(one(key, "x").as_object().cloned().unwrap_or_default());
                    Value::Object(table)
                })),
            Err(format!(
                "a Workflows webhook signs in nowhere, so {key} is a bot's alone; drop it"
            ))
        );
    }
}

fn checked(extra: &Value) -> Vec<Fault> {
    TeamsKind::checked(options(extra), &|_| Err(String::new()))
        .unwrap()
        .1
}

#[test]
fn a_bot_s_keys_not_written_as_microsoft_writes_them_are_named() {
    assert_eq!(checked(&json!({})), []);
    for (key, written, said) in [
        (
            "app_id",
            "app",
            "it is not an application id; give the app registration's application (client) id, such as \"00000000-0000-0000-0000-000000000000\"",
        ),
        (
            "tenant",
            "a/b",
            "it is not a tenant; give the tenant's id or its domain, such as \"contoso.onmicrosoft.com\"",
        ),
        (
            "login",
            "http://login.example",
            "it is not an https URL of a host alone; give one, such as \"https://login.microsoftonline.com\"",
        ),
        (
            "scope",
            "api.botframework.com",
            "it is not an https URL of a host alone; give one, such as \"https://api.botframework.com\"",
        ),
        (
            "service",
            "https://s.example/?q",
            "it is not an https URL of a host alone; give one, such as \"https://smba.trafficmanager.net/teams/\"",
        ),
    ] {
        assert_eq!(checked(&one(key, written)), [Fault::at(key, said)], "{key}");
    }
    assert!(
        TeamsKind::checked(serde_json::from_value(json!({})).unwrap(), &|_| Err(
            String::new()
        ))
        .is_err()
    );
}

#[test]
fn a_bot_s_commands_are_named_once_each_and_worded_as_herald_reads_wording() {
    assert_eq!(checked(&json!({ "interactions": {} })), []);
    assert_eq!(
        checked(&json!({ "interactions": { "commands": [] } })),
        [Fault::at(
            "interactions",
            "it answers no command; name at least one, or leave out the interactions"
        )]
    );
    assert_eq!(
        checked(&json!({ "interactions": { "commands": ["latest", "latest"] } })),
        [Fault::at(
            "interactions",
            "it names latest twice; name it once"
        )]
    );
    let unworded =
        checked(&json!({ "interactions": { "wording": { "subscribed": "{{ project" } } }));
    assert_eq!(unworded.len(), 1);
    assert_eq!(
        unworded.first().map(|fault| fault.key.as_deref()),
        Some(Some("interactions"))
    );
    assert_eq!(
        checked(&json!({ "interactions": { "commands": [] }, "login": "http://login.example" }))
            .len(),
        2
    );
}

fn placed(options: &DestinationOptions, route: &RouteOptions) -> Vec<Fault> {
    TeamsKind::placed(
        options,
        route,
        Placing {
            destination: "teams",
            subscribers: false,
        },
    )
}

#[test]
fn a_bot_s_route_names_its_conversation_and_a_webhook_s_names_none() {
    let bot = options(&json!({}));
    assert_eq!(placed(&bot, &route(Some(true))), []);
    assert_eq!(
        placed(&bot, &RouteOptions::default()),
        [Fault::here(
            "a Microsoft Teams bot posts to a channel or chat; give conversation = \"<its id>\""
        )]
    );
    let unwritten = "it is not a conversation's id; give the channel's or chat's id, such as \"19:…@thread.tacv2\"";
    for conversation in ["", "19:a b", "19:a\n"] {
        let route = RouteOptions {
            conversation: Some(conversation.to_owned()),
            ..RouteOptions::default()
        };
        assert_eq!(
            placed(&bot, &route),
            [Fault::at("conversation", unwritten)],
            "{conversation:?}"
        );
        assert!(
            bot.recalled(&serde_json::to_string(&route).unwrap())
                .is_none()
        );
    }
    let webhook: DestinationOptions =
        serde_json::from_value(json!({ "webhook": { "secret": "W" } })).unwrap();
    assert_eq!(placed(&webhook, &RouteOptions::default()), []);
    let alone = "a Workflows webhook posts where its workflow names, alone; drop it";
    assert_eq!(
        placed(&webhook, &route(Some(false))),
        [
            Fault::at("conversation", alone),
            Fault::at("threads", alone)
        ]
    );
    let written = serde_json::to_string(&route(Some(true))).unwrap();
    assert_eq!(
        written,
        r#"{"conversation":"19:abc@thread.tacv2","threads":true}"#
    );
    assert!(bot.recalled(&written).is_some());
}

/// An `Http` that keeps each request's method and URL and answers each
/// with a token and a message id.
#[derive(Default)]
struct Kept(std::sync::Mutex<Vec<String>>);

impl herald_ports::Http for Kept {
    fn send(
        &self,
        request: herald_ports::Request,
    ) -> impl std::future::Future<Output = Result<herald_ports::Response, herald_ports::HttpError>> + Send
    {
        self.0.lock().unwrap().push(format!(
            "{} {}",
            request.method.as_str(),
            request.url.redacted()
        ));
        std::future::ready(Ok(herald_ports::Response {
            status: 200,
            headers: Vec::new(),
            body: br#"{"id":"9","access_token":"t","expires_in":3600}"#.to_vec(),
        }))
    }
}

#[test]
fn an_unthreaded_bot_posts_each_part_on_its_own_and_edits_a_part_in_place() {
    let kept = Kept::default();
    let teams = teams(None);
    let announcement = sample::announcement();
    let entry = posted(&announcement);
    let edit = Step::Edit {
        revision: "r".to_owned(),
        part: 1,
    };
    for step in [Step::Chunk(1), edit] {
        assert_eq!(
            pollster::block_on(teams.send(&kept, (&step, &announcement, &entry), now())).unwrap(),
            landed("9")
        );
    }
    let conversation = "https://smba.example/teams/v3/conversations/19%3Aabc%40thread.tacv2";
    assert_eq!(
        *kept.0.lock().unwrap(),
        [
            "POST https://login.example/contoso.onmicrosoft.com/oauth2/v2.0/token".to_owned(),
            format!("POST {conversation}/activities"),
            format!("PUT {conversation}/activities/1700000000002"),
        ]
    );
}

#[test]
fn a_bot_s_route_is_placed_in_its_conversation_and_shown_without_its_secret() {
    assert_eq!(
        herald_kinds::Placement::place(&route(None)),
        Some("19:abc@thread.tacv2")
    );
    let shown = format!("{:?}", teams(None));
    assert!(
        shown.starts_with("Teams { bot: Bot {")
            && shown.contains("«secret:HERALD_TEAMS_APP_PASSWORD»")
            && !shown.contains("app-password"),
        "{shown}"
    );
}

#[test]
fn a_bot_whose_tenant_names_nothing_asks_nobody_and_sends_nothing() {
    let unnamed = Bot::new(
        "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
        password(),
        herald_dest_teams::Hosts {
            tenant: ".",
            ..Cloud::Public.hosts()
        },
        &route(None),
    );
    let said = "the bot's tenant is empty or a dot segment, which names nothing in a URL path, so this step is not taken";
    let replay = Replay::of(Fixture::default());
    let first = Entry::new(sample::announcement().key);
    assert_eq!(
        send(
            &Teams::bot(unnamed.clone()),
            &replay,
            &Step::Post,
            &first,
            now()
        ),
        Sent::Refused(said.to_owned())
    );
    let probes = pollster::block_on(herald_kinds::Reached::new(Teams::bot(unnamed)).probe(
        &replay,
        &Delivery::default(),
        Recorded::Kept,
    ));
    assert_eq!(probes, [Probe::Credential(said.to_owned())]);
    replay.finish().unwrap();
}
