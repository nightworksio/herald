use serde_json::{Value, json};

use super::world::*;
use crate::*;

mod forge;
mod refused;
mod updated;

/// The config's path in `setup`.
fn path_of(setup: &Setup) -> String {
    setup
        .directory
        .path()
        .join("herald.toml")
        .display()
        .to_string()
}

/// The config's text in `setup`.
fn config_of(setup: &Setup) -> String {
    std::fs::read_to_string(path_of(setup)).unwrap()
}

/// GitHub's rules of `main`, a merge queue among them where `queued`.
fn ruled(world: &World, queued: bool) {
    let rules: Value = if queued {
        json!([{"type": "merge_queue", "parameters": {}}])
    } else {
        json!([])
    };
    answered(
        world,
        [
            ("/repos/cli/cli", json!({"private": false})),
            (
                "/repos/cli/cli/branches/main",
                json!({"commit": {"sha": "b1"}, "protected": false}),
            ),
            (
                "/repos/cli/cli/rules/branches/main?per_page=100&page=1",
                rules,
            ),
        ],
    );
}

/// The forge answering each of `answers`' paths with its body.
fn answered<const N: usize>(world: &World, answers: [(&str, Value); N]) {
    let forge = &mut world.state.lock().unwrap().forge;
    for (path, body) in answers {
        forge.insert(path.to_owned(), (200, body.to_string()));
    }
}

/// [`CONFIG`] with its source on the GitHub Enterprise Server at
/// ghe.example.
fn enterprise() -> String {
    CONFIG.replace(
        "repository = \"cli/cli\"\n",
        "repository = \"cli/cli\"\napi = \"https://ghe.example/api/v3\"\n",
    )
}

/// `herald setup merge-queue` as `args` asks, the operator answering
/// `answer`: its code or why it stopped, and what it printed.
fn wizard(
    setup: &Setup,
    world: &World,
    args: &[&str],
    answer: &str,
) -> (Result<u8, String>, String) {
    let path = path_of(setup);
    let config = crate::read(&path).unwrap();
    let mut all = vec!["herald", "setup", "merge-queue"];
    all.extend(args);
    let crate::cli::Command::Read(Reading::Setup(crate::cli::SettingUp::MergeQueue(asked))) =
        crate::cli::Cli::try_parse_from(all).unwrap().command
    else {
        panic!("not the merge queue's setup");
    };
    let connect =
        |_: herald_ports::Base<'_>, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let waiting = |_: &str, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let connector = Connector::new(&connect, &waiting);
    let mut out = Vec::new();
    let mut answers = answer.as_bytes();
    let done = on_tokio(crate::queue_setup::setting_up(
        &config,
        &path,
        (&setup.vars, &connector),
        &asked,
        &mut answers,
        &mut out,
    ));
    (done, String::from_utf8(out).unwrap())
}

/// The GitHub App `merger`, as the config names it.
const MERGER: &str = "[github_apps.merger]\nclient_id = \"Iv23liABCDEF\"\nprivate_key = { secret = \"HERALD_APP_KEY\" }\n";

