use super::world::*;
use crate::*;

#[test]
fn doctor_names_what_the_config_leaves_to_chance() {
    let world = World::default();
    let derived = herald(&setup(CONFIG), &world, &["doctor", "--layer", "actions"]);
    assert_eq!(derived.code, DONE);
    assert!(
        derived
            .out
            .contains("reads, its names resolve and its templates render.")
    );
    assert!(
        derived
            .out
            .contains("Rotating that credential forgets every earlier marker")
    );
    let keyed = herald(
        &setup(&format!(
            "{CONFIG}[markers]\nkey = {{ secret = \"HERALD_MARKER_SECRET\" }}\n"
        )),
        &world,
        &["doctor"],
    );
    assert!(!keyed.out.contains("Rotating"), "{}", keyed.out);
    let sqlite = herald(
        &setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"sqlite\"")),
        &world,
        &["doctor"],
    );
    assert_eq!(sqlite.code, STOPPED);
    assert!(
        sqlite.out.ends_with("store.kind: keeping records in sqlite cannot be served: this herald keeps records in git-branch, markers, actions-cache or repo-variables; set [store] kind to one of them.\n"),
        "{}",
        sqlite.out
    );
}

#[test]
fn doctor_names_what_the_markers_store_cannot_serve() {
    let gaps = |config: &str| {
        let doctored = herald(&setup(config), &World::default(), &["doctor"]);
        assert_eq!(doctored.code, STOPPED, "{}", doctored.err);
        assert!(
            doctored
                .out
                .contains("The markers store has no compare-and-set")
        );
        doctored
            .out
            .lines()
            .filter(|line| line.contains("cannot be served"))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    };
    let markers = CONFIG.replace("kind = \"git-branch\"", MARKERS);
    assert_eq!(
        gaps(&markers.replace(
            "template = \"release\"\n",
            "template = \"release\"\npin = \"store\"\n"
        )),
        [
            "routes[0].pin: a pin the store remembers cannot be served: the markers store keeps no pins; set pin = \"markers\"."
        ]
    );
    let hooked = markers.replace(
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }",
        "webhook = { secret = \"HERALD_DISCORD_WEBHOOK\" }",
    );
    assert_eq!(
        gaps(&hooked),
        [
            "routes[0].destination: sending each announcement once cannot be served: the markers store reads announcements back from their posts, and a webhook cannot read back its posts; post through a bot to a text or announcement channel, or choose a store that keeps records."
        ]
    );
    let forum = markers.replace("channel = \"100\"", "channel = \"100\"\nforum = true");
    assert!(gaps(&forum)[0].contains("and a forum's posts are not read back"));
}
