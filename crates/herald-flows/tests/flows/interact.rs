use std::collections::{BTreeMap, BTreeSet};

use herald_config::{Config, Uses};
use herald_core::{
    Asked, Command, DestinationName, Kind, Latest, Member, Place, RouteId, Shown, Subscription,
};
use herald_flows::{
    Answer, Answered, Connected, Inbound, Job, Keys, Ports, Replying, Serving, Webhooks, Worked,
    answer, completed, deferral, interact, prepare, replied, work,
};
use herald_ports::{Announcements, Secret, Subscriptions};
use herald_records::MemoryKeyring;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeVerifier};

use crate::harness::{Failing, Fault, Script, healthy};
use crate::inbound::at;

pub(crate) const APPLICATION: &str = "123456789012345678";
pub(crate) const GUILD: &str = "223456789012345678";
pub(crate) const CHANNEL: &str = "323456789012345678";
const OTHER_CHANNEL: &str = "333456789012345678";
pub(crate) const USER: &str = "423456789012345678";
const ROLE: &str = "523456789012345678";
pub(crate) const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
pub(crate) const SLACK_CHANNEL: &str = "C0123456789";
pub(crate) const ROOM: &str = "!room:matrix.example";
pub(crate) const HOOK: &str = "https://hooks.slack.com/commands/T1/2/abc";
pub(crate) const TOKEN: &str = "«secret:the interaction's token»";

fn config(discord: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[destinations.discord.interactions]
application = "{APPLICATION}"
key = "{KEY}"
{discord}
[destinations.quiet]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[destinations.gatewayed]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[destinations.gatewayed.interactions]
application = "{APPLICATION}"
via = "gateway"
[destinations.slack]
kind = "slack"
token = {{ secret = "HERALD_SLACK_TOKEN" }}
[destinations.slack.interactions]
signing = {{ secret = "HERALD_SLACK_SIGNING" }}
[destinations.socket]
kind = "slack"
token = {{ secret = "HERALD_SLACK_TOKEN" }}
[destinations.socket.interactions]
via = "socket"
app = {{ secret = "HERALD_SLACK_APP" }}
[destinations.matrix]
kind = "matrix"
homeserver = "https://matrix.example"
token = {{ secret = "HERALD_MATRIX_TOKEN" }}
[destinations.matrix.interactions]
homeserver_token = {{ secret = "HERALD_MATRIX_HS" }}
appservice_token = {{ secret = "HERALD_MATRIX_AS" }}
[destinations.neighbour]
kind = "matrix"
homeserver = "https://matrix.example"
token = {{ secret = "HERALD_MATRIX_TOKEN" }}
[destinations.neighbour.interactions]
homeserver_token = {{ secret = "HERALD_MATRIX_HS" }}
appservice_token = {{ secret = "HERALD_MATRIX_AS" }}
sender = "neighbour"
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
channel = "{CHANNEL}"
template = "t"
subscribe = {{ role = "{ROLE}" }}
[[routes]]
id = "plain"
sources = ["gate"]
destination = "discord"
channel = "{CHANNEL}"
template = "t"
[[routes]]
id = "elsewhere"
sources = ["gate"]
destination = "discord"
channel = "{OTHER_CHANNEL}"
template = "t"
[[routes]]
id = "slacked"
sources = ["gate"]
destination = "slack"
channel = "{SLACK_CHANNEL}"
template = "t"
mentions = {{ subscribers = true }}
[[routes]]
id = "unmentioned"
sources = ["gate"]
destination = "slack"
channel = "C9999999999"
template = "t"
[[routes]]
id = "roomed"
sources = ["gate"]
destination = "matrix"
room = "{ROOM}"
template = "t"
mentions = {{ subscribers = true }}
[templates.t]
title = "{{{{ event.project }}}} {{{{ event.attributes.version }}}}"
body = "{{{{ event.body }}}}"
[templates.short]
title = "Short {{{{ event.attributes.version }}}}"
body = "Short notes"
[templates.broken]
title = "{{{{ event.missing }}}}"
body = "{{{{ event.missing }}}}"
"#
    );
    crate::harness::loaded(&text)
}

