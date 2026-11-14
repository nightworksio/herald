use super::world::*;
use crate::*;

const APPLICATION: &str = "123456789012345678";
const GUILD: &str = "223456789012345678";
const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn config(extra: &str) -> String {
    format!(
        r#"version = 1
[store]
kind = "git-branch"
key = {{ secret = "HERALD_STORE_KEY" }}
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[destinations.discord.interactions]
application = "{APPLICATION}"
key = "{KEY}"
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
[destinations.synced]
kind = "matrix"
homeserver = "https://matrix.example"
token = {{ secret = "HERALD_MATRIX_TOKEN" }}
[destinations.synced.interactions]
via = "sync"
{extra}
"#
    )
}

#[test]
fn discord_s_commands_are_registered_in_every_server() {
    let setup = setup(&config(""));
    let world = World::default();
    discord(
        &world,
        &[(&format!("/applications/{APPLICATION}/commands"), 200, "[]")],
    );
    let registered = herald(&setup, &world, &["discord", "commands"]);
    assert_eq!(registered.code, DONE, "{}", registered.err);
    assert_eq!(
        registered.out,
        "Registered subscribe, unsubscribe, latest, changelog with Discord in every server the application is in.\n"
    );
    discord(
        &world,
        &[(
            &format!("/applications/{APPLICATION}/commands"),
            403,
            r#"{"message": "Missing Access"}"#,
        )],
    );
    let refused = herald(
        &setup,
        &world,
        &["discord", "commands", "--destination", "discord"],
    );
    assert_eq!(refused.code, STOPPED);
    assert_eq!(
        refused.err,
        "Discord refused it with 403: Missing Access; check what herald's credential is allowed to do there\n"
    );
    world
        .state
        .lock()
        .unwrap()
        .unanswered
        .push(format!("/applications/{APPLICATION}/commands"));
    let silent = herald(&setup, &world, &["discord", "commands"]);
    assert_eq!(silent.err, "the host did not answer in time\n");
}

#[test]
fn discord_s_commands_are_registered_in_the_server_its_interactions_name() {
    let text = config("").replace(
        &format!("key = \"{KEY}\""),
        &format!("key = \"{KEY}\"\nguild = \"{GUILD}\""),
    );
    let setup = setup(&text);
    let world = World::default();
    discord(
        &world,
        &[(
            &format!("/applications/{APPLICATION}/guilds/{GUILD}/commands"),
            200,
            "[]",
        )],
    );
    let registered = herald(&setup, &world, &["discord", "commands"]);
    assert!(
        registered
            .out
            .ends_with(&format!("in the server {GUILD}.\n")),
        "{}",
        registered.out
    );
}

#[test]
fn a_server_s_roles_are_listed_and_an_opt_in_role_made() {
    let setup = setup(&config(""));
    let world = World::default();
    discord(
        &world,
        &[(
            &format!("/guilds/{GUILD}/roles"),
            200,
            r#"[{"id": "9", "name": "updates"}]"#,
        )],
    );
    let listed = herald(&setup, &world, &["discord", "roles", "--guild", GUILD]);
    assert_eq!(listed.out, "9  updates\n");
    discord(
        &world,
        &[(
            &format!("/guilds/{GUILD}/roles"),
            200,
            r#"{"id": "10", "name": "gate updates"}"#,
        )],
    );
    let made = herald(
        &setup,
        &world,
        &[
            "discord",
            "roles",
            "--guild",
            GUILD,
            "--create",
            "gate updates",
        ],
    );
    assert_eq!(
        made.out,
        "Made the role gate updates. Offer it on a route with:\nsubscribe = { role = \"10\" }\n"
    );
    for args in [
        vec!["discord", "roles", "--guild", "x"],
        vec!["discord", "roles", "--guild", "x", "--create", "r"],
    ] {
        let unwritten = herald(&setup, &world, &args);
        assert_eq!(
            unwritten.err,
            "the server's id is not written as Discord writes one\n"
        );
    }
}

