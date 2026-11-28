use clap::Parser as _;
use std::path::Path;

use super::world::*;

/// The key the random generator makes in these tests, built at run time.
pub(super) fn key() -> String {
    "ab".repeat(32)
}

/// `herald init` as `args` asks in `directory`, the operator answering
/// `answer` and the random generator making `random`: its code or why it
/// stopped, and what it printed. Nothing it reaches answers, the
/// environment is empty and no terminal is there.
fn init(
    directory: &Path,
    args: &[&str],
    answer: &str,
    random: Option<String>,
) -> (Result<u8, String>, String) {
    let unreached = |_: herald_ports::Base<'_>, _: &herald_ports::Outbound| Err(String::new());
    let trying = crate::init::Trying {
        vars: &std::collections::BTreeMap::new(),
        connect: &crate::Connector::new(&unreached, &|_, _| Err(String::new())),
        unechoed: None,
    };
    init_trying(directory, args, answer, (random, &trying))
}

/// [`init`], its test post read and posted as `trying` says.
pub(super) fn init_trying(
    directory: &Path,
    args: &[&str],
    answer: &str,
    (random, trying): (Option<String>, &crate::init::Trying<'_>),
) -> (Result<u8, String>, String) {
    let path = directory.join("herald.toml").display().to_string();
    let mut all = vec!["herald", "init"];
    all.extend(args);
    let crate::cli::Command::Init(asked) = crate::cli::Cli::try_parse_from(all).unwrap().command
    else {
        panic!("not init");
    };
    let mut out = Vec::new();
    let mut answers = answer.as_bytes();
    let done = on_tokio(crate::init::initialising(
        &path,
        &asked,
        (&mut answers, &mut out),
        (|| random, trying),
    ));
    (done, String::from_utf8(out).unwrap())
}

/// A repository in `directory` whose origin is `url`.
pub(super) fn git(directory: &Path, url: &str) {
    std::fs::create_dir_all(directory.join(".git")).unwrap();
    std::fs::write(
        directory.join(".git/config"),
        format!("[core]\n\tbare = false\n[remote \"origin\"]\n\turl = {url}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n"),
    )
    .unwrap();
}

/// The config written in `directory`, where one was.
pub(super) fn written(directory: &Path) -> Option<String> {
    std::fs::read_to_string(directory.join("herald.toml")).ok()
}

const SCHEMA: &str =
    "#:schema https://raw.githubusercontent.com/nightworksio/herald/main/herald.schema.json\n";

const SLACK_WEBHOOK: &str = r#"version = 1

[store]
kind = "actions-cache"
key = { secret = "HERALD_STORE_KEY" }

[destinations.slack]
kind = "slack"
webhook = { secret = "HERALD_SLACK_WEBHOOK" }

[sources.app]
kind = "github"
repository = "owner/app"

[presets.announce-releases]
use = "herald:announce-releases"
destination = "slack"
sources = ["app"]
"#;

#[test]
fn a_slack_webhook_on_github_keeps_its_records_in_the_actions_cache_under_a_key_printed_once() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), "git@github.com:owner/app.git");
    let (done, out) = init(
        directory.path(),
        &["--chat", "slack", "--yes"],
        "",
        Some(key()),
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    let text = written(directory.path()).unwrap();
    assert_eq!(text, format!("{SCHEMA}{SLACK_WEBHOOK}"));
    assert!(
        out.contains("The repository's origin is owner/app on github.com."),
        "{out}"
    );
    assert!(out.contains("A webhook cannot read its posts back, so herald keeps its records in the GitHub Actions cache"), "{out}");
    assert!(out.contains("Store HERALD_SLACK_WEBHOOK as a repository secret (gh secret set HERALD_SLACK_WEBHOOK); https://github.com/nightworksio/herald/blob/main/.docs/first-run.md#slack-webhook says how to make it."), "{out}");
    assert!(out.contains("Store HERALD_STORE_KEY as a repository secret (gh secret set HERALD_STORE_KEY): it is the key below, which herald prints only this once."), "{out}");
    assert_eq!(out.matches(&key()).count(), 1, "{out}");
    assert!(!out.contains("no store keeps"), "{out}");
    for listed in [
        "  github: ",
        "  slack: ",
        "  webhook: ",
        "  herald:announce-releases: ",
    ] {
        assert!(!out.contains(listed), "{listed}\n{out}");
    }
    assert!(
        out.contains(&format!("HERALD_STORE_KEY={}\n", key())),
        "{out}"
    );
    assert!(!text.contains(&key()));
    let (again, _) = init(directory.path(), &["--yes"], "", Some(key()));
    assert_eq!(
        again,
        Err(format!(
            "{} exists, so nothing was written: add to it with herald setup …, and judge it with herald doctor",
            directory.path().join("herald.toml").display()
        ))
    );
    assert_eq!(written(directory.path()).unwrap(), text);
}