fn secrets() -> FakeSecrets {
    FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "bot-token")
        .with("HERALD_SLACK_TOKEN", "slack-token")
        .with("HERALD_SLACK_SIGNING", "signing-secret")
        .with("HERALD_SLACK_APP", "app-token")
        .with("HERALD_MATRIX_TOKEN", "matrix-token")
        .with("HERALD_MATRIX_HS", "hs-token")
        .with("HERALD_MATRIX_AS", "as-token")
}

pub(crate) struct World {
    config: Config,
    connected: Connected<Script>,
    pub(crate) store: Failing,
    keyring: MemoryKeyring,
    pub(crate) script: Script,
}

impl World {
    pub(crate) fn new(discord: &str, script: Script) -> Self {
        let config = config(discord);
        let prepared = pollster::block_on(async {
            prepare(&config, &secrets(), &FakeSigner, Uses::Serves).await
        })
        .unwrap();
        let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
        Self {
            config,
            connected,
            store: healthy(),
            keyring: MemoryKeyring::new(8),
            script,
        }
    }

    pub(crate) fn announced(self, project: &str, routes: &[&str]) -> Self {
        let shown = |version: &str, prerelease: bool, body: &str| Shown {
            kind: Kind::RELEASE,
            attributes: herald_core::versioned(version, prerelease),
            title: format!("{project} {version}"),
            body: body.to_owned(),
            links: BTreeMap::from([
                (
                    "release".to_owned(),
                    format!("https://example.org/{version}"),
                ),
                (
                    "changelog".to_owned(),
                    "https://example.org/changes".to_owned(),
                ),
            ]),
            occurred: at(),
        };
        let latest = Latest {
            project: project.to_owned(),
            newest: shown("2.0.0-rc.1", true, "## Next"),
            stable: Some(shown("1.0.0", false, "## Added\n\nA thing.")),
            routes: routes
                .iter()
                .map(|route| RouteId::new(*route))
                .collect::<BTreeSet<_>>(),
        };
        pollster::block_on(self.store.inner.put_latest(&latest, None)).unwrap();
        self
    }

