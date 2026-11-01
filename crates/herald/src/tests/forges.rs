use super::world::*;
use crate::*;

const HOOK: &str =
    include_str!("../../../herald-sources-forge/fixtures/gitlab/release-hook.reference.json");
const GITLAB_RELEASES: &str =
    include_str!("../../../herald-sources-forge/fixtures/gitlab/releases.json");
const FORGEJO_EVENT: &str =
    include_str!("../../../herald-sources-forge/fixtures/forgejo/release-event.reference.json");
const FORGEJO_RELEASES: &str =
    include_str!("../../../herald-sources-forge/fixtures/forgejo/releases.json");
const FORGEJO_REPOSITORY: &str =
    include_str!("../../../herald-sources-forge/fixtures/forgejo/repository.json");

const GITLAB_RELEASE: &str = "/projects/gitlab-org%2Fcli/releases/v1.126.0";

fn posted(world: &World) {
    discord(world, &[("/channels/100/messages", 200, r#"{"id":"1"}"#)]);
}

fn gitlab() -> (Setup, World) {
    let mut setup = setup(&CONFIG.replace(
        "kind = \"github\"\nrepository = \"cli/cli\"",
        "kind = \"gitlab\"\nrepository = \"gitlab-org/cli\"",
    ));
    for name in ["GITHUB_EVENT_NAME", "GITHUB_EVENT_PATH", "GITHUB_RUN_ID"] {
        setup.vars.remove(name);
    }
    setup.vars.extend(
        [
            ("CI_PROJECT_PATH", "gitlab-org/cli"),
            ("CI_PROJECT_ID", "34675721"),
            ("CI_PROJECT_NAME", "cli"),
            ("CI_COMMIT_TAG", "v1.126.0"),
            ("CI_JOB_TOKEN", "job-token"),
            ("CI_API_V4_URL", "https://gitlab.com/api/v4"),
            ("CI_PIPELINE_ID", "11"),
            ("CI_JOB_ID", "12"),
        ]
        .map(|(name, value)| (name.to_owned(), value.to_owned())),
    );
    let world = World::default();
    let releases: serde_json::Value = serde_json::from_str(GITLAB_RELEASES).unwrap();
    world
        .state
        .lock()
        .unwrap()
        .forge
        .insert(GITLAB_RELEASE.to_owned(), (200, releases[0].to_string()));
    (setup, world)
}

#[test]
fn a_gitlab_pipeline_for_a_tag_announces_its_release_without_asking_for_the_project() {
    let (setup, world) = gitlab();
    posted(&world);
    let announced = herald(&setup, &world, &["announce", "--from", "gitlab"]);
    assert_eq!(
        (
            announced.code,
            announced.out.as_str(),
            announced.err.as_str()
        ),
        (DONE, "releases: announced (post)\n", "")
    );
    let seen = world.state.lock().unwrap().seen.clone();
    assert_eq!(
        seen.first().map(String::as_str),
        Some(format!("GET https://gitlab.com/api/v4{GITLAB_RELEASE}").as_str())
    );
    assert!(
        !seen
            .iter()
            .any(|line| line.ends_with("/projects/gitlab-org%2Fcli"))
    );
}

#[test]
fn a_ci_run_asks_its_own_forge_unless_the_source_names_an_api() {
    let (mut setup, world) = gitlab();
    setup.vars.insert(
        "CI_API_V4_URL".to_owned(),
        "https://unreachable.gitlab.example/api/v4".to_owned(),
    );
    let own = herald(&setup, &world, &["announce", "--from", "gitlab"]);
    assert_eq!(
        own.err,
        "https://unreachable.gitlab.example/api/v4 cannot be reached\n"
    );
    std::fs::write(
        setup.directory.path().join("herald.toml"),
        CONFIG.replace(
            "kind = \"github\"\nrepository = \"cli/cli\"",
            "kind = \"gitlab\"\nrepository = \"gitlab-org/cli\"\napi = \"https://gitlab.com/api/v4\"",
        ),
    )
    .unwrap();
    posted(&world);
    let named = herald(&setup, &world, &["announce", "--from", "gitlab"]);
    assert_eq!(
        (named.code, named.out.as_str()),
        (DONE, "releases: announced (post)\n")
    );
}

#[test]
fn a_gitlab_pipeline_without_a_tag_announces_nothing_and_a_bad_project_id_says_so() {
    let (mut setup, world) = gitlab();
    setup.vars.remove("CI_COMMIT_TAG");
    let untagged = herald(&setup, &world, &["announce", "--from", "gitlab"]);
    assert_eq!(
        (untagged.code, untagged.out.as_str()),
        (
            DONE,
            "The pipeline is not for a tag; nothing to announce.\n"
        )
    );
    setup
        .vars
        .insert("CI_PROJECT_ID".to_owned(), "cli".to_owned());
    let named = herald(
        &setup,
        &world,
        &["announce", "--from", "gitlab", "--release", "v1.126.0"],
    );
    assert_eq!(
        (named.code, named.err.as_str()),
        (STOPPED, "CI_PROJECT_ID is cli, not a project's id\n")
    );
    let unnamed = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        unnamed.err,
        "no source in the config names cli/cli; add [sources.<name>] with kind = \"github\" and repository = \"cli/cli\"\n"
    );
}

fn forgejo() -> (Setup, World) {
    let mut setup = setup(
        &CONFIG
            .replace(
                "kind = \"github\"\nrepository = \"cli/cli\"",
                "kind = \"forgejo\"\nrepository = \"forgejo/forgejo\"",
            )
            .replace(
                "kind = \"git-branch\"",
                "kind = \"git-branch\"\nrepository = \"cli/cli\"",
            ),
    );
    std::fs::write(setup.directory.path().join("event.json"), FORGEJO_EVENT).unwrap();
    setup
        .vars
        .insert("GITHUB_REPOSITORY".to_owned(), "forgejo/forgejo".to_owned());
    let world = World::default();
    let releases: serde_json::Value = serde_json::from_str(FORGEJO_RELEASES).unwrap();
    let mut state = world.state.lock().unwrap();
    state.forge.insert(
        "/repos/forgejo/forgejo/releases/tags/v16.0.7".to_owned(),
        (200, releases[1].to_string()),
    );
    state.forge.insert(
        "/repos/forgejo/forgejo".to_owned(),
        (200, FORGEJO_REPOSITORY.to_owned()),
    );
    drop(state);
    (setup, world)
}

#[test]
fn a_forgejo_run_announces_its_event_or_a_release_named_by_its_tag() {
    let (setup, world) = forgejo();
    posted(&world);
    let evented = herald(&setup, &world, &["announce", "--from", "forgejo"]);
    assert_eq!(
        (evented.code, evented.out.as_str(), evented.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    posted(&world);
    let tagged = herald(
        &setup,
        &world,
        &["announce", "--from", "forgejo", "--release", "v16.0.7"],
    );
    assert_eq!(
        (tagged.code, tagged.out.as_str()),
        (DONE, "releases: announced (post)\n")
    );
    let seen = world.state.lock().unwrap().seen.clone();
    for asked in [
        "GET https://codeberg.org/api/v1/repos/forgejo/forgejo/releases/tags/v16.0.7",
        "GET https://codeberg.org/api/v1/repos/forgejo/forgejo",
    ] {
        assert!(seen.iter().any(|line| line == asked), "{asked}: {seen:?}");
    }
}

#[test]
fn preview_reads_the_event_file_of_the_forge_it_names() {
    let setup = setup(CONFIG);
    let world = World::default();
    let path = setup.directory.path().join("hook.json");
    std::fs::write(&path, HOOK).unwrap();
    let path = path.display().to_string();
    let previewed = herald(
        &setup,
        &world,
        &["preview", "--from", "gitlab", "--event", &path],
    );
    assert!(
        previewed
            .out
            .starts_with("releases to discord:\n  title: 📦 cli v1.126.0\n"),
        "{}",
        previewed.out
    );
    let misread = herald(&setup, &world, &["preview", "--event", &path]);
    assert_eq!(misread.code, MISTAKEN);
    assert!(
        misread
            .err
            .starts_with("GitHub's release event could not be read"),
        "{}",
        misread.err
    );
}