#[test]
fn print_prints_the_config_alone_and_writes_nothing_and_makes_no_key() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), "https://github.com/owner/app");
    let (done, out) = init(
        directory.path(),
        &["--chat", "slack", "--yes", "--print"],
        "",
        None,
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    assert!(out.ends_with(&format!("{SCHEMA}{SLACK_WEBHOOK}")), "{out}");
    assert!(!out.contains("HERALD_STORE_KEY="), "{out}");
    assert_eq!(written(directory.path()), None);
}

#[test]
fn a_bot_reads_its_posts_back_so_markers_keep_it_and_its_channel_lands_in_each_preset() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), "https://github.com/owner/app.git");
    let (done, out) = init(
        directory.path(),
        &[
            "--chat",
            "discord",
            "--variant",
            "bot",
            "--with",
            "channel=1566184281406177531",
            "--announce",
            "herald:announce-releases",
            "--announce",
            "herald:security-alerts-private",
            "--announce",
            "herald:review-reminders",
            "--announce",
            "herald:announce-releases",
            "--yes",
        ],
        "",
        None,
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    let text = written(directory.path()).unwrap();
    assert!(text.contains("[store]\nkind = \"markers\"\n\n"), "{text}");
    assert!(
        text.contains("token = { secret = \"HERALD_DISCORD_TOKEN\" }"),
        "{text}"
    );
    assert!(
        text.contains("repository = \"owner/app\"\nsignals = [\"release\", \"alert\"]\n"),
        "{text}"
    );
    for table in [
        "announce-releases",
        "security-alerts-private",
        "review-reminders",
    ] {
        assert_eq!(
            text.matches(&format!("[presets.{table}]\n")).count(),
            1,
            "{text}"
        );
    }
    assert_eq!(
        text.matches("channel = \"1566184281406177531\"").count(),
        3,
        "{text}"
    );
    assert!(
        out.contains("The bot reads its posts back, so the markers store keeps"),
        "{out}"
    );
    assert!(
        out.contains("first-run.md#discord-bot says how to make it."),
        "{out}"
    );
    assert!(!out.contains("HERALD_STORE_KEY"), "{out}");
}

#[test]
fn a_self_hosted_forgejo_writes_its_api_and_keeps_records_on_its_branch() {
    let directory = tempfile::tempdir().unwrap();
    git(
        directory.path(),
        "ssh://git@forge.example:2222/team/app.git",
    );
    let (done, out) = init(
        directory.path(),
        &["--forge", "forgejo", "--chat", "slack", "--yes"],
        "",
        Some(key()),
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    let text = written(directory.path()).unwrap();
    assert!(text.contains("[store]\nkind = \"forgejo-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }\ntoken = { secret = \"HERALD_STORE_TOKEN\" }\n"), "{text}");
    assert!(text.contains("[sources.app]\nkind = \"forgejo\"\nrepository = \"team/app\"\napi = \"https://forge.example/api/v1\"\n"), "{text}");
    assert!(out.contains("Store HERALD_STORE_TOKEN as a repository secret; https://github.com/nightworksio/herald/blob/main/.docs/first-run.md#forgejo-branch says how to make it and the branch."), "{out}");
    assert!(
        out.contains("Store HERALD_STORE_KEY as a repository secret: it is the key below"),
        "{out}"
    );
}

#[test]
fn on_gitlab_only_a_way_in_that_reads_its_posts_back_is_offered() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), "https://gitlab.com/group/sub/app.git");
    let (refused, _) = init(
        directory.path(),
        &["--chat", "slack", "--variant", "webhook", "--yes"],
        "",
        None,
    );
    let why = refused.unwrap_err();
    assert!(
        why.starts_with("no store keeps herald's records in a GitLab repository yet"),
        "{why}"
    );
    assert!(
        why.ends_with("choose a way into slack that reads its posts back, or another chat"),
        "{why}"
    );
    assert_eq!(written(directory.path()), None);
    let (done, out) = init(
        directory.path(),
        &["--chat", "slack", "--with", "channel=C0123456789", "--yes"],
        "",
        None,
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    let text = written(directory.path()).unwrap();
    assert!(text.contains("kind = \"markers\""), "{text}");
    assert!(
        text.contains("token = { secret = \"HERALD_SLACK_TOKEN\" }"),
        "{text}"
    );
    assert!(
        text.contains("kind = \"gitlab\"\nrepository = \"group/sub/app\"\n\n"),
        "{text}"
    );
    assert!(
        out.contains("on GitLab a first run takes only a way in that reads its posts back"),
        "{out}"
    );
    assert!(
        out.contains("Store HERALD_SLACK_TOKEN as a masked CI/CD variable;"),
        "{out}"
    );
}

