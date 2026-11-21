//! A living post's advisories section recalled for every report the store
//! names, whether or not the config still holds the report, its board or
//! its view, and the names written around the cursor so no such post is
//! ever lost (ADR-0067).

use herald_config::{Config, load};
use herald_core::{
    AdvisedPost, AdvisedReports, Cursor, DestinationName, Entry, Identity, Key, RouteId, SourceName,
};
use herald_flows::{Outcome, recalls};
use herald_ports::{Cursors, Ledger, Recalls, Store};

use crate::advised::{boarded, minute, recalled};
use crate::advisories::{
    ALERT_PAGE, CHANNEL_READ, EDITED, GUILD_READ, ONBOARDING_READ, WITHHELD, alerts, answers,
    board, page, posted, round,
};
use crate::harness::{Failing, Fault, NOON, POSTED, Script, healthy};

const RECALLED: &str = "removed because this route no longer receives it";

/// The names the store keeps of the reports whose living posts were sent
/// advisories.
fn named(store: &Failing) -> Vec<String> {
    pollster::block_on(store.recalls().advised())
        .unwrap()
        .map(|named| named.value.reports.into_iter().collect())
        .unwrap_or_default()
}

/// A route announcing the source's releases on the board's channel, no
/// report's: what a later round reaches the destination through.
const LOBBY: &str = "[[routes]]\nid = \"lobby\"\nsources = [\"open\"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"released\"\n[templates.released]\ntitle = \"{{ event.title }}\"\nbody = \"{{ event.body }}\"\n";

/// `routes` with no view showing advisories and no report: a view `plain`
/// alone.
fn plain(routes: &str) -> Config {
    plain_with("", routes)
}

