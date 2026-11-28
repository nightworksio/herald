use std::collections::BTreeMap;
use std::path::Path;

use super::init::{git, init_trying, key, written};
use super::world::*;
use crate::init::Trying;

/// The secret a first run on a Discord webhook names.
const NAMED: &str = "HERALD_DISCORD_WEBHOOK";

/// Every answer a first run on a Discord webhook asks but the presets, as
/// flags.
const FLAGS: [&str; 12] = [
    "--forge",
    "github",
    "--repository",
    "owner/app",
    "--api",
    "none",
    "--chat",
    "discord",
    "--variant",
    "webhook",
    "--with",
    "webhook=HERALD_DISCORD_WEBHOOK",
];

/// The presets a first run announces with where its flags name none.
const ANNOUNCED: [&str; 2] = ["--announce", "herald:announce-releases"];

/// A Discord webhook's URL, built at run time.
fn webhook() -> String {
    format!(
        "https://discord.com/api/webhooks/{}/{}",
        "1".repeat(19),
        "t".repeat(68)
    )
}

/// The part of [`webhook`] that is the credential.
fn credential() -> String {
    "t".repeat(68)
}

/// What a first run in `directory` does with `extra` flags past [`FLAGS`]
/// and, where they name no preset, [`ANNOUNCED`], the operator answering
/// `answer`, in an environment of `vars`, reaching `world` where one is
/// given and nothing otherwise, a terminal there where `unechoed` reads
/// from it.
fn first_run(
    directory: &Path,
    (extra, answer): (&[&str], &str),
    vars: &BTreeMap<String, String>,
    world: Option<&World>,
    unechoed: Option<&dyn Fn() -> std::io::Result<String>>,
) -> (Result<u8, String>, String) {
    git(directory, "git@github.com:owner/app.git");
    let reaching = |base: herald_ports::Base<'_>, _: &herald_ports::Outbound| {
        world
            .map(|world| crate::Connection::new(world.clone()))
            .ok_or(format!("{base} cannot be reached"))
    };
    let waiting = |_: &str, _: &herald_ports::Outbound| Err(String::new());
    let trying = Trying {
        vars,
        connect: &crate::Connector::new(&reaching, &waiting),
        unechoed,
    };
    let mut args = FLAGS.to_vec();
    args.extend(extra);
    if !extra.contains(&ANNOUNCED[0]) {
        args.extend(ANNOUNCED);
    }
    init_trying(directory, &args, answer, (Some(key()), &trying))
}

/// The environment setting the webhook's secret to `url`.
fn setting(url: String) -> BTreeMap<String, String> {
    BTreeMap::from([(NAMED.to_owned(), url)])
}

/// A world whose Discord answers its first request with `status` and
/// `body`.
fn answering(status: u16, body: &str) -> World {
    let world = World::default();
    world.state.lock().unwrap().discord = vec![(String::new(), status, body.to_owned())];
    world
}

/// The line saying how to post the test message later, for the config at
/// `directory`.
fn later(directory: &Path) -> String {
    format!(
        "herald --config {} doctor --online --post posts it once the environment sets {NAMED}.",
        directory.join("herald.toml").display()
    )
}

/// The requests `world` was sent.
fn seen(world: &World) -> Vec<String> {
    world.state.lock().unwrap().seen.clone()
}