#[test]
fn a_destination_that_cannot_be_chosen_or_has_no_bot_is_named() {
    let world = World::default();
    let unknown = herald(
        &setup(&config("")),
        &world,
        &["discord", "commands", "--destination", "nope"],
    );
    assert_eq!(
        unknown.err,
        "no discord destination named \"nope\" takes commands; the ones that do are discord\n"
    );
    let twice = config(&format!(
        "[destinations.other]\nkind = \"discord\"\ntoken = {{ secret = \"HERALD_DISCORD_TOKEN\" }}\n[destinations.other.interactions]\napplication = \"{APPLICATION}\"\nkey = \"{KEY}\""
    ));
    let both = herald(&setup(&twice), &world, &["discord", "commands"]);
    assert_eq!(
        both.err,
        "more than one discord destination takes commands; name one of discord, other with --destination\n"
    );
    let many = herald(&setup(&config("")), &world, &["slack", "manifest"]);
    assert_eq!(
        many.err,
        "more than one slack destination takes commands; name one of slack, socket with --destination\n"
    );
    let none = herald(&setup(CONFIG), &world, &["discord", "commands"]);
    assert_eq!(
        none.err,
        "no discord destination takes commands; give one [destinations.<name>.interactions]\n"
    );
    let hooked = config("").replace(
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }\n[destinations.discord.interactions]",
        "webhook = { secret = \"HERALD_DISCORD_TOKEN\" }\n[destinations.discord.interactions]",
    );
    let botless = herald(&setup(&hooked), &world, &["discord", "commands"]);
    assert_eq!(
        botless.err,
        "discord has no bot token, which Discord's commands and roles are set up with\n"
    );
    let mut unset = setup(&config(""));
    unset.vars.remove("HERALD_DISCORD_TOKEN");
    let missing = herald(&unset, &world, &["discord", "commands"]);
    assert!(
        missing.err.contains("HERALD_DISCORD_TOKEN"),
        "{}",
        missing.err
    );
    let unreachable = config("").replace(
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }\n[destinations.discord.interactions]",
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }\napi = \"https://unreachable.example\"\n[destinations.discord.interactions]",
    );
    let away = herald(&setup(&unreachable), &world, &["discord", "commands"]);
    assert_eq!(away.err, "https://unreachable.example cannot be reached\n");
}

#[test]
fn a_slack_manifest_posts_to_herald_s_address_or_opens_socket_mode() {
    let world = World::default();
    let setup = setup(&config(""));
    let posted = herald(
        &setup,
        &world,
        &[
            "slack",
            "manifest",
            "--destination",
            "slack",
            "--url",
            "https://herald.example/",
        ],
    );
    assert_eq!(posted.code, DONE, "{}", posted.err);
    let mut lines = posted.out.lines();
    let manifest: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(
        manifest["settings"]["interactivity"]["request_url"],
        "https://herald.example/interactions/slack"
    );
    assert_eq!(
        lines.next(),
        Some(
            "Make a Slack app from this manifest at https://api.slack.com/apps, install it, and keep its signing secret as HERALD_SLACK_SIGNING."
        )
    );
    let unaddressed = herald(
        &setup,
        &world,
        &["slack", "manifest", "--destination", "slack"],
    );
    assert!(
        unaddressed
            .err
            .starts_with("Slack posts commands to herald's address")
    );
    let unsound = herald(
        &setup,
        &world,
        &[
            "slack",
            "manifest",
            "--destination",
            "slack",
            "--url",
            "herald.example",
        ],
    );
    assert_eq!(
        unsound.err,
        "\"herald.example\" is not an address herald is served at; give one such as https://herald.example\n"
    );
    let socket = herald(
        &setup,
        &world,
        &["slack", "manifest", "--destination", "socket"],
    );
    assert!(socket.out.contains("\"socket_mode_enabled\":true"));
    assert!(
        socket
            .out
            .ends_with("keep an app-level token with connections:write as HERALD_SLACK_APP.\n")
    );
    let addressed = herald(
        &setup,
        &world,
        &[
            "slack",
            "manifest",
            "--destination",
            "socket",
            "--url",
            "herald.example",
        ],
    );
    assert_eq!(addressed.code, DONE, "{}", addressed.err);
    assert_eq!(addressed.out, socket.out);
}

