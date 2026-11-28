//! The files a config names, read by the native binary only beside
//! `herald.toml` or in a directory `[files] allowed` names, every link
//! followed (ADR-0092 decision 8).

use std::collections::BTreeMap;
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
/// `allowed` as `[files] allowed`, each directory of `allowed` also listed
/// by the host.
fn loaded(
    directory: &Path,
    body: &str,
    allowed: &[&Path],
) -> Result<herald_config::Config, String> {
    let host: Vec<&str> = allowed.iter().map(|path| path.to_str().unwrap()).collect();
    hosted(directory, body, allowed, &host)
}

/// The config in `directory` whose template body is the file `body`, with
/// `allowed` as `[files] allowed`, on a host whose `HERALD_FILES_ALLOWED`
/// lists `host`, unset where it lists none.
fn hosted(
    directory: &Path,
    body: &str,
    allowed: &[&Path],
    host: &[&str],
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
    crate::read(path.to_str().unwrap(), &listing(host))
}

/// An environment whose `HERALD_FILES_ALLOWED` lists `host`, empty where it
/// lists none.
fn listing(host: &[&str]) -> BTreeMap<String, String> {
    if host.is_empty() {
        return BTreeMap::new();
    }
    let joined = std::env::join_paths(host).unwrap();
    BTreeMap::from([(
        herald_config::FILES_ALLOWED.to_owned(),
        joined.to_str().unwrap().to_owned(),
    )])
}

/// That `refused` refuses `files.allowed`, its reason naming none of
/// `typed`.
fn unwidened(refused: &str, typed: &[&Path]) {
    let (_, reason) = refused.split_once("files.allowed: ").unwrap();
    let reason = reason.lines().next().unwrap();
    for typed in typed {
        assert!(
            !reason.contains(typed.to_str().unwrap()),
            "{typed:?}: {reason}"
        );
    }
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

#[test]
fn a_config_cannot_widen_files_allowed_past_the_directories_its_host_lists() {
    let secret = canary();
    let base = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(base.path()).unwrap();
    let work = base.join("work");
    let workspace = base.join("workspace");
    let outside = base.join("outside");
    for directory in [&work, &workspace, &outside] {
        fs::create_dir(directory).unwrap();
    }
    fs::write(outside.join("secret.md"), &secret).unwrap();
    symlink(&outside, work.join("linked")).unwrap();
    let config = work.join("config");
    fs::create_dir(&config).unwrap();
    let host = [work.to_str().unwrap()];
    let linked = work.join("linked");
    for widened in [Path::new("/"), &base, &workspace, &outside, &linked] {
        let body = widened.join("secret.md");
        let refused = hosted(&config, body.to_str().unwrap(), &[widened], &host).unwrap_err();
        unwidened(&refused, &[widened]);
        assert!(
            refused.contains("lies inside no directory HERALD_FILES_ALLOWED lists on this host"),
            "{refused}"
        );
        assert!(!refused.contains(&secret), "{refused}");
    }
    let stepped = work.join("..").join("outside");
    let refused = hosted(
        &config,
        stepped.join("secret.md").to_str().unwrap(),
        &[&stepped],
        &host,
    )
    .unwrap_err();
    unwidened(&refused, &[&stepped]);
    assert!(refused.contains("is no absolute directory"), "{refused}");
    let templates = work.join("templates");
    fs::create_dir(&templates).unwrap();
    fs::write(templates.join("body.md"), "{{ event.body }}").unwrap();
    let body = templates.join("body.md");
    assert!(hosted(&config, body.to_str().unwrap(), &[&templates], &host).is_ok());
}

#[test]
fn with_no_host_list_files_allowed_reaches_no_further_than_the_config_s_own_directory() {
    let directory = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(directory.path()).unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let elsewhere = fs::canonicalize(elsewhere.path()).unwrap();
    fs::write(elsewhere.join("body.md"), "{{ event.body }}").unwrap();
    let body = elsewhere.join("body.md");
    let refused = hosted(&directory, body.to_str().unwrap(), &[&elsewhere], &[]).unwrap_err();
    unwidened(&refused, &[&elsewhere]);
    assert!(
        refused.contains("lies outside the config's own directory, and HERALD_FILES_ALLOWED, which lists more, is not set on this host"),
        "{refused}"
    );
    let templates = directory.join("templates");
    fs::create_dir(&templates).unwrap();
    fs::write(templates.join("body.md"), "{{ event.body }}").unwrap();
    let body = templates.join("body.md");
    assert!(hosted(&directory, body.to_str().unwrap(), &[&templates], &[]).is_ok());
}

#[test]
fn a_host_list_that_names_no_absolute_directory_stops_the_load_naming_none_of_it() {
    let directory = tempfile::tempdir().unwrap();
    let absent = directory.path().join("absent");
    for listed in ["relative/dir", absent.to_str().unwrap()] {
        let refused = hosted(directory.path(), "body.md", &[], &[listed]).unwrap_err();
        assert!(
            refused.contains("HERALD_FILES_ALLOWED lists a directory that is not absolute or not a directory on this host"),
            "{refused}"
        );
        assert!(!refused.contains(listed), "{refused}");
    }
}

#[test]
fn doctor_says_what_files_allowed_may_reach_and_where_the_host_took_it_from() {
    let mut setup = super::world::setup(&CONFIG.replace("BODY", "body.md"));
    let own = fs::canonicalize(setup.directory.path()).unwrap();
    let run = super::world::herald(&setup, &super::world::World::default(), &["doctor"]);
    assert!(
        run.out.contains(&format!(
            "[files] allowed may name directories inside the config's own, {}, alone: HERALD_FILES_ALLOWED is not set on this host",
            own.display()
        )),
        "{}",
        run.out
    );
    setup.vars.extend(listing(&[own.to_str().unwrap()]));
    let world = super::world::World::default();
    let run = super::world::herald(&setup, &world, &["doctor"]);
    assert!(
        run.out.contains(&format!(
            "[files] allowed may name directories inside those HERALD_FILES_ALLOWED lists on this host: {}",
            own.display()
        )),
        "{}",
        run.out
    );
    let refusing = CONFIG.replace("BODY", "body.md") + "\n[files]\nallowed = [\"/\"]\n";
    fs::write(setup.directory.path().join("herald.toml"), refusing).unwrap();
    let run = super::world::herald(&setup, &world, &["doctor"]);
    assert_eq!(run.code, crate::MISTAKEN, "{}", run.err);
    assert!(run.err.contains("files.allowed"), "{}", run.err);
    assert!(
        run.err.contains("HERALD_FILES_ALLOWED lists on this host:"),
        "{}",
        run.err
    );
    let run = super::world::herald(&setup, &world, &["poll"]);
    assert_eq!(run.code, crate::MISTAKEN, "{}", run.err);
    assert!(run.err.contains("files.allowed"), "{}", run.err);
    assert!(!run.err.contains("may name directories"), "{}", run.err);
}
