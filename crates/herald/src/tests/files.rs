//! The files a config names, read by the native binary only beside
//! `herald.toml` or in a directory `[files] allowed` names, every link
//! followed (ADR-0092 decision 8).

use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::symlink;
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

[[routes]]
id = "releases"
sources = ["app"]
destination = "discord"
channel = "1566200000000000100"
template = "release"

[templates.release]
title = "{{ event.project }}"
file = "BODY"
"#;

/// A value no file of the tests holds but the one made to hold it.
fn canary() -> String {
    format!("canary-{}", std::process::id())
}

/// The config in `directory` whose template body is the file `body`, with
/// `allowed` as `[files] allowed`.
fn loaded(
    directory: &Path,
    body: &str,
    allowed: &[&Path],
) -> Result<herald_config::Config, String> {
    let mut text = CONFIG.replace("BODY", body);
    if !allowed.is_empty() {
        let listed: Vec<String> = allowed
            .iter()
            .map(|directory| format!("\"{}\"", directory.display()))
            .collect();
        let _ = write!(text, "\n[files]\nallowed = [{}]\n", listed.join(", "));
    }
    let path = directory.join("herald.toml");
    fs::write(&path, text).unwrap();
    crate::read(path.to_str().unwrap())
}

/// That `refused` names `file` and the setting, and holds nothing of what
/// the file holds.
fn told(refused: &str, file: &str, secret: &str) {
    assert!(refused.contains(file), "{refused}");
    assert!(refused.contains("[files] allowed"), "{refused}");
    assert!(!refused.contains(secret), "{refused}");
}

#[test]
fn a_template_file_beside_the_config_or_in_an_allowed_directory_is_read() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("templates")).unwrap();
    fs::write(
        directory.path().join("templates/body.md"),
        "{{ event.body }}",
    )
    .unwrap();
    assert!(loaded(directory.path(), "templates/body.md", &[]).is_ok());
    let allowed = tempfile::tempdir().unwrap();
    let canonical = fs::canonicalize(allowed.path()).unwrap();
    fs::write(canonical.join("body.md"), "{{ event.body }}").unwrap();
    let body = canonical.join("body.md");
    assert!(loaded(directory.path(), body.to_str().unwrap(), &[&canonical]).is_ok());
}

#[test]
fn a_file_outside_or_linked_outside_the_config_s_directory_is_refused_unread() {
    let secret = canary();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.md"), &secret).unwrap();
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("config")).unwrap();
    let config = directory.path().join("config");
    fs::write(directory.path().join("secret.md"), &secret).unwrap();
    symlink(outside.path().join("secret.md"), config.join("linked.md")).unwrap();
    symlink(outside.path(), config.join("templates")).unwrap();
    let absolute = outside.path().join("secret.md");
    for body in [
        "../secret.md",
        absolute.to_str().unwrap(),
        "linked.md",
        "templates/secret.md",
    ] {
        let refused = loaded(&config, body, &[]).unwrap_err();
        told(&refused, body, &secret);
    }
}

#[test]
fn a_link_in_an_allowed_directory_that_leads_out_of_it_is_refused() {
    let secret = canary();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.md"), &secret).unwrap();
    let allowed = tempfile::tempdir().unwrap();
    let canonical = fs::canonicalize(allowed.path()).unwrap();
    symlink(
        outside.path().join("secret.md"),
        canonical.join("linked.md"),
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let linked = canonical.join("linked.md");
    let refused = loaded(directory.path(), linked.to_str().unwrap(), &[&canonical]).unwrap_err();
    told(&refused, linked.to_str().unwrap(), &secret);
}

#[cfg(target_os = "linux")]
#[test]
fn the_process_environment_is_no_template_file() {
    let directory = tempfile::tempdir().unwrap();
    let refused = loaded(directory.path(), "/proc/self/environ", &[]).unwrap_err();
    assert!(refused.contains("/proc/self/environ"), "{refused}");
    assert!(refused.contains("[files] allowed"), "{refused}");
}

#[test]
fn doctor_names_each_file_the_config_names_outside_its_directory() {
    let text = format!(
        "{}\n[http]\nca_file = \"/etc/ssl/certs/corp.pem\"\n",
        CONFIG.replace("BODY", "../body.md")
    );
    let setup = super::world::setup(&text);
    let run = super::world::herald(&setup, &super::world::World::default(), &["doctor"]);
    assert_eq!(run.code, crate::MISTAKEN, "{}", run.err);
    for file in ["../body.md", "/etc/ssl/certs/corp.pem"] {
        assert!(
            run.err
                .contains(&format!("{file} could not be read: it lies outside")),
            "{file}: {}",
            run.err
        );
    }
}
