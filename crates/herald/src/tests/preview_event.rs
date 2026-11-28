use serde_json::{Value, json};

use super::world::*;
use crate::*;

/// A source announcing releases, pull requests and tags of cli/cli, and
/// a route of each to Discord.
const SOURCED: &str = r#"version = 1
[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.cli]
kind = "github"
repository = "cli/cli"
signals = ["release", "pull-request", "tag"]
[[routes]]
id = "everything"
sources = ["cli"]
destination = "discord"
channel = "1566200000000000100"
template = "shown"
[templates.shown]
title = "{{ event.kind }}: {{ event.title }}"
body = "{{ event.body | length }} characters"
"#;

/// A pull request of cli/cli opened, its repository seen as `visibility`.
fn opened(visibility: &str) -> Value {
    json!({
        "action": "opened",
        "pull_request": {
            "number": 4, "title": "Bump", "state": "open",
            "html_url": "https://github.com/cli/cli/pull/4",
            "user": { "login": "ann" }, "created_at": "2026-11-18T08:00:00Z",
        },
        "repository": {
            "id": 7, "name": "cli", "full_name": "cli/cli", "visibility": visibility,
            "private": visibility != "public",
            "html_url": "https://github.com/cli/cli",
        }
    })
}

/// `config` set up with `payload` written as `pull.json`, and its path.
fn with_file(config: &str, payload: &str) -> (Setup, String) {
    let setup = setup(config);
    let path = setup.directory.path().join("pull.json");
    std::fs::write(&path, payload).unwrap();
    (setup, path.display().to_string())
}

#[test]
fn preview_reads_an_event_of_any_kind_as_the_config_s_sources_read_the_delivery_its_header_names() {
    let world = World::default();
    let (setup, path) = with_file(SOURCED, &opened("public").to_string());
    let pulled = herald(
        &setup,
        &world,
        &["preview", "--event", &path, "--event-name", "pull_request"],
    );
    assert_eq!(
        (pulled.code, pulled.out.as_str()),
        (
            DONE,
            "everything to discord:\n  title: pull-request: Pull request #4 opened: Bump\n  part 1:\n    0 characters\n"
        )
    );
    let two = format!(
        "{SOURCED}[sources.app]\nkind = \"github\"\nrepository = \"nightworksio/app\"\n[[routes]]\nid = \"apps\"\nsources = [\"app\"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"shown\"\n"
    );
    let grouping = two.replace(
        "template = \"shown\"\n",
        "template = \"shown\"\ngroup = { key = \"{{ event.project }}\" }\n",
    );
    for (config, group) in [(two.as_str(), None), (grouping.as_str(), Some("v9"))] {
        let (released, release) = with_file(config, EVENT);
        let mut args = vec!["preview", "--event", &release];
        args.extend(group.map(|key| ["--group", key]).iter().flatten());
        let shown = herald(&released, &world, &args);
        assert_eq!(
            shown.out.lines().next(),
            Some("everything to discord:"),
            "{group:?}: {}",
            shown.err
        );
        assert!(!shown.out.contains("apps to"), "{group:?}: {}", shown.out);
    }
    let unnamed = herald(&setup, &world, &["preview", "--event", &path]);
    assert_eq!(
        (unnamed.code, unnamed.err.as_str()),
        (
            MISTAKEN,
            "nothing to preview: no source of the config reads the event file as an event it announces\n  source \"cli\": it cannot read it: it is not the event --event-name names, or not of a repository it takes\n"
        )
    );
    let tagged =
        json!({ "ref": "v1", "ref_type": "tag", "repository": { "full_name": "cli/cli" } });
    let (setup, path) = with_file(SOURCED, &tagged.to_string());
    let tag = herald(
        &setup,
        &world,
        &["preview", "--event", &path, "--event-name", "create"],
    );
    assert_eq!(
        (tag.code, tag.err.as_str()),
        (
            MISTAKEN,
            "nothing to preview: no source of the config reads the event file as an event it announces\n  source \"cli\": it reads it as a tag, a package or an owner's repositories, which a poll reads in full and preview does not\n"
        )
    );
    let (setup, path) = with_file(KINDLESS, &opened("public").to_string());
    assert_eq!(
        herald(&setup, &world, &["preview", "--event", &path]).err,
        "nothing to preview: the config has no source to read the event file\n"
    );
}