#[test]
fn every_question_is_asked_where_no_flag_answers_it_and_an_enterprise_host_is_the_api() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), "https://ghe.example/owner/app.git");
    let answers = "github\n\n\nmatrix\n\nhttps://matrix.example\n\n!room:matrix.example\n\ny\n";
    let (done, out) = init(directory.path(), &[], answers, None);
    assert_eq!(done, Ok(crate::DONE), "{out}");
    let text = written(directory.path()).unwrap();
    assert!(
        text.contains("api = \"https://ghe.example/api/v3\""),
        "{text}"
    );
    assert!(
        text.contains("homeserver = \"https://matrix.example\""),
        "{text}"
    );
    assert!(text.contains("room = \"!room:matrix.example\""), "{text}");
    for asked in [
        "  github: GitHub, a GitHub Enterprise Server or a GHE.com tenant\n",
        "The forge the repository is on: ",
        "The repository, as owner/name [owner/app]: ",
        "The forge's API base URL; none for github.com's [https://ghe.example/api/v3]: ",
        "  matrix: ",
        "The chat it announces to [discord]: ",
        "The way into matrix [bot]: ",
        "The homeserver's base URL, such as https://matrix.org: ",
        "The secret that holds the bot's access token [HERALD_MATRIX_TOKEN]: ",
        "  herald:announce-releases: ",
        "The presets it announces with, one per line, an empty line to end, \"none\" for none [herald:announce-releases]: ",
        "Write ",
    ] {
        assert!(out.contains(asked), "{asked}\n{out}");
    }
    assert!(!out.contains("herald:merge-queue-small-team"), "{out}");
}

#[test]
fn a_ghe_com_tenant_is_known_and_its_api_is_its_own_host() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), "https://acme.ghe.com/owner/app.git");
    let (done, out) = init(
        directory.path(),
        &["--chat", "slack", "--announce", "none", "--yes"],
        "",
        Some(key()),
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    let text = written(directory.path()).unwrap();
    assert!(
        text.contains("api = \"https://api.acme.ghe.com\""),
        "{text}"
    );
    assert!(!text.contains("[presets."), "{text}");
}

