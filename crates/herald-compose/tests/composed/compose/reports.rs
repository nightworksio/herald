//! Reports and views composed: a report round runs where no poll does, and
//! a view is read through the composed sources.

use super::*;

fn reporting() -> Config {
    let text = "version = 1\n[store]\nkind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }\n[sources.gate]\nkind = \"github\"\nrepository = \"cli/cli\"\n[reports.daily]\n";
    load(
        "herald.toml",
        text,
        &|_| Err(String::new()),
        &herald_compose::kinds(),
    )
    .unwrap()
}

#[test]
fn a_report_round_runs_where_no_poll_runs_and_says_where_it_stopped() {
    let composed = pollster::block_on(Composed::new(
        github(),
        reporting(),
        NODE,
        Uses::Serves,
        "test",
    ))
    .unwrap();
    let (polled, unreported) = pollster::block_on(futures_util::future::join(
        composed.poll(),
        composed.report(),
    ));
    assert_eq!(unreported, Vec::new());
    assert!(
        polled.iter().any(|swept| swept.source.as_str() == "daily"),
        "{polled:?}"
    );
    let reported = pollster::block_on(composed.report());
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0].source.as_str(), "daily");
    assert!(
        matches!(reported[0].outcome, Sweep::Stopped(Stop::Store(_))),
        "{reported:?}"
    );
}

#[test]
fn a_view_is_read_through_the_composed_sources() {
    let composed = pollster::block_on(Composed::new(
        github(),
        reporting(),
        NODE,
        Uses::Serves,
        "test",
    ))
    .unwrap();
    let read = pollster::block_on(composed.queue("queue", herald_core::Printed::Configured));
    assert!(matches!(read, Err(Stop::Source(_))), "{read:?}");
}

#[test]
fn orphans_are_listed_through_the_composed_store_and_one_it_cannot_read_lists_none() {
    let composed = pollster::block_on(Composed::new(
        github(),
        reporting(),
        NODE,
        Uses::Serves,
        "test",
    ))
    .unwrap();
    let listed = pollster::block_on(composed.orphans("daily", None));
    assert!(listed.is_err(), "{listed:?}");
}
