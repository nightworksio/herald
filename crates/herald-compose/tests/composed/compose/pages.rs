//! The static signed log composed: a herald writes it where its config
//! names a directory alone.

use herald_flows::Publishing;

use super::*;

#[test]
fn a_composed_herald_writes_its_static_log() {
    let text = "version = 1\n[store]\nkind = \"sqlite\"\n[federation]\ndomain = \"owner.example\"\nkeys = [{ secret = \"HERALD_KEY\" }]\n[federation.pages]\nurl = \"https://owner.example/log\"\nartifact = \"_site\"\n";
    let config = load(
        "herald.toml",
        text,
        &|_| Err(String::new()),
        &herald_compose::kinds(),
    )
    .unwrap();
    let composed = pollster::block_on(Composed::dry(
        Hosting {
            secrets: FakeSecrets::default().with("HERALD_KEY", "new-federation-key-of-32-bytes!!"),
            ..Hosting::default()
        },
        config,
        Uses::Polls,
        "dry",
    ))
    .unwrap();
    let published = pollster::block_on(composed.publish());
    assert!(
        matches!(published, Ok(Some(Publishing::Written { .. }))),
        "{published:?}"
    );
}