#[test]
fn without_a_remote_the_forge_and_repository_are_asked_and_api_none_is_the_public_one() {
    let directory = tempfile::tempdir().unwrap();
    let (unanswered, _) = init(directory.path(), &["--yes"], "", None);
    assert_eq!(
        unanswered,
        Err("The forge the repository is on has no default; give --forge".to_owned())
    );
    let (done, out) = init(
        directory.path(),
        &[
            "--forge",
            "forgejo",
            "--repository",
            "me/app",
            "--chat",
            "mastodon",
            "--with",
            "server=https://mastodon.example",
            "--yes",
        ],
        "",
        None,
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    let text = written(directory.path()).unwrap();
    assert!(
        text.contains("kind = \"forgejo\"\nrepository = \"me/app\"\n\n"),
        "{text}"
    );
    assert!(!out.contains("origin"), "{out}");
}

#[test]
fn a_worktree_s_git_file_leads_to_its_repository_s_config() {
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("main");
    git(&main, "https://codeberg.org/me/app.git");
    std::fs::create_dir_all(main.join(".git/worktrees/side")).unwrap();
    std::fs::write(main.join(".git/worktrees/side/commondir"), "../..\n").unwrap();
    let side = directory.path().join("side");
    std::fs::create_dir_all(side.join("sub")).unwrap();
    std::fs::write(
        side.join(".git"),
        format!("gitdir: {}\n", main.join(".git/worktrees/side").display()),
    )
    .unwrap();
    let (done, out) = init(
        &side.join("sub"),
        &[
            "--chat",
            "discord",
            "--variant",
            "bot",
            "--with",
            "channel=1566184281406177531",
            "--yes",
        ],
        "",
        None,
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    assert!(
        out.contains("The repository's origin is me/app on codeberg.org."),
        "{out}"
    );
    assert!(
        written(&side.join("sub"))
            .unwrap()
            .contains("kind = \"forgejo\"\nrepository = \"me/app\"\n\n")
    );
    let module = directory.path().join("module");
    let held = directory.path().join("held");
    git(&held, "https://gitlab.com/group/module.git");
    std::fs::create_dir_all(&module).unwrap();
    std::fs::write(
        module.join(".git"),
        format!("gitdir: {}\n", held.join(".git").display()),
    )
    .unwrap();
    let (done, out) = init(
        &module,
        &[
            "--yes",
            "--print",
            "--chat",
            "matrix",
            "--with",
            "homeserver=https://matrix.org",
            "--with",
            "room=!room:matrix.org",
        ],
        "",
        None,
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    assert!(
        out.contains("The repository's origin is group/module on gitlab.com."),
        "{out}"
    );
}

#[test]
fn a_wrong_answer_given_as_a_flag_stops_with_why_and_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), "https://github.com/owner/app.git");
    for (args, why) in [
        (
            &["--chat", "slack", "--with", "room=x", "--yes"][..],
            "--with: an unknown key or no KEY=VALUE; the slack webhook asks webhook, domain",
        ),
        (
            &["--chat", "slack", "--with", "webhook", "--yes"][..],
            "--with: an unknown key or no KEY=VALUE; the slack webhook asks webhook, domain",
        ),
        (
            &[
                "--chat",
                "slack",
                "--with",
                "webhook=https://hooks.slack.com/x",
                "--yes",
            ][..],
            "--with webhook: name the secret in capitals, digits and underscores, such as HERALD_SLACK_WEBHOOK; its value is never asked",
        ),
        (
            &["--chat", "slack", "--variant", "bot", "--yes"][..],
            "The id of the channel the bot has joined, such as C0123456789 has no default; give --with channel",
        ),
        (
            &[
                "--chat",
                "slack",
                "--announce",
                "herald:merge-queue-small-team",
                "--yes",
            ][..],
            "--announce: a preset a first run does not adopt; it adopts herald:announce-releases, herald:review-reminders, herald:security-alerts-private",
        ),
        (
            &[
                "--chat",
                "slack",
                "--variant",
                "bot",
                "--with",
                "channel=C1",
                "--store-token",
                "HERALD_PUSH",
                "--yes",
            ][..],
            "--store-token names the token of a Forgejo repository's records branch, which herald picks for a chat way in that cannot read its posts back on Forgejo alone; leave it out",
        ),
        (
            &["--repository", "app", "--yes"][..],
            "--repository: give the repository as owner/name",
        ),
        (
            &["--api", "http://ghe.example", "--yes"][..],
            "--api: give the API's https:// base URL, or none for github.com's",
        ),
        (
            &["--chat", "slack", "--variant", "carrier", "--yes"][..],
            "--variant: answer one of webhook, bot",
        ),
        (
            &[
                "--chat",
                "slack",
                "--with",
                "domain=slack.com",
                "--with",
                "domain=slack-gov.com",
                "--yes",
            ][..],
            "--with domain: give it once",
        ),
        (
            &["--chat", "no-such-chat", "--yes"][..],
            "--chat: answer one of discord, mastodon, matrix, slack, teams",
        ),
    ] {
        let (done, _) = init(directory.path(), args, "", Some(key()));
        assert_eq!(done, Err(why.to_owned()), "{args:?}");
        assert_eq!(written(directory.path()), None, "{args:?}");
    }
}

#[test]
fn a_declined_write_or_a_failed_generator_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), "https://github.com/owner/app.git");
    let args = [
        "--chat",
        "slack",
        "--variant",
        "webhook",
        "--with",
        "webhook=HERALD_SLACK_WEBHOOK",
        "--with",
        "domain=slack.com",
        "--announce",
        "herald:announce-releases",
    ];
    let (declined, out) = init(directory.path(), &args, "\n\n\nn\n", Some(key()));
    assert_eq!(declined, Ok(crate::STOPPED), "{out}");
    assert!(out.ends_with("Nothing was written.\n"), "{out}");
    assert!(!out.contains(&key()), "{out}");
    assert_eq!(written(directory.path()), None);
    let (failed, out) = init(directory.path(), &args, "\n\n\ny\n", None);
    assert_eq!(
        failed,
        Err(
            "the system's random generator failed, so no store key was made; nothing was written"
                .to_owned()
        ),
        "{out}"
    );
    assert_eq!(written(directory.path()), None);
}

