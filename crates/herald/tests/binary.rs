//! The `herald` binary itself: its version, and a run that reaches nothing.

#[cfg(test)]
mod binary {
    use std::process::Command;

    fn herald(args: &[&str], directory: &std::path::Path) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_herald"));
        command.env_clear();
        if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", profile);
        }
        command
            .args(args)
            .current_dir(directory)
            .env("GITHUB_EVENT_NAME", "release")
            .env("GITHUB_EVENT_PATH", directory.join("event.json"))
            .env("GITHUB_REPOSITORY", "cli/cli")
            .env("GITHUB_TOKEN", "t")
            .env("HERALD_DISCORD_TOKEN", "d")
            .output()
            .unwrap()
    }

    const EVENT: &str =
        include_str!("../../herald-sources-forge/fixtures/github/release-event.reference.json");

    fn config(store_api: &str) -> String {
        format!(
            r#"version = 1
[store]
kind = "git-branch"
api = "{store_api}"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
api = "http://127.0.0.1:1/api"
[sources.cli]
kind = "github"
repository = "cli/cli"
[[routes]]
id = "releases"
sources = ["cli"]
destination = "discord"
channel = "100"
template = "t"
[templates.t]
title = "{{{{ event.title }}}}"
body = "{{{{ event.body }}}}"
"#
        )
    }

    fn directory(store_api: &str) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("herald.toml"), config(store_api)).unwrap();
        std::fs::write(directory.path().join("event.json"), EVENT).unwrap();
        directory
    }

    #[test]
    fn the_binary_says_its_version() {
        let directory = directory("http://127.0.0.1:1");
        let output = herald(&["--version"], directory.path());
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "herald 0.0.0\n");
    }

    #[test]
    fn a_run_whose_store_cannot_be_reached_stops_and_says_so() {
        let directory = directory("http://127.0.0.1:1");
        let output = herald(&["announce", "--from", "github"], directory.path());
        assert_eq!(output.status.code(), Some(1));
        let out = String::from_utf8(output.stdout).unwrap();
        assert!(
            out.starts_with("releases: stopped: the store could not be reached"),
            "{out}"
        );
    }

    #[test]
    fn doctor_runs_and_a_run_outside_actions_says_what_is_missing() {
        let directory = directory("http://127.0.0.1:1");
        let output = herald(&["doctor"], directory.path());
        assert!(output.status.success());
        let mut command = Command::new(env!("CARGO_BIN_EXE_herald"));
        let outside = command
            .args(["announce", "--from", "github"])
            .current_dir(directory.path())
            .env_remove("GITHUB_REPOSITORY")
            .output()
            .unwrap();
        assert_eq!(outside.status.code(), Some(1));
        assert!(
            String::from_utf8(outside.stderr)
                .unwrap()
                .contains("GITHUB_REPOSITORY is not set")
        );
    }

    #[test]
    fn a_store_base_that_may_not_be_reached_is_refused() {
        let directory = directory("http://example.org");
        let output = herald(&["announce", "--from", "github"], directory.path());
        assert_eq!(output.status.code(), Some(1));
        let err = String::from_utf8(output.stderr).unwrap();
        assert!(err.contains("http://example.org is not HTTPS"), "{err}");
    }
}
