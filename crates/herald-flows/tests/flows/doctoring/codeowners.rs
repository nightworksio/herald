//! What doctor finds online in the CODEOWNERS file of each source reading
//! areas from one (ADR-0073).

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
"#,
    );
    let script = Script::answering(&[(
        "/repos/o/r/contents/.github/CODEOWNERS",
        200,
        "* @org/web\n[\n",
    )]);
    let secrets = secrets().with("HERALD_FORGE_TOKEN", "t");
    let prepared =
        pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let found = pollster::block_on(codeowners(&config, &connected));
    assert_eq!(found.notes.len(), 2, "{:?}", found.notes);
    assert_eq!(
        found.notes[0],
        "The source owned reads an owner's repositories, so doctor reads none of their CODEOWNERS files: a line one skips leaves its owners out of that repository's areas."
    );
    assert!(
        found.notes[1].starts_with(
            "The source single skips line 2 of its CODEOWNERS file .github/CODEOWNERS, which matches no path: "
        ),
        "{}",
        found.notes[1]
    );
    assert_eq!(script.seen().len(), 1);
}
