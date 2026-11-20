//! `herald rerun` composed: a rule's plans read and its jobs applied
//! through the composed sources and store.

use super::*;

fn planning() -> Config {
    let text = "version = 1\n[store]\nkind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }\n[sources.gate]\nkind = \"github\"\nrepository = \"cli/cli\"\n[views.mine]\nsources = [\"gate\"]\n[rules.flaky]\nview = \"mine\"\nflaky = { rerun = true }\nevery = \"1h\"\nat = \"00:00\"\n";
    load(
        "herald.toml",
        text,
        &|_| Err(String::new()),
        &herald_compose::kinds(),
    )
    .unwrap()
}

#[test]
fn a_rule_s_plans_and_an_apply_are_read_through_the_composed_herald() {
    let composed = pollster::block_on(Composed::new(
        github(),
        planning(),
        NODE,
        Uses::Polls,
        "test",
    ))
    .unwrap();
    let unknown = pollster::block_on(composed.rerun_plans(Some("nope")));
    assert!(
        matches!(unknown, Err(Stop::Source(ref why)) if why.contains("no rule is named \"nope\"")),
        "{unknown:?}"
    );
    let chosen = [("420".to_owned(), "h1".to_owned())];
    let applied = pollster::block_on(composed.rerun_apply(
        (Some("nope"), &chosen),
        &FakeTimer::default(),
        &|_| {},
    ));
    assert!(applied.is_err(), "{applied:?}");
    let unchosen = pollster::block_on(composed.rerun_apply(
        (Some("flaky"), &[]),
        &FakeTimer::default(),
        &|_| {},
    ));
    assert_eq!(
        unchosen,
        Err(Stop::Source(herald_flows::UNCHOSEN.to_owned()))
    );
}
