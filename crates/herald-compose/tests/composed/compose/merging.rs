//! `herald merge-queue` composed: each queue read through the composed
//! sources.

use super::*;

#[test]
fn each_merge_queue_is_read_and_applied_through_the_composed_herald() {
    let text = "version = 1\n[store]\nkind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }\n[sources.gate]\nkind = \"github\"\nrepository = \"cli/cli\"\n[merge_queues.app]\nsource = \"gate\"\nbranch = \"main\"\n";
    let config = load(
        "herald.toml",
        text,
        &|_| Err(String::new()),
        &herald_compose::kinds(),
    )
    .unwrap();
    let composed =
        pollster::block_on(Composed::new(github(), config, NODE, Uses::Polls, "test")).unwrap();
    let read = pollster::block_on(composed.merge_queues((None, herald_core::Printed::Configured)))
        .unwrap();
    assert_eq!(
        read.iter()
            .map(|queue| (queue.name.as_str(), queue.repository.as_str()))
            .collect::<Vec<_>>(),
        [("app", "cli/cli")]
    );
    let applied = pollster::block_on(composed.merge_apply(
        (None, herald_core::Printed::Configured),
        &herald_testing::FakeTimer::default(),
    ))
    .unwrap();
    assert_eq!(
        applied
            .iter()
            .map(|queue| (queue.name.as_str(), queue.repository.as_str()))
            .collect::<Vec<_>>(),
        [("app", "cli/cli")]
    );
}
