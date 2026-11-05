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

fn names(announced: &[AnnouncedHere<'_, RouteId>]) -> Vec<String> {
    announced
        .iter()
        .map(|here| here.latest.project.clone())
        .collect()
}

fn every(every_latest: &[Latest]) -> Vec<AnnouncedHere<'_, RouteId>> {
    every_latest
        .iter()
        .map(|latest| AnnouncedHere {
            latest,
            route: latest.routes.first().unwrap(),
        })
        .collect()
}

fn same(id: &RouteId) -> &RouteId {
    id
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
    let routes = [RouteId::new("b"), RouteId::new("a")];
    let found = announced(&every_latest, &routes, same);
    assert_eq!(names(&found), ["alpha", "zeta"]);
    assert_eq!(
        found
            .iter()
            .map(|here| here.route.as_str())
            .collect::<Vec<_>>(),
        ["b", "a"]
    );
    assert_eq!(announced(&every_latest, &[], same), []);
}

#[test]
fn the_project_named_or_the_only_one_is_chosen() {
    let both = [latest("gate", &["a"]), latest("herald", &["a"])];
    let both = every(&both);
    let only = &both[1..];
    let project = |found: Result<AnnouncedHere<'_, RouteId>, Unchosen>| {
        found.map(|here| here.latest.project.clone())
    };
    assert_eq!(project(chosen(None, &[])), Err(Unchosen::Nothing));
    assert_eq!(project(chosen(Some("herald"), &[])), Err(Unchosen::Nothing));
    assert_eq!(project(chosen(None, only)), Ok("herald".to_owned()));
    assert_eq!(
        project(chosen(None, &both)),
        Err(Unchosen::Many(vec!["gate".to_owned(), "herald".to_owned()]))
    );
    assert_eq!(
        project(chosen(Some("HERALD"), &both)),
        Ok("herald".to_owned())
    );
    assert_eq!(
        project(chosen(Some("nope"), &both)),
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
    let announced = every(&every_latest);
    assert_eq!(completions("GA", &announced), ["gate", "php-mutation-gate"]);
    assert_eq!(completions("", &announced).len(), 3);
    let many: Vec<Latest> = (0..30)
        .map(|n| latest(&format!("p{n:02}"), &["a"]))
        .collect();
    let listed = completions("p", &every(&many));
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
fn every_name_herald_says_back_is_shown_as_written_not_read_as_markup() {
    let link = || "[Free](https://evil.example)".to_owned();
    let shown = r"\[Free\]\(https\:\/\/evil\.example\)";
    assert_eq!(
        Said::Unchosen(Unchosen::Unknown {
            named: link(),
            known: vec!["my_app".to_owned(), link()],
        })
        .to_string(),
        format!("{shown} is not announced here; name one of my\\_app, {shown}.")
    );
    assert_eq!(
        Said::Unchosen(Unchosen::Many(vec![link()])).to_string(),
        format!("More than one project is announced here; name one of {shown}.")
    );
    for said in [
        Said::Subscribed(link()),
        Said::AlreadySubscribed(link()),
        Said::Unsubscribed(link()),
        Said::NotSubscribed(link()),
        Said::Unoffered(link()),
    ] {
        let said = said.to_string();
        assert!(said.contains(shown) && !said.contains("[Free]"), "{said}");
    }
}

#[test]
fn a_place_is_where_a_route_announces_in_it_or_in_the_channel_its_thread_is_in() {
    let channel = Place::new("1");
    assert!(channel.is("1"));
    assert!(!channel.is("2"));
    let thread = Place {
        id: "3".to_owned(),
        parent: Some("1".to_owned()),
    };
    assert!(thread.is("3"));
    assert!(thread.is("1"));
    assert!(!thread.is("2"));
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
