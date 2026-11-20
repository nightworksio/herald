//! A living post's advisories section recalled for every report the store
//! names, whether or not the config still holds the report, its board or
//! its view, and the names written around the cursor so no such post is
//! ever lost (ADR-0067).

use herald_config::{Config, load};
use herald_core::{AdvisedPost, AdvisedReports, Cursor, DestinationName, RouteId, SourceName};
use herald_flows::recalls;
use herald_ports::{Cursors, Recalls, Store};

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
    let text = format!(
        "version = 1\n[store]\nkind = \"sqlite\"\n[destinations.discord]\nkind = \"discord\"\ntoken = {{ secret = \"HERALD_DISCORD_TOKEN\" }}\n[sources.open]\nkind = \"github\"\nrepository = \"o/open\"\ntoken = {{ secret = \"HERALD_GITHUB_TOKEN\" }}\n[views.plain]\nsources = [\"open\"]\n{routes}"
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
