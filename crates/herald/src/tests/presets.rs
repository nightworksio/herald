//! A local preset read by the native binary: a file inside the config's
//! directory, every link followed (ADR-0092 decision 4).

use std::fs;
use std::path::Path;

const CONFIG: &str = r#"version = 1

[store]
kind = "markers"

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[sources.app]
kind = "github"
repository = "nightworksio/app"

[presets.team]
use = "./presets/team.toml"
destination = "discord"
sources = ["app"]
channel = "1566200000000000100"
"#;

const TEAM: &str = r#"version = 1

[[routes]]
id = "notes"
kinds = ["release"]
template = "notes"

[templates.notes]
title = "{{ event.project }}"
body = "{{ event.body }}"
"#;

/// The config at `directory`, `herald.toml` holding `CONFIG` with its
/// preset at `preset`.
fn loaded(directory: &Path, preset: &str) -> Result<herald_config::Config, String> {
    let path = directory.join("herald.toml");
    fs::write(&path, CONFIG.replace("./presets/team.toml", preset)).unwrap();
    crate::read(path.to_str().unwrap())
}

#[test]
fn a_local_preset_inside_the_config_s_directory_is_read() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("presets")).unwrap();
    fs::write(directory.path().join("presets/team.toml"), TEAM).unwrap();
    let config = loaded(directory.path(), "./presets/team.toml").unwrap();
    assert_eq!(config.routes[0].route.id.as_str(), "team-notes");
}

#[test]
fn a_local_preset_whose_links_lead_outside_the_config_s_directory_is_refused() {
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("team.toml"), TEAM).unwrap();
    let directory = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("team.toml"),
        directory.path().join("linked.toml"),
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.path(), directory.path().join("presets")).unwrap();
    for preset in ["./linked.toml", "./presets/team.toml"] {
        let error = loaded(directory.path(), preset).unwrap_err();
        assert!(
            error.contains("its links lead outside"),
            "{preset}: {error}"
        );
    }
}

#[test]
fn a_local_preset_that_is_no_file_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("presets")).unwrap();
    let error = loaded(directory.path(), "./presets").unwrap_err();
    assert!(error.contains("it is no file"), "{error}");
}

/// What `herald preset` says of the config at `directory`, answering
/// `answers`, and what it exits with.
fn presetting(
    directory: &Path,
    asked: &crate::cli::Presetting,
    answers: &[u8],
) -> (Result<u8, String>, String) {
    let path = directory.join("herald.toml");
    let mut answers = answers;
    let mut out = Vec::new();
    let done = pollster::block_on(crate::presetting::presetting(
        path.to_str().unwrap(),
        asked,
        &mut answers,
        &mut out,
    ));
    (done, String::from_utf8(out).unwrap())
}

/// A config directory whose `herald.toml` adopts `presets/team.toml`, a
/// comment above its store.
fn adopted() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("presets")).unwrap();
    fs::write(directory.path().join("presets/team.toml"), TEAM).unwrap();
    let config = CONFIG.replace("[store]", "# where herald keeps its records\n[store]");
    fs::write(directory.path().join("herald.toml"), config).unwrap();
    directory
}

#[test]
fn show_prints_the_tables_a_preset_expands_to() {
    let directory = adopted();
    let shown = crate::cli::Presetting::Show {
        name: "team".to_owned(),
    };
    let (done, out) = presetting(directory.path(), &shown, b"");
    assert_eq!(done, Ok(crate::DONE));
    for written in [
        "[[routes]]",
        "id = \"team-notes\"",
        "destination = \"discord\"",
        "channel = \"1566200000000000100\"",
        "template = \"team-notes\"",
        "[templates.team-notes]",
    ] {
        assert!(out.contains(written), "{written}: {out}");
    }
}

#[test]
fn show_names_a_table_the_config_does_not_have() {
    let directory = adopted();
    let shown = crate::cli::Presetting::Show {
        name: "other".to_owned(),
    };
    let (done, _) = presetting(directory.path(), &shown, b"");
    assert!(
        done.unwrap_err()
            .contains("adopts no preset as [presets.other]")
    );
}

fn eject(yes: bool, print: bool) -> crate::cli::Presetting {
    crate::cli::Presetting::Eject {
        name: "team".to_owned(),
        yes,
        print,
    }
}