#[test]
fn a_first_run_shows_its_test_message_and_posts_it_once_with_the_credential_the_environment_sets() {
    let directory = tempfile::tempdir().unwrap();
    let world = answering(200, r#"{"id":"1"}"#);
    let unasked = || -> std::io::Result<String> { panic!("the terminal was read") };
    let (done, out) = first_run(
        directory.path(),
        (
            &[
                "--announce",
                "herald:announce-releases",
                "--announce",
                "herald:review-reminders",
            ],
            "y\ny\n",
        ),
        &setting(webhook()),
        Some(&world),
        Some(&unasked),
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    assert!(out.contains("\nThe test message:\n"), "{out}");
    assert!(out.contains("  title: herald test message\n"), "{out}");
    assert!(
        out.contains("the route announce\\-releases\\-releases reaches here"),
        "{out}"
    );
    assert!(out.contains("Post it to discord? [y/N] "), "{out}");
    assert!(
        out.contains("A test message was posted on the route "),
        "{out}"
    );
    let sent = seen(&world);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(
        sent[0].starts_with("POST https://discord.com/api/webhooks/"),
        "{sent:?}"
    );
    assert!(!out.contains(&credential()), "{out}");
    assert!(!written(directory.path()).unwrap().contains(&credential()));
}

#[test]
fn a_credential_the_environment_does_not_set_is_typed_on_the_terminal_unseen_and_never_shown() {
    let directory = tempfile::tempdir().unwrap();
    let world = answering(200, r#"{"id":"1"}"#);
    let typed = || Ok(webhook());
    let (done, out) = first_run(
        directory.path(),
        (&[], "y\ny\n"),
        &BTreeMap::new(),
        Some(&world),
        Some(&typed),
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    assert!(
        out.contains(&format!("{NAMED}, not shown as it is typed: ")),
        "{out}"
    );
    assert!(
        out.contains("A test message was posted on the route "),
        "{out}"
    );
    assert_eq!(seen(&world).len(), 1);
    assert!(!out.contains(&credential()), "{out}");
    assert!(!written(directory.path()).unwrap().contains(&credential()));
}

#[test]
fn without_a_terminal_or_with_nothing_typed_nothing_is_posted_and_the_command_to_post_later_is_said()
 {
    let typed_nothing = || Ok(String::new());
    let unread = || Err(std::io::Error::other("no terminal"));
    let unechoed: [Option<&dyn Fn() -> std::io::Result<String>>; 3] =
        [None, Some(&typed_nothing), Some(&unread)];
    for (index, unechoed) in unechoed.into_iter().enumerate() {
        let directory = tempfile::tempdir().unwrap();
        let world = World::default();
        let (done, out) = first_run(
            directory.path(),
            (&[], "y\ny\n"),
            &BTreeMap::new(),
            Some(&world),
            unechoed,
        );
        assert_eq!(done, Ok(crate::DONE), "{out}");
        let why = if index == 0 {
            format!(
                "{NAMED} is not set, and no terminal is there to type it on unseen, so nothing was posted; "
            )
        } else {
            format!("Nothing was typed for {NAMED}, so nothing was posted; ")
        };
        assert!(
            out.contains(&format!("{why}{}\n", later(directory.path()))),
            "{out}"
        );
        assert_eq!(seen(&world), Vec::<String>::new());
        assert!(written(directory.path()).is_some());
    }
}

#[test]
fn under_yes_or_where_the_operator_declines_nothing_is_posted_and_the_command_to_post_later_is_said()
 {
    for (extra, answer) in [(&["--yes"][..], ""), (&[][..], "y\nn\n"), (&[][..], "y\n")] {
        let directory = tempfile::tempdir().unwrap();
        let world = World::default();
        let (done, out) = first_run(
            directory.path(),
            (extra, answer),
            &setting(webhook()),
            Some(&world),
            None,
        );
        assert_eq!(done, Ok(crate::DONE), "{out}");
        assert!(out.contains("  title: herald test message\n"), "{out}");
        assert!(
            out.contains(&format!(
                "Nothing was posted; {}\n",
                later(directory.path())
            )),
            "{out}"
        );
        assert_eq!(seen(&world), Vec::<String>::new());
    }
}

#[test]
fn a_test_message_the_chat_refuses_says_what_it_answered_without_the_credential_and_how_to_retry() {
    let directory = tempfile::tempdir().unwrap();
    let echoed = format!(
        r#"{{"message": "Unknown Webhook {}", "code": 10015}}"#,
        webhook()
    );
    let world = answering(404, &echoed);
    let (done, out) = first_run(
        directory.path(),
        (&[], "y\ny\n"),
        &setting(webhook()),
        Some(&world),
        None,
    );
    assert_eq!(done, Ok(crate::STOPPED), "{out}");
    assert!(out.contains("Unknown Webhook"), "{out}");
    assert!(!out.contains(&credential()), "{out}");
    assert!(
        out.contains(&format!(
            "The files stay written; check {NAMED} and what the chat answered above, then run herald --config {} doctor --online --post with {NAMED} set.\n",
            directory.path().join("herald.toml").display()
        )),
        "{out}"
    );
    assert!(written(directory.path()).is_some());
}

#[test]
fn a_chat_that_cannot_be_reached_is_said_and_the_files_stay_written() {
    let directory = tempfile::tempdir().unwrap();
    let (done, out) = first_run(
        directory.path(),
        (&[], "y\ny\n"),
        &setting(webhook()),
        None,
        None,
    );
    assert_eq!(done, Ok(crate::STOPPED), "{out}");
    assert!(
        out.contains("No test message was posted, as https://discord.com"),
        "{out}"
    );
    assert!(out.contains("cannot be reached"), "{out}");
    assert!(out.contains("The files stay written; "), "{out}");
    assert!(!out.contains(&credential()), "{out}");
    assert!(written(directory.path()).is_some());
}

#[test]
fn a_config_with_no_route_posts_no_test_message() {
    let directory = tempfile::tempdir().unwrap();
    let world = World::default();
    let (done, out) = first_run(
        directory.path(),
        (&["--announce", "none"], "y\n"),
        &setting(webhook()),
        Some(&world),
        None,
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    assert!(
        out.contains("The config holds no route, so no test message is posted.\n"),
        "{out}"
    );
    assert!(!out.contains("The test message:"), "{out}");
    assert_eq!(seen(&world), Vec::<String>::new());
}
