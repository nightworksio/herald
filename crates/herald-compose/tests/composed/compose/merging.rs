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

/// Whether a run reaches the merge queues' write side, and makes a
/// connection to GitHub's API through it.
struct Steps;

impl<H: Host> Through<H> for Steps {
    type Done = Option<bool>;

    fn through<S: herald_ports::Store, K: herald_ports::Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> impl Future<Output = Option<bool>> {
        ready(
            serving
                .merging
                .map(|handing| (handing.connect)(Base::At("https://api.github.com")).is_ok()),
        )
    }
}

#[test]
fn the_server_reaches_the_write_side_only_where_a_queue_s_steps_are_its_own() {
    let text = |extra: &str| {
        format!(
            "version = 1\n[store]\nkind = \"git-branch\"\nkey = {{ secret = \"HERALD_STORE_KEY\" }}\n[sources.gate]\nkind = \"github\"\nrepository = \"cli/cli\"\n[merge_queues.app]\nsource = \"gate\"\nbranch = \"main\"\n{extra}"
        )
    };
    let timer = herald_testing::FakeTimer::default();
    for (extra, reached) in [("", None), ("applied_by = \"server\"\n", Some(true))] {
        let config = load(
            "herald.toml",
            &text(extra),
            &|_| Err(String::new()),
            &herald_compose::kinds(),
        )
        .unwrap();
        let composed =
            pollster::block_on(Composed::new(github(), config, NODE, Uses::Serves, "test"))
                .unwrap();
        assert_eq!(pollster::block_on(composed.serve(Steps, &timer)), reached);
    }
}
