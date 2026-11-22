//! What doctor finds online in the CODEOWNERS file of each source reading
//! areas from one (ADR-0073), and a file the store would keep no record
//! of (ADR-0076).

use herald_config::Uses;
use herald_flows::{codeowners, prepare};
use herald_testing::FakeSigner;

use super::loaded;
use crate::harness::{Script, secrets};

#[test]
fn doctor_reads_a_repository_source_s_file_and_says_an_owner_source_s_are_read_by_views() {
    let config = loaded(
        r#"version = 1
[store]
kind = "sqlite"
[sources.owned]
kind = "github"
owner = "acme"
areas = "codeowners"
token = { secret = "HERALD_FORGE_TOKEN" }
[sources.plain]
kind = "github"
repository = "p/q"
token = { secret = "HERALD_FORGE_TOKEN" }
[sources.listed]
kind = "github"
owner = "other"
token = { secret = "HERALD_FORGE_TOKEN" }
[sources.single]
kind = "github"
repository = "o/r"
areas = "codeowners"
token = { secret = "HERALD_FORGE_TOKEN" }
[sources.absent]
kind = "github"
repository = "a/b"
areas = "codeowners"
token = { secret = "HERALD_FORGE_TOKEN" }
"#,
    );
    let script = Script::answering(&[
        ("/repos/a/b/contents/.github/CODEOWNERS", 404, ""),
        ("/repos/a/b/contents/CODEOWNERS", 404, ""),
        ("/repos/a/b/contents/docs/CODEOWNERS", 404, ""),
        (
            "/repos/o/r/contents/.github/CODEOWNERS",
            200,
            "* @org/web\n[\n",
        ),
    ]);
    let secrets = secrets().with("HERALD_FORGE_TOKEN", "t");
    let prepared =
        pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let found = pollster::block_on(codeowners(&config, &connected, jiff::Timestamp::UNIX_EPOCH));
    assert_eq!(found.notes.len(), 4, "{:?}", found.notes);
    assert!(
        found.notes[0].starts_with("The source absent finds no CODEOWNERS file at "),
        "{}",
        found.notes[0]
    );
    assert_eq!(
        found.notes[1],
        "The source owned reads an owner's repositories, so doctor reads none of their CODEOWNERS files: a line one skips leaves its owners out of that repository's areas."
    );
    assert!(
        found.notes[2].starts_with(
            "The source single skips line 2 of its CODEOWNERS file .github/CODEOWNERS, which matches no path: "
        ),
        "{}",
        found.notes[2]
    );
    assert_eq!(
        found.notes[3],
        "The source single's forge gives its CODEOWNERS file .github/CODEOWNERS no validator, so the store keeps no record of it and it is read in full every round."
    );
    assert_eq!(script.seen().len(), 4);
}

#[test]
fn doctor_says_nothing_of_keeping_a_file_where_the_store_keeps_no_pull_requests() {
    let config = loaded(
        r#"version = 1
[store]
kind = "markers"
[sources.single]
kind = "github"
repository = "o/r"
areas = "codeowners"
token = { secret = "HERALD_FORGE_TOKEN" }
"#,
    );
    let script = Script::answering(&[(
        "/repos/o/r/contents/.github/CODEOWNERS",
        200,
        "* @org/web\n",
    )]);
    let secrets = secrets().with("HERALD_FORGE_TOKEN", "t");
    let prepared =
        pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let found = pollster::block_on(codeowners(&config, &connected, jiff::Timestamp::UNIX_EPOCH));
    assert_eq!(found.notes, Vec::<String>::new());
    assert_eq!(script.seen().len(), 1);
}
