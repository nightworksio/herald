//! What doctor finds online of each source reading its forge's teams: a
//! forge refusing them named with the permission, and no team, member or
//! figure said (ADR-0077).

use herald_config::Uses;
use herald_flows::{forge_teams, prepare};
use herald_testing::FakeSigner;

use super::loaded;
use crate::harness::{Script, UNCONNECTED, secrets};

#[test]
fn doctor_names_the_permission_a_forge_refusing_its_teams_needs_and_says_no_team() {
    let config = loaded(
        r#"version = 1
[store]
kind = "sqlite"
[sources.a]
kind = "github"
repository = "a/x"
teams = "forge"
token = { secret = "HERALD_FORGE_TOKEN" }
[sources.b]
kind = "github"
repository = "b/y"
teams = "forge"
token = { secret = "HERALD_FORGE_TOKEN" }
[sources.c]
kind = "github"
repository = "c/z"
teams = "config"
token = { secret = "HERALD_FORGE_TOKEN" }
[sources.d]
kind = "gitlab"
repository = "d/w"
api = "https://gitlab.example/api/v4"
teams = "forge"
token = { secret = "HERALD_FORGE_TOKEN" }
"#,
    );
    let script = Script::answering(&[
        ("/orgs/a/teams?per_page=100&page=1", 403, "{}"),
        (
            "/orgs/b/teams?per_page=100&page=1",
            200,
            r#"[{"slug":"secret-team"}]"#,
        ),
        (
            "/orgs/b/teams/secret-team/members?per_page=100&page=1",
            200,
            r#"[{"login":"secret-login"}]"#,
        ),
        (
            "https://gitlab.example/api/v4/groups/d/descendant_groups?per_page=100&page=1",
            UNCONNECTED,
            "down",
        ),
    ]);
    let secrets = secrets().with("HERALD_FORGE_TOKEN", "t");
    let prepared =
        pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let found = pollster::block_on(forge_teams(&config, &connected));
    let keys: Vec<&str> = found.gaps.iter().map(|gap| gap.key.as_str()).collect();
    assert_eq!(keys, ["sources.a.teams", "sources.d.teams"]);
    assert!(
        found.gaps[0].why.contains("read:org")
            && found.gaps[0].why.contains("config's teams alone"),
        "{}",
        found.gaps[0].why
    );
    assert!(
        found.gaps[1].why.contains("could not be read"),
        "{}",
        found.gaps[1].why
    );
    assert_eq!(
        found.notes,
        ["The source b reads its forge's teams for each view showing its load."]
    );
    let said = format!("{found:?}");
    assert!(!said.contains("secret"), "{said}");
    assert_eq!(script.seen().len(), 4);
}
