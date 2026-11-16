//! A Forgejo owner following its activity feed (ADR-0037).

use super::*;

/// The harness config with its source a Forgejo owner reading its feed.
fn fed() -> Config {
    let mut config = config("");
    if let Some(source) = config.sources.values_mut().next()
        && let Some(mut forged) = source.of::<ForgeSource>().cloned()
    {
        forged.forge = herald_sources_forge::Forge::Forgejo;
        forged.options.repository = None;
        forged.options.api = Some("https://codeberg.org/api/v1".to_owned());
        forged.options.owner = Some("nightworksio".to_owned());
        forged.options.transport = Some(herald_sources_forge::Transport::Feed);
        *source = Source::new(forged);
    }
    config
}

#[test]
fn a_forgejo_owner_following_its_feed_polls_only_the_members_it_names() {
    const LISTED: &str = "/orgs/nightworksio/repos?limit=50&page=1";
    const FEED: &str = "/orgs/nightworksio/activities/feeds?limit=50&page=1";
    let store = healthy();
    let config = fed();
    let two = json!([
        { "id": 1, "name": "herald", "full_name": "nightworksio/herald", "private": false },
        { "id": 2, "name": "gate", "full_name": "nightworksio/gate", "private": false },
    ])
    .to_string();
    let opening = Script::answering(&[
        (LISTED, 200, &two),
        (
            FEED,
            200,
            r#"[{"id":10,"op_type":"commit_repo","repo_id":1}]"#,
        ),
        ("/repos/nightworksio/herald/releases?limit=30", 200, "[]"),
        ("/repos/nightworksio/herald", 200, &described(1, "herald")),
        ("/repos/nightworksio/gate/releases?limit=30", 200, "[]"),
        ("/repos/nightworksio/gate", 200, &described(2, "gate")),
    ]);
    let swept = polled(&config, &store, &opening, NOON);
    assert_eq!(swept.len(), 3, "{swept:?}");
    assert_eq!(listing(&store).unwrap().events.unwrap().newest, 10);
    let named = Script::answering(&[
        (LISTED, 200, &two),
        (
            FEED,
            200,
            r#"[{"id":12,"op_type":"publish_release","repo_id":2},{"id":10,"op_type":"commit_repo","repo_id":1}]"#,
        ),
        ("/repos/nightworksio/gate/releases?limit=30", 200, "[]"),
    ]);
    let swept = polled(
        &config,
        &store,
        &named,
        NOON + SignedDuration::from_hours(1),
    );
    assert_eq!(
        swept
            .iter()
            .filter(|(_, outcome)| *outcome != Sweep::Unchanged)
            .count(),
        2,
        "{swept:?}"
    );
    assert_eq!(listing(&store).unwrap().events.unwrap().newest, 12);
}
