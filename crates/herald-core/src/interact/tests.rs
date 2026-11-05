use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;

use super::*;
use crate::event::Kind;
use crate::latest::Shown;

fn latest(project: &str, routes: &[&str]) -> Latest {
    let shown = Shown {
        kind: Kind::Release,
        version: Some("1.0.0".to_owned()),
        title: project.to_owned(),
        body: String::new(),
        links: BTreeMap::new(),
        prerelease: false,
        occurred: Timestamp::UNIX_EPOCH,
    };
    Latest {
        project: project.to_owned(),
        newest: shown.clone(),
        stable: Some(shown),
        routes: routes
            .iter()
            .map(|route| RouteId::new(*route))
            .collect::<BTreeSet<_>>(),
    }
}

fn names(every_latest: &[&Latest]) -> Vec<String> {
    every_latest
        .iter()
        .map(|latest| latest.project.clone())
        .collect()
}

#[test]
fn every_command_is_named_in_any_case_and_described() {
    for command in Command::ALL {
        assert_eq!(Command::named(command.name()), Some(command));
        assert_eq!(
            Command::named(&command.name().to_uppercase()),
            Some(command)
        );
        assert!(
            command.description().ends_with("last") || command.description().contains("release")
        );
    }
    assert_eq!(Command::named("subscribed"), None);
    assert_eq!(
        Command::ALL.map(Command::name),
        ["subscribe", "unsubscribe", "latest", "changelog"]
    );
}

#[test]
fn only_latest_and_changelog_may_be_shared_and_public_replies_share_them() {
    assert!(!Command::Subscribe.shares());
    assert!(!Command::Unsubscribe.shares());
    assert!(Command::Latest.shares());
    assert!(Command::Changelog.shares());
    assert!(!Replies::Ephemeral.public(Command::Latest, false));
    assert!(Replies::Ephemeral.public(Command::Latest, true));
    assert!(Replies::Public.public(Command::Changelog, false));
    assert!(!Replies::Public.public(Command::Subscribe, false));
    assert!(!Replies::Ephemeral.public(Command::Unsubscribe, true));
    assert_eq!(Replies::default(), Replies::Ephemeral);
}

#[test]
fn what_was_announced_on_the_routes_is_listed_by_name() {
    let every_latest = [
        latest("zeta", &["a"]),
        latest("alpha", &["b", "a"]),
        latest("other", &["c"]),
    ];
    let routes = [RouteId::new("a"), RouteId::new("b")];
    assert_eq!(names(&announced(&every_latest, &routes)), ["alpha", "zeta"]);
    assert_eq!(announced(&every_latest, &[]), Vec::<&Latest>::new());
}

#[test]
fn the_project_named_or_the_only_one_is_chosen() {
    let herald = latest("herald", &["a"]);
    let gate = latest("gate", &["a"]);
    assert_eq!(chosen(None, &[]), Err(Unchosen::Nothing));
    assert_eq!(chosen(Some("herald"), &[]), Err(Unchosen::Nothing));
    assert_eq!(chosen(None, &[&herald]), Ok(&herald));
    assert_eq!(
        chosen(None, &[&gate, &herald]),
        Err(Unchosen::Many(vec!["gate".to_owned(), "herald".to_owned()]))
    );
    assert_eq!(chosen(Some("HERALD"), &[&gate, &herald]), Ok(&herald));
    assert_eq!(
        chosen(Some("nope"), &[&gate, &herald]),
        Err(Unchosen::Unknown {
            named: "nope".to_owned(),
            known: vec!["gate".to_owned(), "herald".to_owned()],
        })
    );
}

#[test]
fn completions_hold_what_was_typed_those_starting_with_it_first_at_most_twenty_five() {
    let every_latest = [
        latest("php-mutation-gate", &["a"]),
        latest("gate", &["a"]),
        latest("herald", &["a"]),
    ];
    let announced: Vec<&Latest> = every_latest.iter().collect();
    assert_eq!(completions("GA", &announced), ["gate", "php-mutation-gate"]);
    assert_eq!(completions("", &announced).len(), 3);
    let many: Vec<Latest> = (0..30)
        .map(|n| latest(&format!("p{n:02}"), &["a"]))
        .collect();
    let many: Vec<&Latest> = many.iter().collect();
    let listed = completions("p", &many);
    assert_eq!(listed.len(), MOST_COMPLETIONS);
    assert_eq!(listed.first().map(String::as_str), Some("p00"));
}

#[test]
fn everything_herald_says_reads_as_a_sentence() {
    let project = || "herald".to_owned();
    let cases = [
        (
            Said::Subscribed(project()),
            "You will be mentioned when herald announces a release.",
        ),
        (
            Said::AlreadySubscribed(project()),
            "You are already mentioned when herald announces a release.",
        ),
        (
            Said::Unsubscribed(project()),
            "You will no longer be mentioned when herald announces a release.",
        ),
        (
            Said::NotSubscribed(project()),
            "You are not mentioned when herald announces a release.",
        ),
        (
            Said::Unoffered(project()),
            "Subscribing to herald is not offered here.",
        ),
        (
            Said::Unchosen(Unchosen::Nothing),
            "Nothing has been announced here yet.",
        ),
        (
            Said::Unchosen(Unchosen::Many(vec!["a".to_owned(), "b".to_owned()])),
            "More than one project is announced here; name one of a, b.",
        ),
        (
            Said::Unchosen(Unchosen::Unknown {
                named: "x".to_owned(),
                known: vec!["a".to_owned()],
            }),
            "x is not announced here; name one of a.",
        ),
        (
            Said::Disabled(Command::Latest),
            "The command latest is not answered here.",
        ),
        (
            Said::Unavailable,
            "herald could not answer just now; try again later.",
        ),
    ];
    for (said, sentence) in cases {
        assert_eq!(said.to_string(), sentence);
    }
}

#[test]
fn answering_takes_the_commands_it_names_each_once() {
    let answering = Answering {
        commands: vec![Command::Latest],
        replies: Replies::Public,
        prereleases: false,
        template: None,
    };
    assert!(answering.takes(Command::Latest));
    assert!(!answering.takes(Command::Subscribe));
    assert_eq!(every_command(), Command::ALL);
    assert_eq!(Answering::check(&Command::ALL), Ok(()));
    assert_eq!(
        Answering::check(&[]),
        Err("it answers no command; name at least one, or leave out the interactions".to_owned())
    );
    assert_eq!(
        Answering::check(&[Command::Latest, Command::Changelog, Command::Latest]),
        Err("it names latest twice; name it once".to_owned())
    );
}
