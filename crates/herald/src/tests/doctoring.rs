use serde_json::{Value, json};

use super::world::*;
use crate::*;

fn reported(run: &Run) -> Value {
    serde_json::from_str(run.out.trim_end()).unwrap_or(Value::Null)
}

#[test]
fn doctor_prints_its_findings_as_json_with_the_first_layer_of_each_gap() {
    let world = World::default();
    let d1 = setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"d1\""));
    let run = herald(&d1, &world, &["doctor", "--format", "json"]);
    assert_eq!(run.code, STOPPED, "{}", run.err);
    let report = reported(&run);
    assert_eq!(report["layer"], "actions");
    assert_eq!(report["online"], false);
    assert_eq!(
        report["gaps"],
        json!([{
            "feature": "keeping records in d1",
            "key": "store.kind",
            "why": "this herald keeps records in sqlite, postgres, dynamodb, git-branch, markers, actions-cache or repo-variables; set [store] kind to one of them",
            "first_layer": "serverless",
        }])
    );
    assert_eq!(report["mistakes"], json!([]));
    assert!(
        report["notes"]
            .as_array()
            .is_some_and(|notes| !notes.is_empty())
    );
    let unstored = setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"nowhere\""));
    let unread = herald(
        &unstored,
        &world,
        &["doctor", "--format", "json", "--layer", "worker"],
    );
    assert_eq!(unread.code, MISTAKEN);
    let report = reported(&unread);
    assert_eq!(report["layer"], "worker");
    assert_eq!(report["gaps"], json!([]));
    assert!(
        report["mistakes"]
            .as_array()
            .is_some_and(|mistakes| mistakes.len() == 1),
        "{report}"
    );
    let text = herald(&unstored, &world, &["doctor"]);
    assert_eq!((text.code, text.out.as_str()), (MISTAKEN, ""));
    assert_ne!(text.err, "");
}

#[test]
fn json_keeps_every_mask_on_standard_error() {
    let mut actions = setup(CONFIG);
    actions
        .vars
        .insert("GITHUB_ACTIONS".to_owned(), "true".to_owned());
    let run = herald(&actions, &World::default(), &["doctor", "--format", "json"]);
    assert_eq!(run.code, DONE, "{}", run.err);
    assert!(run.err.contains("::add-mask::"), "{}", run.err);
    assert_eq!(reported(&run)["gaps"], json!([]));
    let text = herald(&actions, &World::default(), &["doctor"]);
    assert!(text.out.contains("::add-mask::"), "{}", text.out);
}

#[test]
fn the_committed_schema_is_the_one_the_report_generates() {
    assert_eq!(
        doctor_schema(),
        include_str!("../../../../herald-doctor.schema.json"),
        "regenerate it: cargo run -p herald --example doctor-schema > herald-doctor.schema.json"
    );
}

#[test]
fn each_layer_is_its_own_and_the_hosts_settings_are_read_here() {
    let world = World::default();
    for (layer, name) in [
        (cli::Layer::Actions, "actions"),
        (cli::Layer::Server, "server"),
        (cli::Layer::Lambda, "lambda"),
        (cli::Layer::Azure, "azure"),
    ] {
        assert_eq!(layer.served().name, name);
    }
    let every = cli::Layer::every();
    assert_eq!(every.len(), cli::Layer::ALL.len());
    assert!(every.windows(2).all(|pair| pair[0].tier <= pair[1].tier));
    let mut fastly = setup(CONFIG);
    let rounds = "HERALD_ROUND_SECRET: rounds of polling and housekeeping is served only where its setting is made";
    let run = herald(&fastly, &world, &["doctor", "--layer", "fastly"]);
    assert!(run.out.contains(rounds), "{}", run.out);
    let online = herald(
        &fastly,
        &world,
        &["doctor", "--layer", "fastly", "--online"],
    );
    assert_eq!(online.code, STOPPED, "{}", online.out);
    fastly
        .vars
        .insert("HERALD_ROUND_SECRET".to_owned(), "round-secret".to_owned());
    let set = herald(&fastly, &world, &["doctor", "--layer", "fastly"]);
    assert!(!set.out.contains(rounds), "{}", set.out);
    let azure = herald(&setup(CONFIG), &world, &["doctor", "--layer", "azure"]);
    assert!(
        azure
            .out
            .contains("On the azure layer, the poll and housekeep timer triggers in deploy/azure asks for rounds"),
        "{}",
        azure.out
    );
    for (layer, shipped) in [
        (
            cli::Layer::Lambda,
            [
                include_str!("../../../../deploy/lambda/template.yaml"),
                "rate(5 minutes)",
                "rate(5 minutes)",
            ],
        ),
        (
            cli::Layer::Azure,
            [
                concat!(
                    include_str!("../../../../deploy/azure/poll/function.json"),
                    include_str!("../../../../deploy/azure/housekeep/function.json")
                ),
                "\"0 */5 * * * *\"",
                "\"0 17 * * * *\"",
            ],
        ),
    ] {
        let herald_flows::Rounds::Platform(cadence) = layer.served().rounds else {
            panic!("{layer:?} asks for rounds on its platform");
        };
        let [files, poll, housekeep] = shipped;
        assert!(
            files.contains(poll) && files.contains(housekeep),
            "{layer:?}"
        );
        assert_eq!(cadence.poll, herald_compose::POLLED);
        let every = if poll == housekeep {
            herald_compose::POLLED
        } else {
            herald_compose::HOUSEKEPT
        };
        assert_eq!(cadence.housekeep, every);
    }
    let lambda = herald(&setup(CONFIG), &world, &["doctor", "--layer", "lambda"]);
    assert!(
        lambda
            .out
            .contains("a poll every 5m and housekeeping every 5m, as shipped"),
        "{}",
        lambda.out
    );
}

