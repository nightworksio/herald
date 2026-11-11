//! A destination naming an option its kind lacks is refused, naming it.

mod common;

use herald_config::load;

#[test]
fn a_destination_naming_an_option_its_kind_lacks_is_refused_by_name() {
    let nothing = |_: &str| Err("no file".to_owned());
    let kinds = crate::common::kinds();
    let refused: Vec<(&str, String)> = kinds
        .destination_kinds()
        .into_iter()
        .map(|kind| {
            let text = format!(
                "version = 1\n\n[store]\nkind = \"sqlite\"\n\n[destinations.team]\nkind = \"{kind}\"\nbogus = 1\n"
            );
            let error = load("herald.toml", &text, &nothing, &kinds)
                .unwrap_err()
                .to_string();
            (kind, error)
        })
        .collect();
    let unknown = |kind, expected| {
        (
            kind,
            format!(
                "herald.toml:8:1 destinations.team.bogus: unknown field `bogus`, expected one of {expected}"
            ),
        )
    };
    assert_eq!(
        refused,
        [
            unknown(
                "discord",
                "`token`, `webhook`, `api`, `interactions`, `read_back`, `pin_lookups`, `colours`, `archive_after`, `symbols`, `waits`"
            ),
            unknown(
                "mastodon",
                "`server`, `token`, `read_back`, `symbols`, `waits`"
            ),
            unknown(
                "matrix",
                "`homeserver`, `token`, `interactions`, `read_back`, `pin_lookups`, `symbols`, `waits`"
            ),
            unknown(
                "slack",
                "`token`, `webhook`, `api`, `domain`, `team`, `interactions`, `read_back`, `pin_lookups`, `symbols`, `waits`"
            ),
            unknown(
                "activitypub",
                "`domain`, `key`, `symbols`, `waits`, `outbox_page`, `deliveries`, `mode`, `block`, `allow`, `followers`, `approve`, `lists`"
            ),
        ]
    );
}
