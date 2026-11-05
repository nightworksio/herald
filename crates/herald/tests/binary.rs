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
    fn an_app_s_event_is_read_from_standard_input() {
        use std::io::Write as _;
        use std::process::Stdio;
        let directory = directory("http://127.0.0.1:1");
        let config = directory.path().join("herald.toml");
        let mut text = std::fs::read_to_string(&config).unwrap();
        text.push_str("[apps.ci]\nprojects = [\"herald\"]\n");
        std::fs::write(&config, text).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_herald"))
            .args(["announce", "--event", "-", "--app", "ci"])
            .current_dir(directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(br#"{"id": "7", "project": "other", "title": "Deployed"}"#)
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "the app \"ci\" may not announce the project \"other\"; add it to [apps.ci] projects\n"
        );
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

    #[cfg(unix)]
    #[test]
    fn a_server_stops_when_the_process_is_terminated() {
        let directory = directory("http://127.0.0.1:1");
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut command = Command::new(env!("CARGO_BIN_EXE_herald"));
        command.env_clear();
        if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", profile);
        }
        let server = command
            .args(["server", "--listen", &format!("127.0.0.1:{port}")])
            .current_dir(directory.path())
            .env("GITHUB_REPOSITORY", "cli/cli")
            .env("GITHUB_TOKEN", "t")
            .env("HERALD_DISCORD_TOKEN", "d")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let listening = (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
        });
        assert!(listening, "the server never listened");
        let killed = Command::new("kill")
            .args(["-TERM", &server.id().to_string()])
            .status()
            .unwrap();
        assert!(killed.success());
        let output = server.wait_with_output().unwrap();
        assert!(output.status.success(), "{:?}", output.status);
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains(&format!("herald serves on 127.0.0.1:{port}"))
        );
    }
}