    fn ports<T>(&self, act: impl FnOnce(&Ports<'_, Failing, FakeClock, FakeSigner>) -> T) -> T {
        let clock = FakeClock::at(at());
        act(&Ports {
            store: &self.store,
            clock: &clock,
            signer: &FakeSigner,
        })
    }

    pub(crate) fn interact(&self, asked: &Asked) -> Answer {
        self.ports(|ports| {
            pollster::block_on(interact(&self.config, &self.connected, ports, asked))
        })
    }

    pub(crate) fn serving<T>(&self, act: impl FnOnce(&Served<'_>) -> T) -> T {
        let keys = Keys {
            keyring: &self.keyring,
            refreshed: jiff::SignedDuration::from_hours(1),
            reread: jiff::SignedDuration::from_mins(1),
        };
        let webhooks = Webhooks::default();
        self.ports(|ports| {
            act(&Serving {
                config: &self.config,
                connected: &self.connected,
                ports,
                verifier: &FakeVerifier,
                public: &self.script,
                keys: &keys,
                webhooks: &webhooks,
                admission: herald_flows::Admission::Once,
            })
        })
    }

    pub(crate) fn answer(&self, inbound: &Inbound<'_>) -> Answered {
        self.serving(|serving| pollster::block_on(answer(serving, inbound)))
    }

    pub(crate) fn work(&self, job: Job) -> Worked {
        self.serving(|serving| pollster::block_on(work(serving, job, "server")))
    }
}

pub(crate) type Served<'a> =
    Serving<'a, Failing, FakeClock, FakeSigner, FakeVerifier, Script, Script, MemoryKeyring>;

pub(crate) fn world() -> World {
    World::new("", Script::default()).announced("gate", &["releases", "slacked", "roomed"])
}

pub(crate) fn discord(command: Command) -> Asked {
    Asked {
        destination: DestinationName::new("discord"),
        place: Place::new(CHANNEL),
        user: USER.to_owned(),
        command,
        project: None,
        share: false,
        toggles: false,
        member: Some(Member {
            guild: GUILD.to_owned(),
            roles: Vec::new(),
        }),
    }
}

fn slack(command: Command) -> Asked {
    Asked {
        destination: DestinationName::new("slack"),
        place: Place::new(SLACK_CHANNEL),
        user: "U0123456789".to_owned(),
        member: None,
        ..discord(command)
    }
}

fn said(answer: &Answer) -> &str {
    &answer.said
}

#[test]
fn latest_shows_the_newest_release_s_title_linked_with_its_day_to_the_asker() {
    let answer = world().interact(&discord(Command::Latest));
    assert_eq!(
        answer,
        Answer {
            said: "**[gate 1\\.0\\.0](https://example.org/1.0.0)**\n2027-01-15".to_owned(),
            public: false,
            trouble: None,
        }
    );
    let shared = world().interact(&Asked {
        share: true,
        ..discord(Command::Latest)
    });
    assert!(shared.public);
}

#[test]
fn a_destination_answering_pre_releases_shows_the_newest_with_its_own_template() {
    let world = World::new(
        "prereleases = true\ntemplate = \"short\"\nreplies = \"public\"",
        Script::default(),
    )
    .announced("gate", &["releases"]);
    let latest = world.interact(&discord(Command::Latest));
    assert_eq!(
        said(&latest),
        "**[Short 2\\.0\\.0\\-rc\\.1](https://example.org/2.0.0-rc.1)**\n2027-01-15"
    );
    assert!(latest.public);
    assert_eq!(
        said(&world.interact(&discord(Command::Changelog))),
        "Short notes\n\n[Changelog](https://example.org/changes)"
    );
}

#[test]
fn a_destination_s_wording_words_its_replies_its_days_and_its_links() {
    let worded = r#"wording = { subscribed = "Je krijgt een melding bij {{ project }}.", date = "%d-%m-%Y", changelog_link = "Wijzigingen", unavailable = "Even niet." }"#;
    let dutch = World::new(worded, Script::answering(&[(ROLE_PATH, 204, "")]))
        .announced("gate", &["releases"]);
    assert_eq!(
        said(&dutch.interact(&discord(Command::Latest))),
        "**[gate 1\\.0\\.0](https://example.org/1.0.0)**\n15-01-2027"
    );
    assert_eq!(
        said(&dutch.interact(&discord(Command::Changelog))),
        "## Added\n\nA thing.\n\n[Wijzigingen](https://example.org/changes)"
    );
    assert_eq!(
        said(&dutch.interact(&discord(Command::Subscribe))),
        "Je krijgt een melding bij gate."
    );
    let unworded = world().interact(&slack(Command::Latest));
    assert_eq!(
        said(&unworded),
        "**[gate 1\\.0\\.0](https://example.org/1.0.0)**\n2027-01-15"
    );
}

#[test]
fn changelog_shows_the_notes_with_a_link_to_the_changelog_or_the_release() {
    assert_eq!(
        said(&world().interact(&discord(Command::Changelog))),
        "## Added\n\nA thing.\n\n[Changelog](https://example.org/changes)"
    );
    let world = World::new("", Script::default());
    let mut latest = Latest {
        project: "bare".to_owned(),
        newest: Shown {
            kind: Kind::RELEASE,
            attributes: herald_core::versioned("v1", false),
            title: "Bare".to_owned(),
            body: "  ".to_owned(),
            links: BTreeMap::from([("release".to_owned(), "https://example.org/r".to_owned())]),
            occurred: at(),
        },
        stable: None,
        routes: BTreeSet::from([RouteId::new("releases")]),
    };
    pollster::block_on(world.store.inner.put_latest(&latest, None)).unwrap();
    assert_eq!(
        said(&world.interact(&discord(Command::Changelog))),
        "No notes were announced.\n\n[Release](https://example.org/r)"
    );
    latest.newest.links.clear();
    let version = pollster::block_on(world.store.inner.latest("bare"))
        .unwrap()
        .unwrap()
        .version;
    pollster::block_on(world.store.inner.put_latest(&latest, Some(version))).unwrap();
    assert_eq!(
        said(&world.interact(&discord(Command::Changelog))),
        "No notes were announced."
    );
    assert_eq!(
        said(&world.interact(&discord(Command::Latest))),
        "**bare v1**\n2027-01-15"
    );
}

#[test]
fn a_project_not_chosen_is_named_and_the_choices_listed() {
    let empty = World::new("", Script::default());
    assert_eq!(
        said(&empty.interact(&discord(Command::Latest))),
        "Nothing has been announced here yet."
    );
    let many = world().announced("herald", &["plain"]);
    assert_eq!(
        said(&many.interact(&discord(Command::Latest))),
        "More than one project is announced here; name one of gate, herald."
    );
    let named = many.interact(&Asked {
        project: Some("HERALD".to_owned()),
        ..discord(Command::Latest)
    });
    assert_eq!(
        said(&named),
        "**[herald 1\\.0\\.0](https://example.org/1.0.0)**\n2027-01-15"
    );
    assert_eq!(
        said(&many.interact(&Asked {
            project: Some("nope".to_owned()),
            ..discord(Command::Latest)
        })),
        "nope is not announced here; name one of gate, herald."
    );
}

#[test]
fn a_command_a_destination_does_not_answer_says_so() {
    let world = World::new("commands = [\"latest\", \"subscribe\"]", Script::default())
        .announced("gate", &["releases"]);
    assert_eq!(
        said(&world.interact(&discord(Command::Changelog))),
        "The command changelog is not answered here."
    );
    let quiet = Asked {
        destination: DestinationName::new("quiet"),
        ..discord(Command::Latest)
    };
    assert_eq!(
        said(&world.interact(&quiet)),
        "The command latest is not answered here."
    );
}

#[test]
fn a_store_that_cannot_list_or_an_unshowable_record_is_answered_as_unavailable() {
    let mut failing = world();
    failing.store.fault = Fault::ListsLatest;
    let answer = failing.interact(&discord(Command::Latest));
    assert_eq!(
        said(&answer),
        "herald could not answer just now; try again later."
    );
    assert_eq!(
        answer.trouble.as_deref(),
        Some("the store could not be reached: broken")
    );
    let world = World::new("", Script::default());
    let untitled = Latest {
        project: "untitled".to_owned(),
        newest: Shown {
            kind: Kind::RELEASE,
            attributes: std::collections::BTreeMap::default(),
            title: String::new(),
            body: String::new(),
            links: BTreeMap::new(),
            occurred: at(),
        },
        stable: None,
        routes: BTreeSet::from([RouteId::new("releases")]),
    };
    pollster::block_on(world.store.inner.put_latest(&untitled, None)).unwrap();
    assert_eq!(
        world.interact(&discord(Command::Latest)).trouble.as_deref(),
        Some("the event has no title")
    );
    let unrendered =
        World::new("template = \"broken\"", Script::default()).announced("gate", &["releases"]);
    for (command, template) in [(Command::Latest, "title"), (Command::Changelog, "body")] {
        let answer = unrendered.interact(&discord(command));
        assert_eq!(
            said(&answer),
            "herald could not answer just now; try again later."
        );
        assert_eq!(
            answer.trouble,
            Some(format!(
                "template broken/{template}: undefined value (in broken/{template}:1)"
            ))
        );
    }
}

fn role_world(answers: &[(&str, u16, &str)]) -> World {
    World::new("", Script::answering(answers)).announced("gate", &["releases"])
}

const ROLE_PATH: &str =
    "/guilds/223456789012345678/members/423456789012345678/roles/523456789012345678";

#[test]
fn subscribing_on_discord_gives_the_opt_in_role_and_unsubscribing_takes_it() {
    let world = role_world(&[(ROLE_PATH, 204, "")]);
    assert_eq!(
        said(&world.interact(&discord(Command::Subscribe))),
        "You will be mentioned when gate announces a release."
    );
    assert_eq!(
        world.script.seen(),
        [format!("PUT https://discord.com/api/v10{ROLE_PATH}")]
    );
    let holding = Asked {
        member: Some(Member {
            guild: GUILD.to_owned(),
            roles: vec![ROLE.to_owned()],
        }),
        ..discord(Command::Subscribe)
    };
    assert_eq!(
        said(&world.interact(&holding)),
        "You are already mentioned when gate announces a release."
    );
    let world = role_world(&[(ROLE_PATH, 204, "")]);
    assert_eq!(
        said(&world.interact(&Asked {
            toggles: true,
            ..holding.clone()
        })),
        "You will no longer be mentioned when gate announces a release."
    );
    assert_eq!(
        world.script.seen(),
        [format!("DELETE https://discord.com/api/v10{ROLE_PATH}")]
    );
    let world = role_world(&[(ROLE_PATH, 204, "")]);
    assert_eq!(
        said(&world.interact(&Asked {
            command: Command::Unsubscribe,
            ..holding
        })),
        "You will no longer be mentioned when gate announces a release."
    );
    assert_eq!(
        said(&role_world(&[]).interact(&discord(Command::Unsubscribe))),
        "You are not mentioned when gate announces a release."
    );
}

#[test]
fn discord_refusing_a_role_or_no_role_to_give_is_said() {
    let refused = role_world(&[(ROLE_PATH, 403, r#"{"message": "Missing Permissions"}"#)])
        .interact(&discord(Command::Subscribe));
    assert_eq!(
        refused.trouble.as_deref(),
        Some("Discord refused it with 403: Missing Permissions")
    );
    let unwritten = role_world(&[]).interact(&Asked {
        member: Some(Member {
            guild: "not-an-id".to_owned(),
            roles: Vec::new(),
        }),
        ..discord(Command::Subscribe)
    });
    assert_eq!(
        unwritten.trouble.as_deref(),
        Some("a role could not be asked of Discord: an id is not written as Discord writes one")
    );
    let direct = role_world(&[]).interact(&Asked {
        member: None,
        ..discord(Command::Subscribe)
    });
    assert_eq!(said(&direct), "Subscribing to gate is not offered here.");
    let roleless = World::new("", Script::default()).announced("gate", &["plain"]);
    assert_eq!(
        said(&roleless.interact(&discord(Command::Subscribe))),
        "Subscribing to gate is not offered here."
    );
}

fn subscribed(world: &World) -> Vec<String> {
    pollster::block_on(
        world
            .store
            .inner
            .subscribers(&DestinationName::new("slack"), "gate"),
    )
    .unwrap()
    .into_iter()
    .map(|subscription| subscription.subscriber)
    .collect()
}

#[test]
fn subscribing_on_slack_keeps_the_member_in_the_store_until_they_unsubscribe() {
    let world = world();
    assert_eq!(
        said(&world.interact(&slack(Command::Subscribe))),
        "You will be mentioned when gate announces a release."
    );
    assert_eq!(subscribed(&world), ["U0123456789"]);
    assert_eq!(
        said(&world.interact(&slack(Command::Subscribe))),
        "You are already mentioned when gate announces a release."
    );
    assert_eq!(
        said(&world.interact(&Asked {
            toggles: true,
            ..slack(Command::Subscribe)
        })),
        "You will no longer be mentioned when gate announces a release."
    );
    assert_eq!(subscribed(&world), Vec::<String>::new());
    assert_eq!(
        said(&world.interact(&slack(Command::Unsubscribe))),
        "You are not mentioned when gate announces a release."
    );
    pollster::block_on(world.store.inner.subscribe(&Subscription {
        destination: DestinationName::new("slack"),
        project: "gate".to_owned(),
        subscriber: "U0123456789".to_owned(),
        inbox: None,
        pending: None,
    }))
    .unwrap();
    assert_eq!(
        said(&world.interact(&slack(Command::Unsubscribe))),
        "You will no longer be mentioned when gate announces a release."
    );
}

#[test]
fn subscribing_where_no_route_mentions_subscribers_or_the_store_fails_is_said() {
    let world = World::new("", Script::default()).announced("gate", &["unmentioned"]);
    let elsewhere = Asked {
        place: Place::new("C9999999999"),
        ..slack(Command::Subscribe)
    };
    assert_eq!(
        said(&world.interact(&elsewhere)),
        "Subscribing to gate is not offered here."
    );
    for fault in [Fault::Subscribers, Fault::Subscribes] {
        let mut failing = World::new("", Script::default()).announced("gate", &["slacked"]);
        failing.store.fault = fault;
        assert_eq!(
            failing
                .interact(&slack(Command::Subscribe))
                .trouble
                .as_deref(),
            Some("the store could not be reached: broken"),
            "{fault:?}"
        );
    }
}

#[test]
fn only_a_run_that_answers_commands_reads_the_secrets_it_answers_with() {
    let config = config("");
    let announcing = FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "bot-token")
        .with("HERALD_SLACK_TOKEN", "slack-token")
        .with("HERALD_MATRIX_TOKEN", "matrix-token");
    let prepared =
        pollster::block_on(prepare(&config, &announcing, &FakeSigner, Uses::Polls)).unwrap();
    let unanswering = prepared
        .connect(|_| Ok::<_, ()>(Script::default()))
        .unwrap();
    assert_eq!(unanswering.interactions().count(), 0);
    assert_eq!(
        pollster::block_on(prepare(&config, &announcing, &FakeSigner, Uses::Serves))
            .err()
            .map(|error| error.to_string()),
        Some(
            [
                "destinations.matrix.interactions.appservice_token, destinations.neighbour.interactions.appservice_token: no secret is named HERALD_MATRIX_AS; set it where this platform keeps secrets",
                "destinations.matrix.interactions.homeserver_token, destinations.neighbour.interactions.homeserver_token: no secret is named HERALD_MATRIX_HS; set it where this platform keeps secrets",
                "destinations.socket.interactions.app: no secret is named HERALD_SLACK_APP; set it where this platform keeps secrets",
                "destinations.slack.interactions.signing: no secret is named HERALD_SLACK_SIGNING; set it where this platform keeps secrets",
            ]
            .join("\n")
        )
    );
}

#[test]
fn each_destination_answers_through_its_own_base_and_socket_mode_opens_through_slack_s_api() {
    let config = config("");
    let prepared =
        pollster::block_on(async { prepare(&config, &secrets(), &FakeSigner, Uses::Serves).await })
            .unwrap();
    let connected = prepared
        .connect(|base| Ok::<_, ()>(base.to_owned()))
        .unwrap();
    let bases: Vec<(&str, &str, Option<&str>)> = connected
        .interactions()
        .map(|(name, _, answers, sessions)| {
            (
                name.as_str(),
                answers.as_str(),
                sessions.map(String::as_str),
            )
        })
        .collect();
    let discord = "https://discord.com/api/v10";
    let hooks = "https://hooks.slack.com";
    assert_eq!(
        bases,
        [
            ("discord", discord, None),
            ("gatewayed", discord, None),
            ("matrix", "https://matrix.example", None),
            ("neighbour", "https://matrix.example", None),
            ("slack", hooks, None),
            ("socket", hooks, Some("https://slack.com/api")),
        ]
    );
}

#[test]
fn only_the_destinations_that_take_commands_are_answered_for() {
    let config = crate::harness::config(
        r#"[destinations.slack]
kind = "slack"
token = { secret = "HERALD_DISCORD_TOKEN" }
[destinations.matrix]
kind = "matrix"
homeserver = "https://matrix.example"
token = { secret = "HERALD_DISCORD_TOKEN" }
[destinations.synced]
kind = "matrix"
homeserver = "https://matrix.example"
token = { secret = "HERALD_DISCORD_TOKEN" }
[destinations.synced.interactions]
via = "sync"
[destinations.fedi]
kind = "mastodon"
server = "https://social.example"
token = { secret = "HERALD_DISCORD_TOKEN" }"#,
    );
    let secrets = crate::harness::secrets();
    let prepared =
        pollster::block_on(async { prepare(&config, &secrets, &FakeSigner, Uses::Serves).await })
            .unwrap();
    let connected = prepared
        .connect(|_| Ok::<_, ()>(Script::default()))
        .unwrap();
    let taking: Vec<&str> = connected
        .interactions()
        .map(|(name, ..)| name.as_str())
        .collect();
    assert_eq!(taking, ["synced"]);
}

#[test]
fn completions_list_what_was_announced_in_the_channel() {
    let world = world()
        .announced("herald", &["elsewhere"])
        .announced("other", &["plain"]);
    let found = pollster::block_on(completed(
        &world.config,
        &world.store,
        &DestinationName::new("discord"),
        &Place::new(CHANNEL),
        "",
    ));
    assert_eq!(found, ["gate", "other"]);
    let in_a_thread = pollster::block_on(completed(
        &world.config,
        &world.store,
        &DestinationName::new("discord"),
        &Place {
            id: "923456789012345678".to_owned(),
            parent: Some(CHANNEL.to_owned()),
        },
        "",
    ));
    assert_eq!(in_a_thread, ["gate", "other"]);
    let mut failing = world;
    failing.store.fault = Fault::ListsLatest;
    let none = pollster::block_on(completed(
        &failing.config,
        &failing.store,
        &DestinationName::new("discord"),
        &Place::new(CHANNEL),
        "",
    ));
    assert_eq!(none, Vec::<String>::new());
}

pub(crate) fn discord_replying() -> Replying {
    Replying::new(herald_dest_discord::Replying {
        id: "900".to_owned(),
        token: Secret::new("the interaction's token", "tok"),
    })
}

#[test]
fn an_answer_is_sent_in_its_destination_s_parts_and_a_refusal_said() {
    let world = World::new(
        "",
        Script::answering(&[(
            &format!("/webhooks/{APPLICATION}/{TOKEN}/messages/@original"),
            200,
            "{}",
        )]),
    );
    let answer = Answer {
        said: "**hi**".to_owned(),
        public: false,
        trouble: None,
    };
    let destination = DestinationName::new("discord");
    let sent = pollster::block_on(replied(
        &world.connected,
        &destination,
        &discord_replying(),
        &answer,
    ));
    assert_eq!(sent, Ok(()));
    assert_eq!(
        world.script.bodies()[0],
        br#"{"allowed_mentions":{"parse":[]},"content":"**hi**"}"#
    );
    let refused = World::new(
        "",
        Script::answering(&[(
            "/messages/@original",
            404,
            r#"{"message": "Unknown Webhook"}"#,
        )]),
    );
    assert_eq!(
        pollster::block_on(replied(
            &refused.connected,
            &destination,
            &discord_replying(),
            &answer
        )),
        Err("Discord refused it with 404: Unknown Webhook".to_owned())
    );
    let quiet = DestinationName::new("quiet");
    assert_eq!(
        pollster::block_on(replied(
            &world.connected,
            &quiet,
            &discord_replying(),
            &answer
        )),
        Err("quiet takes no commands".to_owned())
    );
    let unreachable = World::new("", Script::default());
    assert_eq!(
        pollster::block_on(replied(
            &unreachable.connected,
            &destination,
            &discord_replying(),
            &answer
        ))
        .unwrap_err(),
        "the host could not be reached: nothing scripted for PATCH https://discord.com/api/v10/webhooks/123456789012345678/«secret:the interaction's token»/messages/@original"
    );
    let mismatched = pollster::block_on(replied(
        &world.connected,
        &DestinationName::new("slack"),
        &discord_replying(),
        &answer,
    ));
    assert_eq!(mismatched, Ok(()));
}

#[test]
fn a_command_the_gateway_carried_is_deferred_first_and_others_are_not() {
    let world = World::new(
        "",
        Script::answering(&[(&format!("/interactions/900/{TOKEN}/callback"), 204, "")]),
    );
    let destination = DestinationName::new("discord");
    assert_eq!(
        pollster::block_on(deferral(
            &world.connected,
            &destination,
            &discord_replying(),
            &discord(Command::Latest)
        )),
        Ok(())
    );
    assert_eq!(
        world.script.bodies()[0],
        br#"{"data":{"flags":64},"type":5}"#
    );
    let slack_replying = Replying::new(herald_dest_slack::Replying {
        url: Secret::new("the command's response URL", HOOK),
    });
    assert_eq!(
        pollster::block_on(deferral(
            &world.connected,
            &DestinationName::new("slack"),
            &slack_replying,
            &slack(Command::Latest)
        )),
        Ok(())
    );
    assert_eq!(
        pollster::block_on(deferral(
            &world.connected,
            &DestinationName::new("quiet"),
            &discord_replying(),
            &discord(Command::Latest)
        )),
        Err("quiet takes no commands".to_owned())
    );
}
