use jiff::SignedDuration;
use serde_json::json;

use super::*;

#[test]
fn a_first_listing_s_members_remember_their_tags_unread_and_a_member_that_joins_reads_them() {
    let store = healthy();
    let config = owned(|options| options.signals = Some(vec![herald_core::Signal::Tag]));
    let page = |id: u64, name: &str| {
        json!({"id": id, "name": name, "html_url": format!("https://github.com/nightworksio/{name}")})
            .to_string()
    };
    let (herald_page, gate_page) = (page(1, "herald"), page(2, "gate"));
    let first_round = Script::answering(&[
        (LISTING, 200, &repositories(&[(1, "herald")])),
        (
            "/repos/nightworksio/herald/tags?per_page=30",
            200,
            r#"[{"name":"v1"}]"#,
        ),
        ("/repos/nightworksio/herald", 200, &herald_page),
    ]);
    assert_eq!(
        polled(&config, &store, &first_round, NOON),
        [listed(1, &["herald"], &[]), member("herald", first(0))]
    );
    let tagged_before = json!({"committer": {"date": NOON.to_string()}}).to_string();
    let joined = NOON + SignedDuration::from_hours(1);
    let second_round = Script::answering(&[
        (LISTING, 200, &repositories(&[(1, "herald"), (2, "gate")])),
        (
            "/repos/nightworksio/herald/tags?per_page=30",
            200,
            r#"[{"name":"v1"}]"#,
        ),
        ("/repos/nightworksio/herald", 200, &herald_page),
        (
            "/repos/nightworksio/gate/tags?per_page=30",
            200,
            r#"[{"name":"v2"}]"#,
        ),
        ("/repos/nightworksio/gate", 200, &gate_page),
        (
            "/repos/nightworksio/gate/git/ref/tags/v2",
            200,
            r#"{"object":{"type":"commit","sha":"c2"}}"#,
        ),
        (
            "/repos/nightworksio/gate/git/commits/c2",
            200,
            &tagged_before,
        ),
    ]);
    assert_eq!(
        polled(&config, &store, &second_round, joined),
        [
            listed(2, &["gate"], &[]),
            member(
                "herald",
                Sweep::Polled {
                    first: false,
                    found: 0,
                    announced: Vec::new(),
                    cursor: Remembered::Advanced,
                }
            ),
            member("gate", first(1)),
        ]
    );
    assert!(second_round.answers.lock().unwrap().is_empty());
}