#[test]
fn online_doctor_asks_each_destination_and_reads_every_secret() {
    let world = World::default();
    world.state.lock().unwrap().discord = vec![
        ("/users/@me".to_owned(), 200, "{}".to_owned()),
        (
            "/channels/1566200000000000100".to_owned(),
            200,
            "{}".to_owned(),
        ),
    ];
    let run = herald(
        &setup(CONFIG),
        &world,
        &["doctor", "--online", "--format", "json"],
    );
    assert_eq!(run.code, DONE, "{}", run.out);
    let report = reported(&run);
    assert_eq!(report["online"], true);
    assert!(
        report["notes"].as_array().is_some_and(
            |notes| notes.contains(&json!("The destination \"discord\" takes its credential."))
        ),
        "{report}"
    );
    world.state.lock().unwrap().discord = vec![("/users/@me".to_owned(), 401, "{}".to_owned())];
    let refused = herald(&setup(CONFIG), &world, &["doctor", "--online"]);
    assert_eq!(refused.code, STOPPED, "{}", refused.out);
    assert!(
        refused.out.contains("destinations.discord: posting through the destination \"discord\" cannot be served: Discord refuses its credential, answering 401."),
        "{}",
        refused.out
    );
    let mut unset = setup(CONFIG);
    unset.vars.remove("HERALD_DISCORD_TOKEN");
    let unread = herald(&unset, &world, &["doctor", "--online"]);
    assert_eq!(unread.code, STOPPED, "{}", unread.out);
    assert!(
        unread.out.contains(
            "HERALD_DISCORD_TOKEN: reading the secret HERALD_DISCORD_TOKEN cannot be served"
        ),
        "{}",
        unread.out
    );
    assert!(
        unread.out.contains("No destination is asked, as "),
        "{}",
        unread.out
    );
    let unreachable = setup(&CONFIG.replace(
        "kind = \"discord\"\n",
        "kind = \"discord\"\napi = \"https://unreachable.example/api/v10\"\n",
    ));
    let unconnected = herald(&unreachable, &world, &["doctor", "--online"]);
    assert!(
        unconnected.out.contains(
            "No destination is asked, as https://unreachable.example/api/v10 cannot be reached."
        ),
        "{}",
        unconnected.out
    );
}

#[test]
fn an_actor_on_fastly_is_noted_until_its_egress_is_said_to_be_guarded() {
    let mut fastly = setup(&format!(
        "{CONFIG}[destinations.fedi]\nkind = \"activitypub\"\ndomain = \"herald.example\"\nkey = {{ secret = \"HERALD_ACTIVITYPUB_KEY\" }}\n"
    ));
    let guard = "destinations.fedi: herald's own ActivityPub actor is served only where its setting is made";
    let unguarded = herald(&fastly, &World::default(), &["doctor", "--layer", "fastly"]);
    assert!(unguarded.out.contains(guard), "{}", unguarded.out);
    fastly
        .vars
        .insert("HERALD_EGRESS_GUARDED".to_owned(), "yes".to_owned());
    let unsaid = herald(&fastly, &World::default(), &["doctor", "--layer", "fastly"]);
    assert!(unsaid.out.contains(guard), "{}", unsaid.out);
    fastly
        .vars
        .insert("HERALD_EGRESS_GUARDED".to_owned(), "true".to_owned());
    let guarded = herald(&fastly, &World::default(), &["doctor", "--layer", "fastly"]);
    assert!(!guarded.out.contains(guard), "{}", guarded.out);
}

#[test]
fn an_azure_server_warns_of_what_the_azure_layer_cannot_serve() {
    let mut azure = setup(&CONFIG.replace(
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }\n",
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }\n[destinations.discord.interactions]\napplication = \"1566184281406177000\"\nvia = \"gateway\"\n",
    ));
    azure
        .vars
        .insert("FUNCTIONS_CUSTOMHANDLER_PORT".to_owned(), "0".to_owned());
    let warned = "warning: destinations.discord.interactions.via: a session carrying commands cannot be served: a session is held open only on the worker or server layer";
    let served = herald_until(
        &azure,
        &World::default(),
        &["server"],
        std::future::ready(()),
    );
    assert!(served.out.contains(warned), "{}{}", served.out, served.err);
    azure.vars.remove("FUNCTIONS_CUSTOMHANDLER_PORT");
    azure
        .vars
        .insert("HERALD_LISTEN".to_owned(), "127.0.0.1:0".to_owned());
    let held = herald_until(
        &azure,
        &World::default(),
        &["server"],
        std::future::ready(()),
    );
    assert!(!held.out.contains(warned), "{}", held.out);
}