#[test]
fn a_matrix_registration_holds_two_new_tokens_and_reaches_herald_by_destination() {
    let world = World::default();
    let setup = setup(&config(""));
    let registered = herald(
        &setup,
        &world,
        &[
            "matrix",
            "registration",
            "--destination",
            "matrix",
            "--url",
            "https://herald.example",
        ],
    );
    assert_eq!(registered.code, DONE, "{}", registered.err);
    assert!(registered.out.starts_with("Keep the homeserver's token as HERALD_MATRIX_HS and the application service's token as HERALD_MATRIX_AS"));
    assert!(
        registered
            .out
            .contains("url: 'https://herald.example/matrix/matrix'\n")
    );
    let tokens: Vec<&str> = registered
        .out
        .lines()
        .filter_map(|line| {
            line.strip_prefix("as_token: ")
                .or_else(|| line.strip_prefix("hs_token: "))
        })
        .collect();
    assert_eq!(tokens.len(), 2);
    assert_ne!(tokens[0], tokens[1]);
    assert!(
        registered
            .out
            .ends_with("herald prints these tokens only this once.\n")
    );
    let synced = herald(
        &setup,
        &world,
        &[
            "matrix",
            "registration",
            "--destination",
            "synced",
            "--url",
            "https://herald.example",
        ],
    );
    assert_eq!(
        synced.err,
        "synced reads commands by syncing, so the homeserver installs nothing\n"
    );
    let config = herald_config::load(
        "herald.toml",
        &config(""),
        &|_| Err(String::new()),
        &herald_compose::kinds(),
    )
    .unwrap();
    let registered = |destination: &str, url: &str, random: &dyn Fn() -> Option<String>| {
        let setting = cli::Setting::Matrix(cli::Matrixing::Registration {
            destination: Some(destination.to_owned()),
            url: url.to_owned(),
        });
        let vars = std::collections::BTreeMap::new();
        let mut out = Vec::new();
        pollster::block_on(setup::set_up(
            &config, &vars, &NATIVE, &setting, random, &mut out,
        ))
    };
    let made = || Some("t".to_owned());
    assert_eq!(
        registered("matrix", "https://herald.example", &|| None),
        Err("the system's random generator failed, so no token was made".to_owned())
    );
    assert!(
        registered("nope", "https://herald.example", &made)
            .unwrap_err()
            .starts_with("no matrix destination named \"nope\" takes commands")
    );
    for unsound in [
        "ftp://x",
        "https://herald.example/a b",
        "https://herald.example/'",
    ] {
        assert!(
            registered("matrix", unsound, &made)
                .unwrap_err()
                .contains("is not an address herald is served at"),
            "{unsound}"
        );
    }
}

#[test]
fn a_manifest_and_a_registration_reach_herald_under_its_base() {
    let world = World::default();
    let setup = setup(&config("[server]\nbase = \"/tools/herald\"\n"));
    let posted = herald(
        &setup,
        &world,
        &[
            "slack",
            "manifest",
            "--destination",
            "slack",
            "--url",
            "https://herald.example/",
        ],
    );
    assert_eq!(posted.code, DONE, "{}", posted.err);
    let manifest: serde_json::Value =
        serde_json::from_str(posted.out.lines().next().unwrap()).unwrap();
    assert_eq!(
        manifest["settings"]["interactivity"]["request_url"],
        "https://herald.example/tools/herald/interactions/slack"
    );
    let registered = herald(
        &setup,
        &world,
        &[
            "matrix",
            "registration",
            "--destination",
            "matrix",
            "--url",
            "https://herald.example",
        ],
    );
    assert_eq!(registered.code, DONE, "{}", registered.err);
    assert!(
        registered
            .out
            .contains("url: 'https://herald.example/tools/herald/matrix/matrix'\n"),
        "{}",
        registered.out
    );
}