#[test]
fn eject_writes_the_tables_in_place_of_the_preset_and_keeps_the_comments() {
    let directory = adopted();
    let before = loaded(directory.path(), "./presets/team.toml").unwrap();
    fs::write(
        directory.path().join("herald.toml"),
        CONFIG.replace("[store]", "# where herald keeps its records\n[store]"),
    )
    .unwrap();
    let (done, out) = presetting(directory.path(), &eject(true, false), b"");
    assert_eq!(done, Ok(crate::DONE), "{out}");
    let path = directory.path().join("herald.toml");
    let written = fs::read_to_string(&path).unwrap();
    assert!(!written.contains("[presets"), "{written}");
    assert!(
        written.contains("# where herald keeps its records"),
        "{written}"
    );
    let after = crate::read(path.to_str().unwrap()).unwrap();
    let routes = |config: &herald_config::Config| {
        config
            .routes
            .iter()
            .map(|route| format!("{:?} {:?}", route.route, route.options))
            .collect::<Vec<_>>()
    };
    assert_eq!(routes(&after), routes(&before));
}

#[test]
fn eject_writes_nothing_unless_confirmed_and_prints_the_config_under_print() {
    let directory = adopted();
    let path = directory.path().join("herald.toml");
    let was = fs::read_to_string(&path).unwrap();
    let (done, out) = presetting(directory.path(), &eject(false, false), b"n\n");
    assert_eq!(done, Ok(crate::STOPPED), "{out}");
    assert!(out.contains("Nothing was written."), "{out}");
    let (done, out) = presetting(directory.path(), &eject(false, true), b"");
    assert_eq!(done, Ok(crate::DONE));
    assert!(
        out.contains("[templates.team-notes]") && !out.contains("[presets"),
        "{out}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), was);
}

#[test]
fn eject_keeps_every_other_preset_the_config_adopts() {
    let directory = adopted();
    let path = directory.path().join("herald.toml");
    let text = fs::read_to_string(&path).unwrap().replace(
        "[presets.team]",
        "[presets.more]\nuse = \"./presets/team.toml\"\ndestination = \"discord\"\nsources = [\"app\"]\nchannel = \"1566200000000000101\"\n\n[presets.team]",
    );
    fs::write(&path, text).unwrap();
    let (done, out) = presetting(directory.path(), &eject(false, true), b"");
    assert_eq!(done, Ok(crate::DONE), "{out}");
    assert!(
        out.contains("[presets.more]") && !out.contains("[presets.team]"),
        "{out}"
    );
}

#[test]
fn eject_refuses_a_section_the_config_writes_inline() {
    let directory = adopted();
    let path = directory.path().join("herald.toml");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str(
        "\n[templates]\nown = { title = \"{{ event.project }}\", body = \"{{ event.body }}\" }\n",
    );
    text = text.replace(
        "version = 1\n",
        "version = 1\nroutes = [{ id = \"own\", sources = [\"app\"], destination = \"discord\", channel = \"1566200000000000102\", template = \"own\" }]\n",
    );
    fs::write(&path, text).unwrap();
    let (done, out) = presetting(directory.path(), &eject(true, false), b"");
    assert_eq!(
        done,
        Err("the config writes routes inline, where eject adds tables; write it as [[routes]] or [routes.<name>] tables first".to_owned()),
        "{out}"
    );
}

#[test]
fn herald_preset_show_reads_the_config_it_is_given() {
    let directory = adopted();
    let setup = super::world::Setup {
        directory,
        vars: std::collections::BTreeMap::new(),
    };
    let run = super::world::herald(
        &setup,
        &super::world::World::default(),
        &["preset", "show", "team"],
    );
    assert_eq!(run.code, crate::DONE, "{}", run.err);
    assert!(run.out.contains("[templates.team-notes]"), "{}", run.out);
}

#[test]
fn a_config_named_without_a_directory_reads_its_preset_from_the_working_directory() {
    use herald_config::Files as _;
    let beside = crate::Beside {
        directory: Path::new(""),
    };
    let read = beside.read_inside("Cargo.toml").unwrap();
    assert!(read.contains("name = \"herald\""), "{read}");
}
