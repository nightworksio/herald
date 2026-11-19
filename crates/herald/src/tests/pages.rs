use super::peers::{NEWER, seeded};
use super::world::*;
use crate::*;

const HEAD: &str = "/repos/cli/cli/git/ref/heads/herald-pages";

fn logging(artifact: &str) -> String {
    format!(
        r#"{CONFIG}
[federation]
domain = "owner.example"
keys = [{{ secret = "HERALD_KEY" }}]

[federation.pages]
url = "https://owner.example/log"
branch = "herald-pages"
artifact = "{artifact}"

[destinations.feed]
kind = "herald"

[[routes]]
id = "fed"
sources = ["cli"]
destination = "feed"
"#
    )
}

fn keyed(config: &str) -> Setup {
    let mut setup = setup(config);
    setup
        .vars
        .insert("HERALD_KEY".to_owned(), seeded(NEWER).expose().to_owned());
    setup
}

fn headed(world: &World, status: u16) {
    world
        .state
        .lock()
        .unwrap()
        .forge
        .insert(HEAD.to_owned(), (status, "{}".to_owned()));
}

#[test]
fn a_poll_writes_the_static_log_to_its_branch_and_directory_and_then_leaves_it() {
    let site = tempfile::tempdir().unwrap();
    let setup = keyed(&logging(&site.path().display().to_string()));
    let world = World::default();
    forge(&world, (200, &["v1"]));
    headed(&world, 404);
    let first = herald(&setup, &world, &POLL);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (
            DONE,
            "cli: first poll found 1; 0 announced, as backfill asks\nThe static log was written under 1: 4 files, 4 written or removed on its branch.\n",
            ""
        )
    );
    for path in [
        ".well-known/herald",
        ".well-known/herald.sig",
        ".nojekyll",
        "herald/v1/feeds/feed",
    ] {
        assert!(site.path().join(path).is_file(), "{path}");
    }
    let seen = world.state.lock().unwrap().seen.clone();
    assert!(
        seen.iter()
            .any(|line| line == "POST https://api.github.com/repos/cli/cli/git/refs"),
        "{seen:?}"
    );
    forge(&world, (304, &[]));
    let later = herald(&setup, &world, &POLL);
    assert_eq!(
        (later.code, later.out.as_str()),
        (
            DONE,
            "cli: unchanged since the last poll\nThe static log stands at 1; no feed changed.\n"
        )
    );
}

#[test]
fn a_log_its_branch_or_directory_refuses_stops_the_run() {
    let blocked = tempfile::tempdir().unwrap();
    std::fs::write(blocked.path().join("file"), "").unwrap();
    let site = blocked
        .path()
        .join("file")
        .join("site")
        .display()
        .to_string();
    let setup = keyed(&logging(&site));
    let world = World::default();
    forge(&world, (200, &["v1"]));
    headed(&world, 404);
    let unfilled = herald(&setup, &world, &POLL);
    assert_eq!(unfilled.code, STOPPED);
    assert!(
        unfilled
            .out
            .contains(&format!("The static log was not written to {site}: ")),
        "{}",
        unfilled.out
    );
    let occupied = blocked.path().join("occupied");
    std::fs::create_dir_all(occupied.join(".nojekyll")).unwrap();
    let occupied = occupied.display().to_string();
    let setup = keyed(&logging(&occupied));
    let world = World::default();
    forge(&world, (200, &["v1"]));
    headed(&world, 404);
    let unwritten = herald(&setup, &world, &POLL);
    assert_eq!(unwritten.code, STOPPED);
    assert!(
        unwritten.out.contains(&format!(
            "The static log was not written to {occupied}: .nojekyll: "
        )),
        "{}",
        unwritten.out
    );
    let refused = keyed(&logging(&blocked.path().join("site").display().to_string()));
    let world = World::default();
    forge(&world, (200, &["v1"]));
    headed(&world, 500);
    let stopped = herald(&refused, &world, &POLL);
    assert_eq!(stopped.code, STOPPED);
    assert!(
        stopped
            .out
            .ends_with("The static log was not written: GitHub answered 500 for the branch's head of the static log\n"),
        "{}",
        stopped.out
    );
}
