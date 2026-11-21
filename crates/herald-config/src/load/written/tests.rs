use std::collections::BTreeMap;

use super::apart;
use crate::load::Loader;
use crate::{Config, Kinds, load};

const HEAD: &str = r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
[[routes]]
id = "releases"
sources = ["org"]
destination = "discord"
channel = "1566200000000000900"
template = "t"
"#;

const REPO: &str = "[sources.org]\nkind = \"github\"\nrepository = \"o/r\"\n";

const OTHER: &str = "[sources.other]\nkind = \"github\"\nrepository = \"o/other\"\n";

const TOKEN: &str = "write = { token = { secret = \"HERALD_RERUN\" } }\n";

const RERUN_APP: &str = "[github_apps.rerun]\nclient_id = \"Iv23liRerun\"\nprivate_key = { secret = \"HERALD_RERUN_KEY\" }\n";

fn kinds() -> Kinds {
    Kinds::new()
        .destination::<herald_dest_discord::DiscordKind>()
        .source::<herald_sources_forge::GitHubKind>()
        .store::<herald_store_sqlite::SqliteKind>()
}

fn text(rest: &str) -> String {
    format!("{HEAD}{rest}")
}

fn loaded(rest: &str) -> Result<Config, String> {
    load(
        "herald.toml",
        &text(rest),
        &|_| Err(String::new()),
        &kinds(),
    )
    .map_err(|errors| errors.to_string())
}

#[test]
fn every_write_credential_is_held_apart_or_refused_saying_why() {
    for apart in [
        format!("{REPO}{TOKEN}{OTHER}{TOKEN}"),
        format!("{REPO}{TOKEN}{OTHER}"),
        format!("{REPO}write = {{ app = \"rerun\" }}\n{RERUN_APP}"),
    ] {
        assert!(
            loaded(&apart).is_ok(),
            "{apart}: {:?}",
            loaded(&apart).err()
        );
    }
    for (refused, said) in [
        (
            format!("{REPO}write = {{ app = \"rerun\" }}\n{OTHER}app = \"rerun\"\n{RERUN_APP}"),
            "a source reads as the App rerun",
        ),
        (
            format!(
                "{REPO}write = {{ app = \"rerun\" }}\n{OTHER}app = \"herald\"\n{RERUN_APP}[github_apps.herald]\nclient_id = \"Iv23liRerun\"\nprivate_key = {{ secret = \"HERALD_APP_KEY\" }}\n"
            ),
            "a source reads as the App rerun",
        ),
        (
            format!("{REPO}write = {{ token = {{ secret = \"HERALD_DISCORD_TOKEN\" }} }}\n"),
            "which herald also reads",
        ),
        (
            format!("{REPO}{TOKEN}{OTHER}api = \"https://ghes.example/api/v3\"\n{TOKEN}"),
            "at another API",
        ),
        (
            format!(
                "{REPO}write = {{ app = \"rerun\" }}\n{OTHER}write = {{ token = {{ secret = \"HERALD_RERUN_KEY\" }} }}\n{RERUN_APP}"
            ),
            "as another credential",
        ),
    ] {
        let error = loaded(&refused).unwrap_err();
        assert!(error.contains(said), "{refused}: {error}");
    }
}

#[test]
fn a_write_app_no_table_gives_is_left_to_the_check_that_names_it() {
    let given = text(&format!("{REPO}write = {{ app = \"rerun\" }}\n{RERUN_APP}"));
    let kinds = kinds();
    let mut config = load("herald.toml", &given, &|_| Err(String::new()), &kinds).unwrap();
    config.github_apps.clear();
    let loader = Loader::new("herald.toml", &given, &kinds);
    let kept = apart(config, loader, &BTreeMap::new()).unwrap();
    assert!(kept.github_apps.is_empty());
}