/// The table every default writes for `main` of cli/cli, where the forge
/// has no queue on it.
const DEFAULTS: &str = "[merge_queues.cli-main]
source = \"cli\"
branch = \"main\"
runs = \"herald\"
enter = { label = \"herald:merge\" }
least = \"write\"
vouches = \"gesture\"
skew = \"5s\"
method = \"squash\"
message = \"forge\"
wait = \"2h\"
updated = \"signed\"
update_wait = \"10s\"
applied_by = \"command\"
write = { token = { secret = \"HERALD_GITHUB_MERGE_TOKEN\" } }
";

/// Doctor's words on `setup`'s config.
fn doctored(setup: &Setup, world: &World) -> String {
    herald(setup, world, &["doctor", "--layer", "actions"]).out
}

#[test]
fn each_question_s_default_is_taken_by_an_empty_answer_and_the_table_loads_and_starts() {
    let setup = setup(CONFIG);
    let world = World::default();
    ruled(&world, false);
    let answers = "\nmain\n\n\n\n\n\n\n\n\nsquash\n\n\n\n\n\n\n\n\n\ny\n";
    let (done, out) = wizard(&setup, &world, &[], answers);
    assert_eq!(done, Ok(DONE), "{out}");
    assert_eq!(config_of(&setup), format!("{CONFIG}\n{DEFAULTS}"));
    for secret in setup.vars.values().filter(|secret| secret.len() >= 8) {
        assert!(!out.contains(secret.as_str()), "a secret's value in {out}");
        assert!(
            !DEFAULTS.contains(secret.as_str()),
            "a secret's value written"
        );
    }
    for said in [
        "The forge has no merge queue on main: no ruleset on the branch requires a merge queue; add one that does.",
        "runs = \"herald\" is offered",
        "Who runs it [herald]: ",
        "  seen: The head herald first reads with the gesture standing: a push between the gesture and that read is vouched for as if the writer had seen it.",
        "How it merges: ",
        "  never: It never does: the pull request leaves, and a new gesture queues it.",
        "made from the head it vouched for [signed]: ",
        "The committer https://api.github.com signs the commits it makes for, which you vouch is the instance's own and no user's; none for noreply@github.com [none]: ",
        "up to 1m; 0s reads it once [10s]: ",
        "this question has no default; give an answer",
        "Store its write token as the secret HERALD_GITHUB_MERGE_TOKEN",
        "herald --config ",
    ] {
        assert!(out.contains(said), "{said} in {out}");
    }
    assert!(!out.contains("fast-forward"), "{out}");
    assert!(!out.contains("never lets a gesture vouch"), "{out}");
    let doctor = doctored(&setup, &world);
    assert!(
        doctor.contains("The merge queue cli-main merges into main"),
        "{doctor}"
    );
    assert!(!doctor.contains("does not start"), "{doctor}");
}

#[test]
fn every_alternative_is_written_as_its_flag_answers_it() {
    let setup = setup(CONFIG);
    let world = World::default();
    ruled(&world, true);
    let flags = [
        "app",
        "--branch",
        "main",
        "--runs",
        "herald",
        "--comment",
        "/merge",
        "--least",
        "maintain",
        "--vouches",
        "seen",
        "--skew",
        "10s",
        "--method",
        "merge",
        "--message",
        "pull",
        "--may-fail",
        "lint",
        "--may-fail",
        "test (ubuntu, 1)",
        "--wait",
        "30m",
        "--retry",
        "test",
        "--retry-times",
        "3",
        "--updated",
        "signed",
        "--signer",
        "web-flow@cli.example",
        "--update-wait",
        "0ms",
        "--applied-by",
        "server",
        "--write-token",
        "HERALD_MERGER",
        "--source",
        "cli",
        "--yes",
    ];
    let (done, out) = wizard(&setup, &world, &flags, "");
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(
        out.contains("The forge has a merge queue on main."),
        "{out}"
    );
    assert!(config_of(&setup).ends_with(
        "[merge_queues.app]
source = \"cli\"
branch = \"main\"
runs = \"herald\"
enter = { comment = \"/merge\" }
least = \"maintain\"
vouches = \"seen\"
skew = \"10s\"
method = \"merge\"
message = \"pull\"
may_fail = [\"lint\", \"test (ubuntu, 1)\"]
wait = \"30m\"
updated = \"signed\"
signer = \"web-flow@cli.example\"
update_wait = \"0s\"
retries = { checks = [\"test\"], times = 3 }
applied_by = \"server\"
write = { token = { secret = \"HERALD_MERGER\" } }
"
    ));
}

#[test]
fn the_forge_s_queue_is_chosen_where_it_has_one_and_herald_s_keys_are_not_asked() {
    let setup = setup(CONFIG);
    let world = World::default();
    ruled(&world, true);
    let flags = [
        "--branch",
        "main",
        "--vouches",
        "approved",
        "--method",
        "rebase",
        "--role-lost",
        "stay",
        "--source",
        "cli",
        "--yes",
    ];
    let (done, out) = wizard(&setup, &world, &flags, "");
    assert_eq!(done, Ok(DONE), "{out}");
    let written = config_of(&setup);
    for key in [
        "runs = \"forge\"",
        "vouches = \"approved\"",
        "method = \"rebase\"",
        "role_lost = \"stay\"\napplied_by",
    ] {
        assert!(written.contains(key), "{key} in {written}");
    }
    for key in [
        "message",
        "wait",
        "may_fail",
        "retries",
        "updated",
        "signer",
        "update_wait",
    ] {
        assert!(!written.contains(&format!("{key} =")), "{key} in {written}");
    }
    let (refused, _) = wizard(
        &setup,
        &world,
        &[
            "--branch", "dev", "--method", "squash", "--wait", "1h", "--runs", "forge", "--yes",
        ],
        "",
    );
    assert!(refused.unwrap_err().contains("herald's own queue alone"));
}

#[test]
fn a_refused_answer_is_asked_again_and_never_repeated() {
    let setup = setup(CONFIG);
    let world = World::default();
    ruled(&world, false);
    let answers = format!(
        "\n\nmain\nMain Queue\n\nsometimes\n\n\n\n\n\nsoon\n\nfast-forward\nsquash\n\n\n\nmaybe\n\n\n2m\n\n\nwhenever\n\n{}\nHERALD_MERGER\nn\n",
        ["ghp", "abcdefghijklmnopqrstuvwxyz0123456789"].join("_")
    );
    let (done, out) = wizard(&setup, &world, &[], &answers);
    assert_eq!(done, Ok(STOPPED), "{out}");
    for said in [
        "this question has no default; give an answer",
        "a name is lowercase letters, digits and inner '-'",
        "answer one of forge, herald",
        "answer a number above 0 and its unit",
        "answer one of merge, squash, rebase, repository",
        "answer one of signed, never, approved",
        "update_wait is at most \"1m\"; shorten it",
        "answer one of command, server",
        "what was typed is not repeated",
        "Nothing was written.",
    ] {
        assert!(out.contains(said), "{said} in {out}");
    }
    for typed in [
        "ghp_",
        "sometimes",
        "Main Queue",
        "whenever",
        "soon",
        "maybe",
    ] {
        assert_eq!(out.matches(typed).count(), 0, "{typed} in {out}");
    }
    assert_eq!(config_of(&setup), CONFIG);
}

#[test]
fn no_credential_value_enters_the_config() {
    let setup = setup(CONFIG);
    let world = World::default();
    ruled(&world, false);
    for value in [
        &["ghp", "abcdefghijklmnopqrstuvwxyz0123456789"].join("_"),
        &["glpat", "abcdefghij0123456789"].join("-"),
        "0123456789abcdef0123456789abcdef01234567",
        "1PASSWORD",
        &"A".repeat(65),
        "",
    ] {
        let flags = [
            "--branch",
            "main",
            "--method",
            "squash",
            "--write-token",
            value,
            "--runs",
            "herald",
            "--yes",
        ];
        let (done, out) = wizard(&setup, &world, &flags, "");
        let why = done.unwrap_err();
        assert!(why.starts_with("--write-token: answer the name"), "{why}");
        if !value.is_empty() {
            assert!(!why.contains(value) && !out.contains(value), "{why} {out}");
        }
        assert_eq!(config_of(&setup), CONFIG);
    }
}

#[test]
fn an_existing_queue_is_edited_in_place_not_written_twice() {
    let queue = "[merge_queues.app]\nsource = \"cli\"\nbranch = \"main\"\nruns = \"herald\"\nmethod = \"squash\"\nmay_fail = [\"lint\"]\nwrite = { token = { secret = \"HERALD_MERGER\" } }\n";
    let after = "[cli]\nprivate = false\n";
    let setup = setup(&format!("{CONFIG}{queue}{after}"));
    let world = World::default();
    ruled(&world, true);
    let (done, out) = wizard(
        &setup,
        &world,
        &["--branch", "main", "--vouches", "seen", "--yes"],
        "",
    );
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(
        out.contains("The merge queue app merges into main of cli/cli already; it is edited."),
        "{out}"
    );
    let written = config_of(&setup);
    assert_eq!(written.matches("[merge_queues.").count(), 1, "{written}");
    assert!(
        written.starts_with(CONFIG) && written.ends_with(after),
        "{written}"
    );
    for kept in [
        "runs = \"herald\"",
        "method = \"squash\"",
        "may_fail = [\"lint\"]",
        "vouches = \"seen\"",
        "secret = \"HERALD_MERGER\"",
    ] {
        assert!(written.contains(kept), "{kept} in {written}");
    }
    let (again, _) = wizard(&setup, &world, &["app", "--yes"], "");
    assert_eq!(again, Ok(DONE));
    assert_eq!(config_of(&setup), written);
    let (other, _) = wizard(&setup, &world, &["dev", "--branch", "main", "--yes"], "");
    assert_eq!(other, Ok(DONE));
    assert_eq!(config_of(&setup), written);
    let (taken, out) = wizard(
        &setup,
        &world,
        &["--branch", "dev", "--method", "merge"],
        "\napp\n",
    );
    assert!(taken.unwrap_err().contains("answers ended"));
    assert!(out.contains("merges into another branch"), "{out}");
    let (kept, out) = wizard(
        &setup,
        &world,
        &["app", "--vouches", "seen"],
        &"\n".repeat(30),
    );
    assert_eq!(kept, Ok(STOPPED), "{out}");
    assert!(out.contains("How it merges [squash]: "), "{out}");
    assert!(out.contains("may_fail = [\"lint\"]"), "{out}");
    assert!(!out.contains("  seen: "), "{out}");
    assert_eq!(config_of(&setup), written);
    let (cleared, out) = wizard(&setup, &world, &["app", "--may-fail", "none", "--yes"], "");
    assert_eq!(cleared, Ok(DONE), "{out}");
    let cleared = config_of(&setup);
    assert!(!cleared.contains("may_fail"), "{cleared}");
    assert!(cleared.contains("vouches = \"seen\""), "{cleared}");
}

#[test]
fn the_command_line_runs_the_wizard_without_saying_the_choices_under_yes() {
    let setup = setup(CONFIG);
    let world = World::default();
    let run = herald(
        &setup,
        &world,
        &[
            "setup",
            "merge-queue",
            "--branch",
            "main",
            "--method",
            "squash",
            "--runs",
            "herald",
            "--yes",
            "--print",
        ],
    );
    assert_eq!(run.code, DONE, "{}", run.err);
    assert!(run.out.contains("[merge_queues.cli-main]"), "{}", run.out);
    assert!(!run.out.contains("  seen: "), "{}", run.out);
    assert_eq!(config_of(&setup), CONFIG);
}

#[test]
fn a_forge_not_read_leaves_the_forge_s_queue_the_default_where_asked() {
    let setup = setup(CONFIG);
    let world = World::default();
    let (done, out) = wizard(&setup, &world, &[], "\nmain\n\n");
    assert!(done.unwrap_err().contains("answers ended"), "{out}");
    assert!(out.contains("main was not read"), "{out}");
    assert!(out.contains("Who runs it [forge]: "), "{out}");
}

#[test]
fn a_secret_s_name_of_the_most_characters_with_digits_is_written() {
    let setup = setup(CONFIG);
    let world = World::default();
    let name = format!("H{}", "2".repeat(63));
    let flags = [
        "--branch",
        "main",
        "--method",
        "squash",
        "--runs",
        "herald",
        "--write-token",
        &name,
        "--yes",
    ];
    let (done, out) = wizard(&setup, &world, &flags, "");
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(config_of(&setup).contains(&format!("secret = \"{name}\"")));
}

#[test]
fn where_the_forge_has_no_queue_herald_s_own_is_offered() {
    let forgejo = CONFIG.replace(
        "kind = \"github\"\nrepository = \"cli/cli\"",
        "kind = \"forgejo\"\nrepository = \"forgejo/forgejo\"",
    );
    let setup = setup(&forgejo);
    let world = World::default();
    let flags = ["--branch", "main", "--method", "merge", "--yes"];
    let (done, out) = wizard(&setup, &world, &flags, "");
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(
        out.contains("The forge was not asked about main: under --yes"),
        "{out}"
    );
    assert!(out.contains("runs = \"herald\" is offered"), "{out}");
    let written = config_of(&setup);
    assert!(
        written.contains("runs = \"herald\"") && !written.contains("retries"),
        "{written}"
    );
    assert!(written.contains("HERALD_FORGEJO_MERGE_TOKEN"), "{written}");
    let retried = [
        "--branch", "dev", "--method", "merge", "--retry", "test", "--yes",
    ];
    let (refused, _) = wizard(&setup, &world, &retried, "");
    assert!(refused.unwrap_err().contains("Forgejo"));
    let times = [
        "--branch",
        "dev",
        "--method",
        "merge",
        "--retry-times",
        "2",
        "--yes",
    ];
    let (refused, _) = wizard(&setup, &world, &times, "");
    assert!(refused.unwrap_err().contains("Forgejo"));
    let apps = super::world::setup(&format!("{forgejo}{MERGER}"));
    let by_app = [
        "--branch",
        "dev",
        "--method",
        "merge",
        "--write-app",
        "merger",
        "--yes",
    ];
    for appless in [&setup, &apps] {
        let (refused, _) = wizard(appless, &world, &by_app, "");
        assert!(
            refused
                .unwrap_err()
                .contains("a GitHub App writes on a GitHub source")
        );
    }
}

#[test]
fn an_app_writes_where_the_source_is_github() {
    let apps = format!(
        "{MERGER}[github_apps.other]\nclient_id = \"Iv23liGHIJKL\"\nprivate_key = {{ secret = \"HERALD_APP_KEY\" }}\n"
    );
    let setup = setup(&format!("{CONFIG}{apps}"));
    let world = World::default();
    ruled(&world, true);
    let answers = "\nmain\n\n\n\n\n\n\n\nmerge\n\n\n\napp\n\nmerger\n42\ny\n";
    let (done, out) = wizard(&setup, &world, &[], answers);
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(config_of(&setup).contains("write = { app = \"merger\", installation = 42 }"));
    assert!(!out.contains("Store its write token"), "{out}");
    let flags = [
        "--branch",
        "dev",
        "--method",
        "merge",
        "--write-app",
        "merger",
        "--installation",
        "none",
        "--runs",
        "forge",
        "--yes",
    ];
    let (done, _) = wizard(&setup, &world, &flags, "");
    assert_eq!(done, Ok(DONE));
    assert!(config_of(&setup).contains("write = { app = \"merger\" }"));
    let (kept, out) = wizard(&setup, &world, &["cli-main", "--yes"], "");
    assert_eq!(kept, Ok(DONE), "{out}");
    assert!(config_of(&setup).contains("write = { app = \"merger\", installation = 42 }"));
}

#[test]
fn a_source_s_one_app_is_the_default_writer() {
    let setup = setup(&format!("{CONFIG}{MERGER}"));
    let world = World::default();
    ruled(&world, true);
    let answers = "\nmain\n\n\n\n\n\n\n\nmerge\n\n\n\napp\n\n\ny\n";
    let (done, out) = wizard(&setup, &world, &[], answers);
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(out.contains("(merger) [merger]: "), "{out}");
    assert!(config_of(&setup).contains("write = { app = \"merger\" }"));
}

#[test]
fn a_printed_table_writes_nothing() {
    let setup = setup(CONFIG);
    let world = World::default();
    let flags = [
        "--branch",
        "main",
        "--method",
        "squash",
        "--source",
        "cli",
        "--runs",
        "forge",
        "--yes",
        "--print",
        "--private",
    ];
    let (done, out) = wizard(&setup, &world, &flags, "");
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(
        out.ends_with("write = { token = { secret = \"HERALD_GITHUB_MERGE_TOKEN\" } }\n"),
        "{out}"
    );
    assert!(out.contains("main was not read: "), "{out}");
    assert_eq!(config_of(&setup), CONFIG);
}

#[test]
fn without_a_forge_source_or_an_answer_nothing_is_written() {
    let world = World::default();
    let forgeless = setup(
        &CONFIG
            .replace(
                "kind = \"github\"\nrepository = \"cli/cli\"",
                "kind = \"feed\"\nurl = \"https://blog.example/feed.xml\"",
            )
            .replace(" {{ event.attributes.version }}", ""),
    );
    let (done, _) = wizard(&forgeless, &world, &[], "");
    assert!(done.unwrap_err().starts_with("no source of kind github"));
    let setup = setup(CONFIG);
    let (unanswered, _) = wizard(&setup, &world, &[], "\n");
    assert!(unanswered.unwrap_err().contains("answers ended"));
    let (runless, _) = wizard(&setup, &world, &["--branch", "main", "--yes"], "");
    assert_eq!(
        runless.unwrap_err(),
        "Who runs it has no default; give --runs"
    );
    let (methodless, _) = wizard(
        &setup,
        &world,
        &["--branch", "main", "--runs", "forge", "--yes"],
        "",
    );
    assert_eq!(
        methodless.unwrap_err(),
        "How it merges has no default; give --method"
    );
    assert_eq!(config_of(&setup), CONFIG);
}

#[test]
fn a_branch_whose_source_s_installation_token_is_not_minted_is_not_read() {
    let config = CONFIG.replace(
        "repository = \"cli/cli\"\n",
        "repository = \"cli/cli\"\napp = \"herald\"\n",
    ) + "[github_apps.herald]\nclient_id = \"Iv23liABCDEF\"\nprivate_key = { secret = \"HERALD_APP_KEY\" }\n";
    let mut minting = setup(&config);
    minting
        .vars
        .insert("HERALD_APP_KEY".to_owned(), super::activitypub::pkcs8());
    let world = World::default();
    ruled(&world, true);
    let flags = [
        "--branch",
        "main",
        "--method",
        "squash",
        "--source",
        "cli",
        "--runs",
        "forge",
        "--yes",
        "--print",
        "--private",
    ];
    let (done, out) = wizard(&minting, &world, &flags, "");
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(
        out.contains("main was not read: its source's installation token could not be minted"),
        "{out}"
    );
}

/// Two queues on cli/cli: app on main, and dev on dev queued by a comment.
const TWO: &str = "[merge_queues.app]\nsource = \"cli\"\nbranch = \"main\"\nruns = \"herald\"\nmethod = \"squash\"\nmay_fail = [\"lint\"]\nwrite = { token = { secret = \"HERALD_MERGER\" } }\n[merge_queues.dev]\nsource = \"cli\"\nbranch = \"dev\"\nruns = \"herald\"\nenter = { comment = \"/merge\" }\nmethod = \"merge\"\nwrite = { token = { secret = \"HERALD_MERGER\" } }\n";

#[test]
fn a_queue_is_edited_with_its_own_answers_and_never_moved_onto_another_s_branch() {
    let setup = setup(&format!("{CONFIG}{TWO}"));
    let world = World::default();
    ruled(&world, true);
    let (moved, _) = wizard(&setup, &world, &["dev", "--branch", "main", "--yes"], "");
    assert!(
        moved
            .unwrap_err()
            .starts_with("the merge queue \"app\" merges into main of cli/cli already")
    );
    let (kept, out) = wizard(&setup, &world, &["dev", "--yes"], "");
    assert_eq!(kept, Ok(DONE), "{out}");
    assert!(config_of(&setup).contains("enter = { comment = \"/merge\" }"));
    let flags = [
        "app",
        "--source",
        "cli",
        "--branch",
        "main",
        "--runs",
        "herald",
        "--label",
        "herald:merge",
        "--least",
        "write",
        "--vouches",
        "seen",
        "--skew",
        "5s",
        "--method",
        "squash",
        "--message",
        "forge",
        "--wait",
        "2h",
        "--applied-by",
        "command",
        "--write-token",
        "HERALD_MERGER",
        "--print",
    ];
    let (listed, out) = wizard(&setup, &world, &flags, "build\ntest (a, b)\n\n\n\n\nnone\n");
    assert_eq!(listed, Ok(DONE), "{out}");
    assert!(out.contains("[lint]: "), "{out}");
    assert!(
        out.contains("may_fail = [\"build\", \"test (a, b)\"]"),
        "{out}"
    );
    assert!(!out.contains("retries"), "{out}");
}

#[test]
fn an_owner_s_repository_is_asked_and_a_forge_not_read_says_so_only_where_printed() {
    let group = "[sources.group]\nkind = \"gitlab\"\nowner = \"group\"\ntoken = { secret = \"HERALD_GITLAB_TOKEN\" }\n";
    let setup = setup(&format!("{CONFIG}{group}"));
    let world = World::default();
    let flags = [
        "--runs",
        "herald",
        "--label",
        "herald:merge",
        "--least",
        "write",
        "--vouches",
        "gesture",
        "--skew",
        "5s",
        "--method",
        "merge",
        "--message",
        "forge",
        "--may-fail",
        "none",
        "--wait",
        "2h",
        "--retry",
        "none",
        "--updated",
        "approved",
        "--applied-by",
        "command",
        "--write-token",
        "HERALD_GITLAB_MERGE",
        "--print",
    ];
    let (done, out) = wizard(
        &setup,
        &world,
        &flags,
        "group\nother/app\ngroup/app\nmain\n\n",
    );
    assert_eq!(done, Ok(DONE), "{out}");
    for said in [
        "The source whose repository it merges in (cli, group): ",
        "\"other/app\" is no repository of the owner group",
        "main was not read; --private, or [cli] private, prints why.",
        "GitLab's rebase rewrites the commits",
        "wait = \"2h\"\nupdated = \"approved\"\napplied_by",
        "[merge_queues.group-main]\nsource = \"group\"\nrepository = \"group/app\"\nbranch = \"main\"\n",
    ] {
        assert!(out.contains(said), "{said} in {out}");
    }
    assert!(
        !out.contains("may_fail") && !out.contains("retries"),
        "{out}"
    );
}

#[test]
fn a_branch_under_classic_protection_is_said_unknown() {
    let setup = setup(CONFIG);
    let world = World::default();
    ruled(&world, false);
    let protected = json!({
        "commit": {"sha": "b1"},
        "protected": true,
        "protection": {"enabled": true, "required_status_checks": {"contexts": [], "checks": []}}
    });
    world.state.lock().unwrap().forge.insert(
        "/repos/cli/cli/branches/main".to_owned(),
        (200, protected.to_string()),
    );
    let flags = [
        "--source", "cli", "--branch", "main", "--runs", "forge", "--method", "squash", "--yes",
        "--print",
    ];
    let (done, out) = wizard(&setup, &world, &flags, "");
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(
        out.contains("The forge does not say whether main has a merge queue."),
        "{out}"
    );
}