/// A daily report of the queue to Discord, and no source.
const KINDLESS: &str = r#"version = 1
[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[reports.daily]
view = "queue"
[[routes]]
id = "board"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000100"
template = "board"
private = false
[templates.board]
title = "{{ view.name }}"
body = "{{ view.name }}"
"#;

#[test]
fn an_event_of_a_private_repository_is_said_private_on_each_route_unless_the_output_is() {
    let world = World::default();
    let (setup, path) = with_file(SOURCED, &opened("private").to_string());
    let asked = ["preview", "--event", &path, "--event-name", "pull_request"];
    for (format, said) in [
        ("text", "everything to discord: (private)\n"),
        ("wire", "everything to discord: (private)\n"),
        (
            "json",
            "[{\"against\":null,\"destination\":\"discord\",\"private\":true,\"route\":\"everything\"}]\n",
        ),
    ] {
        let mut args = asked.to_vec();
        args.extend(["--format", format]);
        let withheld = herald(&setup, &world, &args);
        assert_eq!((withheld.code, withheld.out.as_str()), (DONE, said));
    }
    let mut args = asked.to_vec();
    args.push("--private");
    assert!(
        herald(&setup, &world, &args)
            .out
            .contains("title: pull-request: Pull request #4 opened: Bump")
    );
    let configured = with_file(
        &format!("{SOURCED}[cli]\nprivate = true\n"),
        &opened("private").to_string(),
    );
    assert!(
        herald(
            &configured.0,
            &world,
            &[
                "preview",
                "--event",
                &configured.1,
                "--event-name",
                "pull_request"
            ]
        )
        .out
        .contains("title: pull-request: Pull request #4 opened: Bump")
    );
    let owned = SOURCED
        .replace(
            "repository = \"cli/cli\"\nsignals = [\"release\", \"pull-request\", \"tag\"]\n",
            "owner = \"cli\"\nvisibility = [\"public\", \"private\"]\n",
        )
        .replace(
            "template = \"shown\"\n",
            "template = \"shown\"\nprivate = true\n",
        );
    let private_release = EVENT
        .replace("\"private\": false", "\"private\": true")
        .replace("\"visibility\": \"public\"", "\"visibility\": \"private\"");
    let (setup, path) = with_file(&owned, &private_release);
    let member = herald(&setup, &world, &["preview", "--event", &path]);
    assert_eq!(
        (member.code, member.out.as_str()),
        (DONE, "everything to discord: (private)\n"),
        "{}",
        member.err
    );
}

#[test]
fn preview_reads_an_event_from_standard_input_for_a_dash() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("herald.toml").display().to_string();
    let config = crate::loaded(&path, SOURCED).unwrap();
    let flags = crate::preview::Flags {
        route: None,
        kind: None,
        sample: None,
        report: None,
        event: Some("-".to_owned()),
        from: crate::cli::RunForge::Github,
        event_name: Some("pull_request".to_owned()),
        private: false,
        group: None,
        format: crate::cli::PreviewFormat::Text,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let payload = opened("public").to_string();
    let code = on_tokio(crate::preview::previewing(
        &config,
        &flags,
        &mut payload.as_bytes(),
        (&mut out, &mut err),
    ));
    assert_eq!(
        (code, String::from_utf8(out).unwrap().lines().nth(1)),
        (
            DONE,
            Some("  title: pull-request: Pull request #4 opened: Bump")
        )
    );
}

#[test]
fn no_refusal_of_an_event_file_repeats_its_path_its_name_or_what_it_holds() {
    let world = World::default();
    let typed = format!("canary-{}", std::process::id());
    let (setup, _) = with_file(SOURCED, "");
    let missing = setup
        .directory
        .path()
        .join(&typed)
        .join("event.json")
        .display()
        .to_string();
    for (payload, args, said) in [
        (
            None,
            vec!["--event", missing.as_str()],
            "the event file could not be read: No such file or directory (os error 2)",
        ),
        (
            Some(format!("{{\"action\": \"{typed}\"")),
            vec!["--event", "FILE"],
            "the event file cannot be read at line 1, column",
        ),
        (
            Some(format!("{{\"action\": {{\"{typed}\": 1}}}}")),
            vec!["--event", "FILE", "--event-name", "pull_request"],
            "  source \"cli\": it cannot read it:",
        ),
        (
            Some(opened("public").to_string()),
            vec!["--event", "FILE", "--event-name", typed.as_str()],
            "  source \"cli\": the delivery is not of a release, a tag or a package",
        ),
    ] {
        let file = setup.directory.path().join(format!("{typed}.json"));
        if let Some(payload) = &payload {
            std::fs::write(&file, payload).unwrap();
        }
        let file = file.display().to_string();
        let mut all = vec!["preview"];
        all.extend(
            args.iter()
                .map(|arg| if *arg == "FILE" { file.as_str() } else { arg }),
        );
        let refused = herald(&setup, &world, &all);
        assert_eq!(refused.code, MISTAKEN, "{args:?}");
        assert!(refused.err.contains(said), "{args:?}: {}", refused.err);
        assert!(
            !refused.err.contains(&typed) && !refused.out.contains(&typed),
            "{args:?}: {}",
            refused.err
        );
    }
}
