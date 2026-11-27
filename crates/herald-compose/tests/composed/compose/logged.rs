//! What a composed herald's log writes on its host (ADR-0089).

use std::collections::BTreeMap;

use herald_compose::{Composed, LOG_LEVEL, Told};
use herald_config::Uses;
use herald_testing::FakeSecrets;

use super::{Hosting, NODE, asked, config};

#[test]
fn a_composed_herald_logs_at_the_level_its_host_names_scrubbing_each_secret_it_read() {
    let token = format!("ghs_{}", "Ab9".repeat(12));
    let host = |level: &str| Hosting {
        vars: BTreeMap::from([(LOG_LEVEL.to_owned(), level.to_owned())]),
        secrets: FakeSecrets::default().with("HERALD_ROUND_SECRET", &token),
        ..Hosting::default()
    };
    let composed = pollster::block_on(Composed::new(
        host("warn"),
        config("kind = \"deno-kv\""),
        NODE,
        Uses::Serves,
        "test",
    ))
    .unwrap();
    let round = asked(
        "POST",
        "/herald/round/poll",
        Some(&format!("Bearer {token}")),
    );
    assert_eq!(
        pollster::block_on(composed.reached(&round)).reply.status,
        200
    );
    assert_eq!(
        composed
            .log()
            .lines([Told::info("polled"), Told::warn(format!("echoed {token}")),]),
        ["echoed «secret:HERALD_ROUND_SECRET»"]
    );
    let refused = pollster::block_on(Composed::new(
        host("loud"),
        config("kind = \"deno-kv\""),
        NODE,
        Uses::Serves,
        "test",
    ));
    assert_eq!(
        refused.err(),
        Some("HERALD_LOG_LEVEL is \"loud\"; it takes debug, info, warn, error".to_owned())
    );
}