#[test]
fn a_candidate_that_does_not_load_is_not_written() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), "https://github.com/owner/app.git");
    let (done, _) = init(
        directory.path(),
        &[
            "--chat",
            "discord",
            "--variant",
            "bot",
            "--with",
            "channel=abc",
            "--yes",
        ],
        "",
        None,
    );
    let why = done.unwrap_err();
    assert!(why.contains("\"abc\" is not a channel id"), "{why}");
    assert!(why.ends_with("\nNothing was written."), "{why}");
    assert_eq!(written(directory.path()), None);
}

#[test]
fn every_way_in_a_first_run_offers_has_its_section_in_the_guide() {
    let guide = include_str!("../../../../.docs/first-run.md");
    let mut anchors = vec!["### `forgejo-branch`".to_owned()];
    for run in herald_compose::kinds().first_runs() {
        for variant in run.variants {
            anchors.push(format!("### `{}-{}`", run.kind, variant.name));
        }
    }
    for anchor in anchors {
        assert_eq!(guide.matches(&format!("{anchor}\n")).count(), 1, "{anchor}");
    }
}

/// A Slack webhook's URL, as a secret pasted in the wrong place is, built
/// at run time.
fn pasted() -> String {
    format!(
        "https://hooks.slack.com/services/T{}/B{}/{}",
        "0".repeat(8),
        "1".repeat(10),
        "x".repeat(24)
    )
}

#[test]
fn no_refusal_echoes_what_was_given_on_the_command_line() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), "https://github.com/owner/app.git");
    let secret = pasted();
    for args in [
        vec!["--with".to_owned(), format!("webhook:{secret}")],
        vec!["--with".to_owned(), format!("{secret}=HERALD_X")],
        vec!["--with".to_owned(), format!("webhook={secret}")],
        vec!["--announce".to_owned(), secret.clone()],
        vec!["--repository".to_owned(), secret.clone()],
        vec!["--api".to_owned(), format!("http{}", &secret[5..])],
        vec!["--forge".to_owned(), secret.clone()],
        vec!["--variant".to_owned(), secret.clone()],
        vec!["--store-token".to_owned(), secret.clone()],
    ] {
        let mut all = vec!["--chat", "slack", "--yes"];
        all.extend(args.iter().map(String::as_str));
        let (done, out) = init(directory.path(), &all, "", Some(key()));
        let why = done.unwrap_err();
        assert!(!why.contains(&secret[8..]), "{args:?}: {why}");
        assert!(!out.contains(&secret[8..]), "{args:?}: {out}");
        assert_eq!(written(directory.path()), None, "{args:?}");
    }
}

#[test]
fn a_credential_in_the_remote_s_url_is_never_said() {
    let directory = tempfile::tempdir().unwrap();
    let token = "y".repeat(40);
    git(
        directory.path(),
        &format!("https://owner:{token}@github.com/owner/app.git"),
    );
    let (done, out) = init(
        directory.path(),
        &["--chat", "slack", "--yes"],
        "",
        Some(key()),
    );
    assert_eq!(done, Ok(crate::DONE), "{out}");
    assert!(!out.contains(&token), "{out}");
    assert!(!written(directory.path()).unwrap().contains(&token));
}

#[test]
fn the_cli_runs_init_and_says_why_it_stopped_on_standard_error() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("herald.toml").display().to_string();
    let run = |extra: &[&str]| {
        let mut args = vec!["herald", "--config", &config, "init", "--yes", "--print"];
        args.extend(extra);
        let args = args.into_iter().map(ToOwned::to_owned).collect::<Vec<_>>();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = on_tokio(crate::run(
            &args,
            &std::collections::BTreeMap::new(),
            &crate::Connector::new(&|_, _| Err(String::new()), &|_, _| Err(String::new())),
            &mut out,
            &mut err,
        ));
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };
    let (code, out, err) = run(&[
        "--forge",
        "github",
        "--repository",
        "owner/app",
        "--chat",
        "slack",
    ]);
    assert_eq!(code, crate::DONE, "{err}");
    assert!(out.ends_with(&format!("{SCHEMA}{SLACK_WEBHOOK}")), "{out}");
    let (code, _, err) = run(&["--forge", "teams"]);
    assert_eq!(code, crate::STOPPED);
    assert_eq!(err, "--forge: answer one of github, gitlab, forgejo\n");
}