/// `routes` with no view showing advisories and no report, the
/// destination set as `destination` also says.
fn plain_with(destination: &str, routes: &str) -> Config {
    let text = format!(
        "version = 1\n[store]\nkind = \"sqlite\"\n[destinations.discord]\nkind = \"discord\"\ntoken = {{ secret = \"HERALD_DISCORD_TOKEN\" }}\n{destination}\n[sources.open]\nkind = \"github\"\nrepository = \"o/open\"\ntoken = {{ secret = \"HERALD_GITHUB_TOKEN\" }}\n[views.plain]\nsources = [\"open\"]\n{routes}"
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

#[test]
fn a_living_post_of_a_removed_board_report_or_view_is_still_edited_to_the_fixed_line() {
    for later in [crate::advisories::config(LOBBY), plain(LOBBY)] {
        let store = healthy();
        boarded(&store);
        assert_eq!(named(&store), ["live"]);
        let edited = Script::answering(&[EDITED]);
        let swept = round(&later, (&store, &edited), (minute(), None));
        assert_eq!(edited.seen().len(), 1, "{:?}", edited.seen());
        let edit = posted(&edited).pop().unwrap();
        assert!(
            edit.contains(RECALLED) && !edit.contains(ALERT_PAGE),
            "{edit}"
        );
        assert_eq!(recalled(&swept).len(), 1, "{swept:?}");
        assert_eq!(named(&store), Vec::<String>::new());
        let found = pollster::block_on(recalls(
            &later,
            (&store.cursors(), &store.recalls(), &store.ledger()),
        ));
        assert_eq!(found.mistakes, Vec::<String>::new());
    }
}

#[test]
fn a_renamed_board_s_old_living_post_is_edited_and_the_new_one_posted() {
    let store = healthy();
    boarded(&store);
    let renamed = crate::advisories::config(&board(true).replace("boards.live", "boards.fresh"));
    let (page, open) = (page(), alerts(true));
    let mut then = answers((&page, &open), &[POSTED]);
    then.insert(4, EDITED);
    let script = Script::answering(&then);
    round(&renamed, (&store, &script), (minute(), None));
    let bodies = posted(&script);
    assert_eq!(bodies.len(), 2, "{:?}", script.seen());
    assert!(bodies[0].contains(RECALLED), "{}", bodies[0]);
    assert!(bodies[1].contains(ALERT_PAGE), "{}", bodies[1]);
    assert_eq!(named(&store), ["fresh"]);
}

#[test]
fn a_removed_board_whose_post_cannot_be_reached_stays_named_and_doctor_says_so() {
    let store = healthy();
    boarded(&store);
    let none = Script::answering(&[]);
    let unreached = plain("");
    let swept = round(&unreached, (&store, &none), (minute(), None));
    assert_eq!(none.seen(), Vec::<String>::new());
    assert_eq!(recalled(&swept).len(), 1, "{swept:?}");
    assert_eq!(named(&store), ["live"]);
    let found = pollster::block_on(recalls(
        &unreached,
        (&store.cursors(), &store.recalls(), &store.ledger()),
    ));
    assert_eq!(found.mistakes.len(), 1, "{found:?}");
    assert!(
        found.mistakes[0].contains("report live on the route live"),
        "{found:?}"
    );
}

#[test]
fn the_names_are_written_before_the_cursor_and_a_name_whose_cursor_keeps_none_is_dropped() {
    for fault in [
        Fault::WritesRecalls,
        Fault::RecallsConflict,
        Fault::KeepsNoRecalls,
    ] {
        let store = Failing { fault, ..healthy() };
        let (page, open) = (page(), alerts(true));
        let script = Script::answering(&answers((&page, &open), &[POSTED]));
        round(
            &crate::advisories::config(&board(true)),
            (&store, &script),
            (NOON, None),
        );
        let body = posted(&script).pop().unwrap();
        assert!(
            body.contains(WITHHELD) && !body.contains(ALERT_PAGE),
            "{body}"
        );
        let cursor = pollster::block_on(store.cursors().cursor(&SourceName::new("live")));
        let advised = cursor.unwrap().map(|cursor| cursor.value.advised);
        assert_eq!(advised.unwrap_or_default(), []);
    }
    let mut store = Failing {
        fault: Fault::WritesCursors,
        ..healthy()
    };
    let (page, open) = (page(), alerts(true));
    let script = Script::answering(&answers((&page, &open), &[POSTED]));
    round(
        &crate::advisories::config(&board(true)),
        (&store, &script),
        (NOON, None),
    );
    assert!(posted(&script).pop().unwrap().contains(WITHHELD));
    assert_eq!(named(&store), ["live"]);
    store.fault = Fault::Nothing;
    let quiet = Script::answering(&[]);
    round(&plain(""), (&store, &quiet), (minute(), None));
    assert_eq!(quiet.seen(), Vec::<String>::new());
    assert_eq!(named(&store), Vec::<String>::new());
}

#[test]
fn the_names_unread_or_past_their_most_withhold_the_section_and_doctor_names_them_unread() {
    let store = Failing {
        fault: Fault::ReadsRecalls,
        ..healthy()
    };
    let found = pollster::block_on(recalls(
        &crate::advisories::config(&board(true)),
        (&store.cursors(), &store.recalls(), &store.ledger()),
    ));
    assert!(found.mistakes[0].contains("could not be read"), "{found:?}");
    let full = healthy();
    let most = AdvisedReports {
        reports: (0..herald_core::MOST_ADVISED_REPORTS)
            .map(|index| format!("report{index}"))
            .collect(),
    };
    let kept = Cursor {
        advised: vec![AdvisedPost {
            route: RouteId::new("gone"),
            destination: DestinationName::new("discord"),
            placed: r#"{"channel":"1566200000000000100"}"#.to_owned(),
        }],
        ..Cursor::default()
    };
    for name in &most.reports {
        let source = SourceName::new(name);
        pollster::block_on(full.cursors().put_cursor(&source, &kept, None)).unwrap();
    }
    pollster::block_on(full.recalls().put_advised(&most, None)).unwrap();
    let (page, open) = (page(), alerts(true));
    let script = Script::answering(&answers((&page, &open), &[POSTED]));
    round(
        &crate::advisories::config(&board(true)),
        (&full, &script),
        (NOON, None),
    );
    let body = posted(&script).pop().unwrap();
    assert!(body.contains(WITHHELD), "{body}");
    let names = named(&full);
    assert_eq!(names.len(), herald_core::MOST_ADVISED_REPORTS);
    assert!(!names.iter().any(|name| name == "live"), "{names:?}");
}

#[test]
fn a_kept_place_missing_an_option_its_kind_now_writes_with_a_default_has_not_moved() {
    let store = healthy();
    boarded(&store);
    let source = SourceName::new("live");
    let record = pollster::block_on(store.cursors().cursor(&source))
        .unwrap()
        .unwrap();
    let older = Cursor {
        advised: vec![AdvisedPost {
            route: RouteId::new("live"),
            destination: DestinationName::new("discord"),
            placed: r#"{"channel":"1566200000000000100"}"#.to_owned(),
        }],
        ..record.value.clone()
    };
    pollster::block_on(
        store
            .cursors()
            .put_cursor(&source, &older, Some(record.version)),
    )
    .unwrap();
    let quiet = Script::answering(&[CHANNEL_READ, GUILD_READ, ONBOARDING_READ]);
    round(
        &crate::advisories::config(&board(true)),
        (&store, &quiet),
        (minute(), None),
    );
    assert_eq!(
        quiet.seen(),
        [CHANNEL_READ.0, GUILD_READ.0, ONBOARDING_READ.0]
            .map(|path| format!("GET https://discord.com/api/v10{path}"))
    );
}

/// The recall's outcomes and the names still kept after a round of
/// `later` over a store `boarded` posted to, `fault` set on it, `script`
/// answering.
fn recalled_with(later: &Config, fault: Fault, script: &Script) -> (Vec<Outcome>, Vec<String>) {
    let mut store = healthy();
    boarded(&store);
    store.fault = fault;
    *store.landings.lock().unwrap() = 0;
    let swept = round(later, (&store, script), (minute(), None));
    let outcomes = recalled(&swept).into_iter().cloned().collect();
    store.fault = Fault::Nothing;
    (outcomes, named(&store))
}

#[test]
fn a_recall_whose_ledger_entry_cannot_be_read_before_or_after_is_kept_for_the_next_round() {
    let none = Script::answering(&[]);
    let (outcomes, names) = recalled_with(&plain(LOBBY), Fault::Reads, &none);
    assert_eq!(outcomes, [Outcome::Sealed { stopped: true }]);
    assert_eq!(names, ["live"]);
    assert_eq!(none.seen(), Vec::<String>::new());
    let unread = Script::answering(&[]);
    let (outcomes, names) = recalled_with(&plain(LOBBY), Fault::ReadsOnce, &unread);
    assert_eq!(outcomes, [Outcome::Sealed { stopped: true }]);
    assert_eq!(names, ["live"]);
    let cursorless = Script::answering(&[]);
    let (outcomes, names) = recalled_with(&plain(LOBBY), Fault::Cursors, &cursorless);
    assert_eq!(outcomes, Vec::<Outcome>::new());
    assert_eq!(names, ["live"]);
    assert_eq!(cursorless.seen(), Vec::<String>::new());
}

#[test]
fn a_recall_is_sent_to_the_place_herald_kept_not_the_place_of_the_route_it_borrows() {
    let elsewhere = LOBBY.replace("1566200000000000100", "1566200000000000200");
    let edited = Script::answering(&[EDITED]);
    let (outcomes, names) = recalled_with(&plain(&elsewhere), Fault::Nothing, &edited);
    assert_eq!(
        edited.seen(),
        ["PATCH https://discord.com/api/v10/channels/1566200000000000100/messages/1"]
    );
    assert!(
        matches!(outcomes[..], [Outcome::Sealed { stopped: false }]),
        "{outcomes:?}"
    );
    assert_eq!(names, Vec::<String>::new());
}

#[test]
fn a_report_named_by_no_title_is_never_edited_and_stays_named() {
    let store = healthy();
    boarded(&store);
    let live = SourceName::new("live");
    let blank = SourceName::new(" ");
    let cursor = pollster::block_on(store.cursors().cursor(&live))
        .unwrap()
        .unwrap()
        .value;
    pollster::block_on(store.cursors().put_cursor(&blank, &cursor, None)).unwrap();
    for post in &cursor.advised {
        let identity = |name: &str| Identity::new(herald_core::living_identity(name));
        let kept = pollster::block_on(
            store
                .ledger()
                .entry(&Key::derive(&post.route, &identity("live"))),
        )
        .unwrap()
        .unwrap()
        .value;
        let moved = Entry {
            key: Key::derive(&post.route, &identity(" ")),
            ..kept
        };
        pollster::block_on(store.ledger().put_entry(&moved, None)).unwrap();
    }
    let both = AdvisedReports {
        reports: ["live".to_owned(), " ".to_owned()].into_iter().collect(),
    };
    let held = pollster::block_on(store.recalls().advised()).unwrap();
    pollster::block_on(
        store
            .recalls()
            .put_advised(&both, held.map(|held| held.version)),
    )
    .unwrap();
    let edited = Script::answering(&[EDITED]);
    round(&plain(LOBBY), (&store, &edited), (minute(), None));
    assert_eq!(edited.seen().len(), 1, "{:?}", edited.seen());
    assert_eq!(named(&store), [" "]);
}

#[test]
fn a_recall_through_a_destination_that_no_longer_edits_is_skipped_and_kept() {
    let store = healthy();
    let kept = Cursor {
        advised: vec![AdvisedPost {
            route: RouteId::new("gone"),
            destination: DestinationName::new("chat"),
            placed: r#"{"channel":"C0123456789"}"#.to_owned(),
        }],
        ..Cursor::default()
    };
    pollster::block_on(
        store
            .cursors()
            .put_cursor(&SourceName::new("live"), &kept, None),
    )
    .unwrap();
    let named_live = AdvisedReports {
        reports: ["live".to_owned()].into_iter().collect(),
    };
    pollster::block_on(store.recalls().put_advised(&named_live, None)).unwrap();
    let text = "version = 1\n[store]\nkind = \"sqlite\"\n[destinations.chat]\nkind = \"slack\"\nwebhook = { secret = \"HERALD_SLACK_WEBHOOK\" }\n[sources.open]\nkind = \"github\"\nrepository = \"o/open\"\ntoken = { secret = \"HERALD_GITHUB_TOKEN\" }\n[[routes]]\nid = \"lobby\"\nsources = [\"open\"]\ndestination = \"chat\"\ntemplate = \"released\"\n[templates.released]\ntitle = \"{{ event.title }}\"\nbody = \"{{ event.body }}\"\n";
    let config = load(
        "herald.toml",
        text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let secrets = herald_testing::FakeSecrets::default()
        .with(
            "HERALD_SLACK_WEBHOOK",
            "https://hooks.slack.com/services/T0/B0/hook",
        )
        .with("HERALD_GITHUB_TOKEN", "ghp_x");
    let script = Script::answering(&[]);
    let prepared = pollster::block_on(herald_flows::prepare(
        &config,
        &secrets,
        &herald_testing::FakeSigner,
        herald_config::Uses::Polls,
    ))
    .unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = herald_testing::FakeClock::at(NOON);
    let ports = herald_flows::Ports {
        store: &store,
        clock: &clock,
        signer: &herald_testing::FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let swept = pollster::block_on(herald_flows::report(&config, &connected, &ports, "r1"));
    let outcomes = recalled(&swept);
    assert!(
        matches!(outcomes[..], [Outcome::Skipped(_)]),
        "{outcomes:?}"
    );
    assert_eq!(script.seen(), Vec::<String>::new());
    assert_eq!(named(&store), ["live"]);
}

#[test]
fn recall_s_text_holds_no_symbol() {
    let symbols = ['✔', '☐', '≈', '»'];
    let set = plain_with(
        "symbols = { done = \"✔\", todo = \"☐\", rule = \"≈\", bullet = \"»\" }",
        LOBBY,
    );
    let edited = Script::answering(&[EDITED]);
    let (outcomes, _) = recalled_with(&set, Fault::Nothing, &edited);
    assert!(
        matches!(outcomes[..], [Outcome::Sealed { stopped: false }]),
        "{outcomes:?}"
    );
    let edit = posted(&edited).pop().unwrap();
    assert!(edit.contains(RECALLED), "{edit}");
    assert!(
        !edit.chars().any(|shown| symbols.contains(&shown)),
        "{edit}"
    );
}
