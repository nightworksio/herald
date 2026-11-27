//! The files a config names: each read only beside `herald.toml` or in a
//! directory `[files] allowed` names, and none echoed where it does not
//! parse (ADR-0092 decision 8).

use std::cell::RefCell;

use herald_config::{Files, Unread, Within, load, load_with};

const CONFIG: &str = r#"version = 1

[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "release"

[templates.release]
title = "{{ event.project }}"
body = "{{ event.body }}"
"#;

/// A value no file of the tests holds but the one made to hold it.
fn canary() -> String {
    format!("canary-{}-{}", std::process::id(), line!())
}

/// `CONFIG` with `table`'s file named `file`, where `table` is a
/// template's, a member list's, a block list's or the CA file.
fn naming(table: &str, file: &str) -> String {
    match table {
        "templates.release.file" => CONFIG.replace(
            "body = \"{{ event.body }}\"",
            &format!("file = \"{file}\""),
        ),
        "sources.org.members.file" => format!(
            "{CONFIG}\n[sources.org]\nkind = \"github\"\nowner = \"acme\"\nmembers = {{ file = \"{file}\" }}\n"
        ),
        "destinations.fedi.lists" => CONFIG.replace(
            "key = { secret = \"HERALD_ACTIVITYPUB_KEY\" }\n",
            &format!(
                "key = {{ secret = \"HERALD_ACTIVITYPUB_KEY\" }}\nlists = [{{ file = \"{file}\", sha256 = \"{}\" }}]\n",
                herald_core::digest(&canary())
            ),
        ),
        _ => format!("{CONFIG}\n[http]\nca_file = \"{file}\"\n"),
    }
}

/// What loading `text` says, with every file it asks for named in `asked`
/// and answered with `answer`.
fn loading(text: &str, answer: &str) -> (Result<herald_config::Config, String>, Vec<String>) {
    let asked = RefCell::new(Vec::new());
    let read = |name: &str| {
        asked.borrow_mut().push(name.to_owned());
        Ok(answer.to_owned())
    };
    let loaded = load("herald.toml", text, &read, &crate::common::kinds())
        .map_err(|errors| errors.to_string());
    (loaded, asked.into_inner())
}

const TABLES: [&str; 4] = [
    "templates.release.file",
    "sources.org.members.file",
    "destinations.fedi.lists",
    "http.ca_file",
];

#[test]
fn every_file_the_config_names_outside_its_directory_is_refused_unread() {
    let secret = canary();
    for table in TABLES {
        for file in [
            "/proc/self/environ",
            "../outside.toml",
            "templates/../../outside.toml",
            "/etc/herald/outside.toml",
        ] {
            let (loaded, asked) = loading(&naming(table, file), &secret);
            let error = loaded.unwrap_err();
            assert!(asked.is_empty(), "{table} {file}: read {asked:?}");
            assert!(error.contains(file), "{table} {file}: {error}");
            assert!(
                error.contains("lies outside the directory of herald.toml and every directory [files] allowed names"),
                "{table} {file}: {error}"
            );
            assert!(!error.contains(&secret), "{table} {file}: {error}");
        }
    }
}

#[test]
fn a_file_in_a_directory_files_allowed_names_is_read() {
    let allowed = "\n[files]\nallowed = [\"/etc/herald\"]\n";
    let (loaded, asked) = loading(
        &format!(
            "{}{allowed}",
            naming("templates.release.file", "/etc/herald/body.md")
        ),
        "{{ event.body }}",
    );
    assert!(loaded.is_ok(), "{loaded:?}");
    assert_eq!(asked, ["/etc/herald/body.md"]);
    let (loaded, asked) = loading(
        &format!(
            "{}{allowed}",
            naming("templates.release.file", "/etc/heraldic/body.md")
        ),
        "{{ event.body }}",
    );
    assert!(loaded.is_err());
    assert!(asked.is_empty(), "{asked:?}");
}

#[test]
fn a_directory_files_allowed_names_is_absolute_with_no_step_up() {
    for directory in ["templates", "/etc/herald/../ssh"] {
        let (loaded, _) = loading(
            &format!("{CONFIG}\n[files]\nallowed = [\"{directory}\"]\n"),
            "",
        );
        let error = loaded.unwrap_err();
        assert!(error.contains("files.allowed"), "{error}");
        assert!(
            error.contains(&format!("\"{directory}\" is no absolute directory")),
            "{error}"
        );
    }
}

#[test]
fn a_file_that_does_not_parse_is_told_by_its_file_and_line_alone() {
    let secret = canary();
    let cases = [
        (
            "templates.release.file",
            "body.md",
            format!("hello\n{{% {secret} %}}\n"),
            "body.md cannot be read at line 2",
        ),
        (
            "sources.org.members.file",
            "members.toml",
            format!("[[repositories]]\nname = \"a\"\ntoken = {secret}\n"),
            "members.toml cannot be read at line 3, column 9",
        ),
        (
            "sources.org.members.file",
            "members.json",
            format!("[{{\"name\": {secret}}}]"),
            "members.json cannot be read at line 1, column 11",
        ),
    ];
    for (table, file, text, told) in cases {
        let (loaded, _) = loading(&naming(table, file), &text);
        let error = loaded.unwrap_err();
        assert!(error.contains(told), "{table}: {error}");
        assert!(!error.contains(&secret), "{table}: {error}");
    }
}

#[test]
fn a_block_list_row_that_names_no_domain_is_told_by_its_line_alone() {
    let secret = canary();
    let text = CONFIG.replace(
        "key = { secret = \"HERALD_ACTIVITYPUB_KEY\" }\n",
        &format!(
            "key = {{ secret = \"HERALD_ACTIVITYPUB_KEY\" }}\nlists = [{{ file = \"blocks.csv\", sha256 = \"{}\" }}]\n",
            herald_core::digest(&format!("#domain\n{secret}!,suspend\n"))
        ),
    );
    let (loaded, _) = loading(&text, &format!("#domain\n{secret}!,suspend\n"));
    let error = loaded.unwrap_err();
    assert!(error.contains("line 2 names no domain"), "{error}");
    assert!(!error.contains(&secret), "{error}");
}

/// A host that follows links: each file it is asked for, with the
/// directory it must lie in, found to lead outside it.
struct Linked(RefCell<Vec<(String, Option<String>)>>);

impl Files for Linked {
    fn read(&self, path: &str, within: Within<'_>) -> Result<String, Unread> {
        let within = match within {
            Within::Beside => None,
            Within::Allowed(directory) => Some(directory.to_owned()),
        };
        self.0.borrow_mut().push((path.to_owned(), within));
        Err(Unread::Outside)
    }

    fn read_inside(&self, path: &str) -> Result<String, String> {
        Err(format!("no file {path}"))
    }
}

#[test]
fn a_host_that_follows_links_is_asked_within_the_directory_each_file_must_lie_in() {
    let linked = Linked(RefCell::new(Vec::new()));
    let text = format!(
        "{}\n[files]\nallowed = [\"/etc/herald\"]\n",
        naming("templates.release.file", "body.md").replace(
            "[sources.gate]",
            "[http]\nca_file = \"/etc/herald/ca.pem\"\n\n[sources.gate]"
        )
    );
    let error = load_with("herald.toml", &text, &linked, &crate::common::kinds())
        .unwrap_err()
        .to_string();
    assert_eq!(
        linked.0.into_inner(),
        [
            ("body.md".to_owned(), None),
            (
                "/etc/herald/ca.pem".to_owned(),
                Some("/etc/herald".to_owned())
            ),
        ]
    );
    for file in ["body.md", "/etc/herald/ca.pem"] {
        assert!(
            error.contains(&format!(
                "{file} could not be read: it lies outside the directory of herald.toml"
            )),
            "{file}: {error}"
        );
    }
}
