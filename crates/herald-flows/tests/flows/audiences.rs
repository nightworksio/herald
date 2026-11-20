//! Who may read a private route's place, checked once a run before an
//! advisories section is first sent there, and the names of the reports
//! whose living posts were sent one kept before each send (ADR-0067).

use std::collections::BTreeMap;

use herald_config::{Config, load};
use herald_core::AdvisedReports;
use herald_ports::{Recalls, Response, Store};
use jiff::SignedDuration;

use crate::advised::boarded;
use crate::advisories::{
    ALERT_PAGE, CHANNEL_READ, CODE, DEPENDABOT, EDITED, GRAPHQL, GUILD_READ, ONBOARDING_READ,
    SECRETS, WITHHELD, alerts, answers, board, page, posted, reported, round, routed,
};
use crate::harness::{Failing, Fault, NOON, POSTED, Script, healthy};

/// Discord's answer for a channel whose server's `@everyone` may view it.
const VIEWED: (&str, u16, &str) = (
    "/channels/1566200000000000100",
    200,
    r#"{"id":"1566200000000000100","type":0,"guild_id":"1566200000000000900","permission_overwrites":[]}"#,
);

fn listings(open: &str) -> [(&str, u16, &str); 3] {
    [
        (CODE, 200, "[]"),
        (DEPENDABOT, 200, open),
        (SECRETS, 200, "[]"),
    ]
}

#[test]
fn a_private_route_whose_channel_everyone_may_view_is_sent_the_line_and_nothing_is_read() {
    let config = crate::advisories::config(&format!(
        "{}{}",
        reported("daily"),
        routed("daily", "team", true)
    ));
    let page = page();
    let script = Script::answering(&[(GRAPHQL, 200, &page), VIEWED, GUILD_READ, POSTED]);
    round(&config, (&healthy(), &script), (NOON, None));
    let body = posted(&script).pop().unwrap();
    assert!(body.contains(WITHHELD), "{body}");
    assert!(
        script.seen().iter().all(|line| !line.contains("/alerts?")),
        "{:?}",
        script.seen()
    );
}

#[test]
fn one_place_is_checked_once_a_run_however_many_reports_send_there() {
    let config = crate::advisories::config(&format!(
        "{}{}{}{}",
        reported("a"),
        reported("b"),
        routed("a", "one", true),
        routed("b", "two", true)
    ));
    let (page, open) = (page(), alerts(true));
    let api = "https://discord.com/api/v10";
    let mut held = BTreeMap::new();
    let mut answer = |url: String, (_, status, body): (&str, u16, &str)| {
        held.insert(
            url,
            Response {
                status,
                headers: Vec::new(),
                body: body.as_bytes().to_vec(),
            },
        );
    };
    answer(GRAPHQL.to_owned(), (GRAPHQL, 200, &page));
    answer(format!("{api}{}", CHANNEL_READ.0), CHANNEL_READ);
    answer(format!("{api}{}", GUILD_READ.0), GUILD_READ);
    answer(format!("{api}{}", ONBOARDING_READ.0), ONBOARDING_READ);
    answer(format!("{api}{}", POSTED.0), POSTED);
    for listing in listings(&open) {
        answer(format!("https://api.github.com{}", listing.0), listing);
    }
    let script = Script::keyed(held);
    round(&config, (&healthy(), &script), (NOON, None));
    let checked = |path: &str| {
        script
            .seen()
            .iter()
            .filter(|line| line.ends_with(path))
            .count()
    };
    assert_eq!(
        (
            checked(CHANNEL_READ.0),
            checked(GUILD_READ.0),
            checked(ONBOARDING_READ.0)
        ),
        (1, 1, 1),
        "{:?}",
        script.seen()
    );
    let bodies = posted(&script);
    assert_eq!(bodies.len(), 2, "{:?}", script.seen());
    assert!(
        bodies.iter().all(|body| body.contains(ALERT_PAGE)),
        "{bodies:?}"
    );
}

/// The config of a report kept as a living post in a Discord webhook's
/// channel, which says `private = true` and `vouch` besides.
fn hooked(vouch: &str) -> Config {
    let text = format!(
        "version = 1\n[store]\nkind = \"sqlite\"\n[destinations.hooked]\nkind = \"discord\"\nwebhook = {{ secret = \"HERALD_DISCORD_WEBHOOK\" }}\n[sources.open]\nkind = \"github\"\nrepository = \"o/open\"\ntoken = {{ secret = \"HERALD_GITHUB_TOKEN\" }}\n[views.security]\nsources = [\"open\"]\noverviews = [\"advisories\"]\n{}[[routes]]\nid = \"hook\"\nsources = [\"daily\"]\ndestination = \"hooked\"\ntemplate = \"herald:queue\"\nprivate = true\nliving = true\n{vouch}\n",
        reported("daily")
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
fn a_place_herald_cannot_check_is_sent_the_section_only_on_the_operator_s_word() {
    let (page, open) = (page(), alerts(true));
    let sent = ("", 200, POSTED.2);
    let unvouched = Script::answering(&[(GRAPHQL, 200, &page), sent]);
    round(&hooked(""), (&healthy(), &unvouched), (NOON, None));
    let body = posted(&unvouched).pop().unwrap();
    assert!(body.contains(WITHHELD), "{body}");
    let mut read = vec![(GRAPHQL, 200, page.as_str())];
    read.extend(listings(&open));
    read.push(sent);
    let vouched = Script::answering(&read);
    round(
        &hooked("vouch_unchecked = true"),
        (&healthy(), &vouched),
        (NOON, None),
    );
    let body = posted(&vouched).pop().unwrap();
    assert!(body.contains(ALERT_PAGE), "{body}");
}

/// The names `store` keeps of the reports whose living posts were sent a
/// section, dropped as a run that overlapped another may leave them.
fn dropped(store: &Failing) {
    let recalls = store.recalls();
    let held = pollster::block_on(recalls.advised()).unwrap().unwrap();
    assert!(held.value.reports.contains("live"));
    let none = AdvisedReports::default();
    pollster::block_on(recalls.put_advised(&none, Some(held.version))).unwrap();
}

#[test]
fn a_living_post_is_sent_its_section_again_only_once_its_report_is_named_again() {
    let config = crate::advisories::config(&board(true));
    let mut store = healthy();
    boarded(&store);
    dropped(&store);
    let hour = NOON.checked_add(SignedDuration::from_hours(1)).unwrap();
    let (page, open) = (page(), alerts(true).replace("dependabot/3", "dependabot/4"));
    let again = Script::answering(&answers((&page, &open), &[EDITED]));
    round(&config, (&store, &again), (hour, None));
    let edit = posted(&again).pop().unwrap_or_default();
    assert!(edit.contains("dependabot/4"), "{edit}\n{:?}", again.seen());
    let named = pollster::block_on(store.recalls().advised())
        .unwrap()
        .unwrap();
    assert!(named.value.reports.contains("live"));
    dropped(&store);
    store.fault = Fault::WritesRecalls;
    let later = hour.checked_add(SignedDuration::from_hours(1)).unwrap();
    let withheld = Script::answering(&answers((&page, &open), &[EDITED]));
    round(&config, (&store, &withheld), (later, None));
    let edit = posted(&withheld).pop().unwrap_or_default();
    assert!(edit.contains(WITHHELD), "{edit}\n{:?}", withheld.seen());
    assert!(!edit.contains("dependabot/4"), "{edit}");
}
