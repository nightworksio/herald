use herald_core::Permission;
use herald_core::merging::{
    AppliedBy, Enter, Message, Method, RoleLost, Runs, SKEW, UPDATE_PAUSE, UPDATE_WAIT,
    UPDATE_WAIT_MOST, Update, Updated, Vouches, WAIT,
};
use jiff::SignedDuration;

use super::{BRANCHLESS, UNENTERED, UNFORGED_ROLE_LOST, UNMESSAGED, UNNAMED_CHECK, UNRETRIED};
use crate::merging::UNSIGNING;
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
sources = ["app"]
destination = "discord"
channel = "1566200000000000900"
template = "t"
[sources.app]
kind = "github"
repository = "acme/app"
token = { secret = "HERALD_GITHUB_TOKEN" }
[sources.org]
kind = "github"
owner = "acme"
[sources.lab]
kind = "gitlab"
repository = "group/app"
api = "https://gitlab.example/api/v4"
[sources.forge]
kind = "forgejo"
repository = "acme/app"
api = "https://forge.example/api/v1"
[sources.news]
kind = "feed"
url = "https://blog.example/feed.xml"
"#;

const MERGER: &str = "[github_apps.merger]\nclient_id = \"Iv23liMerger\"\nprivate_key = { secret = \"HERALD_MERGER_KEY\" }\n";

const QUEUE: &str = "[merge_queues.main]\nsource = \"app\"\nbranch = \"main\"\n";

fn kinds() -> Kinds {
    Kinds::new()
        .destination::<herald_dest_discord::DiscordKind>()
        .source::<herald_sources_forge::GitHubKind>()
        .source::<herald_sources_forge::GitLabKind>()
        .source::<herald_sources_forge::ForgejoKind>()
        .source::<herald_sources_feed::FeedKind>()
        .store::<herald_store_sqlite::SqliteKind>()
}

fn loaded(rest: &str) -> Result<Config, String> {
    load(
        "herald.toml",
        &format!("{HEAD}{rest}"),
        &|_| Err(String::new()),
        &kinds(),
    )
    .map_err(|errors| errors.to_string())
}

fn refused(rest: &str, said: &str) {
    let error = loaded(rest).expect_err(rest);
    assert!(error.contains(said), "{rest}\n{error}");
}

#[test]
fn a_queue_names_a_source_and_a_branch_and_takes_every_other_key_s_default() {
    let config = loaded(QUEUE).unwrap();
    let queue = &config.merge_queues["main"];
    assert_eq!(queue.source.as_str(), "app");
    assert_eq!(queue.forge.forge, "github");
    assert_eq!(queue.forge.repository, "acme/app");
    assert_eq!(queue.forge.api, "https://api.github.com");
    assert_eq!(queue.forge.update, Update::Merge);
    assert_eq!(queue.branch, "main");
    assert_eq!(queue.runs, Runs::Forge);
    assert_eq!(queue.enter, Enter::Label("herald:merge".to_owned()));
    assert_eq!(queue.least, Permission::Write);
    assert_eq!(queue.vouches, Vouches::Gesture);
    assert_eq!(queue.skew, SKEW);
    assert_eq!(SKEW, SignedDuration::from_secs(5));
    assert_eq!(queue.method, None);
    assert_eq!(queue.message, Message::Forge);
    assert_eq!(queue.may_fail, Vec::<String>::new());
    assert_eq!(queue.wait, WAIT);
    assert_eq!(queue.update_wait, UPDATE_WAIT);
    assert_eq!(UPDATE_WAIT, SignedDuration::from_secs(10));
    assert_eq!(UPDATE_PAUSE, SignedDuration::from_secs(2));
    assert_eq!(queue.retries, None);
    assert_eq!(queue.applied_by, AppliedBy::Command);
    assert_eq!(queue.write, None);
    assert!(loaded("").unwrap().merge_queues.is_empty());
}

#[test]
fn every_key_a_queue_gives_is_kept() {
    let config = loaded(&format!(
        r#"[merge_queues.lab]
source = "lab"
branch = "release"
runs = "herald"
enter = {{ comment = "/merge" }}
least = "maintain"
vouches = "approved"
skew = "30s"
method = "squash"
message = "pull"
may_fail = ["flaky"]
wait = "30m"
retries = {{ checks = ["test"], times = 2 }}
applied_by = "server"
write = {{ token = {{ secret = "HERALD_GITLAB_MERGE" }} }}
[merge_queues.org]
source = "org"
repository = "acme/site"
branch = "main"
method = "rebase"
write = {{ app = "merger", installation = 42 }}
{MERGER}"#
    ))
    .unwrap();
    let lab = &config.merge_queues["lab"];
    assert_eq!(lab.forge.api, "https://gitlab.example/api/v4");
    assert_eq!(lab.forge.update, Update::Rebase);
    assert_eq!(lab.runs, Runs::Herald);
    assert_eq!(lab.enter, Enter::Comment("/merge".to_owned()));
    assert_eq!(lab.least, Permission::Maintain);
    assert_eq!(lab.vouches, Vouches::Approved);
    assert_eq!(config.merge_queues["org"].vouches, Vouches::Gesture);
    assert_eq!(lab.skew, SignedDuration::from_secs(30));
    assert_eq!(lab.method, Some(Method::Squash));
    assert_eq!(lab.message, Message::Pull);
    assert_eq!(lab.updated, Updated::Never);
    assert_eq!(lab.signer, None);
    assert_eq!(lab.may_fail, ["flaky"]);
    assert_eq!(lab.wait, SignedDuration::from_mins(30));
    assert_eq!(
        lab.retries.as_ref().map(|retries| retries.times.get()),
        Some(2)
    );
    assert_eq!(lab.applied_by, AppliedBy::Server);
    let org = &config.merge_queues["org"];
    assert_eq!(org.forge.repository, "acme/site");
    let Some(herald_kinds::WriteCredential::App(installed)) = org.writing() else {
        panic!("{:?}", org.writing());
    };
    assert_eq!(installed.installation, herald_core::Installation::Fixed(42));
    let site = loaded(&format!(
        "[merge_queues.org]\nsource = \"org\"\nrepository = \"acme/site\"\nbranch = \"main\"\nwrite = {{ app = \"merger\" }}\n{MERGER}"
    ))
    .unwrap();
    let Some(herald_kinds::WriteCredential::App(installed)) = site.merge_queues["org"].writing()
    else {
        panic!();
    };
    assert_eq!(
        installed.installation,
        herald_core::Installation::Repository("acme/site")
    );
}

#[test]
fn a_queue_s_source_is_a_forge_s_and_its_repository_the_source_s() {
    refused(
        "[merge_queues.main]\nsource = \"nowhere\"\nbranch = \"main\"\n",
        "\"nowhere\" is no source; the sources a merge queue may name are \"app\", \"forge\", \"lab\", \"org\"",
    );
    refused(
        "[merge_queues.main]\nsource = \"news\"\nbranch = \"main\"\n",
        "\"news\" is no source of kind \"github\", \"gitlab\" or \"forgejo\"",
    );
    refused(
        "[merge_queues.main]\nsource = \"org\"\nbranch = \"main\"\n",
        "merge_queues.main.repository: the source is the owner acme",
    );
    refused(
        "[merge_queues.main]\nsource = \"forge\"\nbranch = \"main\"\nretries = { checks = [\"ci\"], times = 1 }\nruns = \"herald\"\n",
        "merge_queues.main.retries: Forgejo has no API to re-run a job",
    );
}

#[test]
fn one_table_merges_into_a_branch_of_a_repository() {
    refused(
        &format!(
            "{QUEUE}[merge_queues.other]\nsource = \"org\"\nrepository = \"acme/app\"\nbranch = \"main\"\n"
        ),
        "the queue main merges into main of acme/app already; give one table per branch",
    );
    assert_eq!(
        loaded(&format!(
            "{QUEUE}[merge_queues.next]\nsource = \"app\"\nbranch = \"next\"\n[merge_queues.forge]\nsource = \"forge\"\nbranch = \"main\"\n"
        ))
        .unwrap()
        .merge_queues
        .len(),
        3
    );
}

#[test]
fn every_key_that_cannot_hold_is_refused_saying_why() {
    for (rest, key, said) in [
        (
            "[merge_queues.main]\nsource = \"app\"\nbranch = \" \"\n".to_owned(),
            "branch",
            BRANCHLESS,
        ),
        (
            format!("{QUEUE}enter = {{ label = \"\" }}\n"),
            "enter",
            UNENTERED,
        ),
        (
            format!("{QUEUE}enter = {{ comment = \"  \" }}\n"),
            "enter",
            UNENTERED,
        ),
        (
            format!("{QUEUE}runs = \"herald\"\nmay_fail = [\"\"]\n"),
            "may_fail",
            UNNAMED_CHECK,
        ),
        (
            format!("{QUEUE}runs = \"herald\"\nretries = {{ checks = [\" \"], times = 1 }}\n"),
            "retries.checks",
            UNNAMED_CHECK,
        ),
        (
            format!("{QUEUE}runs = \"herald\"\nretries = {{ checks = [], times = 1 }}\n"),
            "retries.checks",
            UNRETRIED,
        ),
        (
            format!("{QUEUE}method = \"rebase\"\nmessage = \"pull\"\n"),
            "message",
            UNMESSAGED,
        ),
        (
            "[merge_queues.main]\nsource = \"lab\"\nbranch = \"main\"\nruns = \"herald\"\nsigner = \"noreply@gitlab.example\"\n".to_owned(),
            "signer",
            UNSIGNING,
        ),
        (
            format!("{QUEUE}runs = \"herald\"\nsigner = \" \"\n"),
            "signer",
            UNSIGNING,
        ),
        (
            format!("{QUEUE}method = \"fast-forward\"\n"),
            "method",
            "GitHub has no fast-forward merge",
        ),
        (
            format!(
                "{QUEUE}write = {{ token = {{ secret = \"A\" }}, app = \"merger\" }}\n{MERGER}"
            ),
            "write",
            "a write credential is a token or a GitHub App, one of them",
        ),
        (
            format!("{QUEUE}runs = \"herald\"\nretries = {{ checks = [\"test\"], times = 0 }}\n"),
            "retries.times",
            "",
        ),
    ] {
        refused(&rest, &format!("merge_queues.main.{key}: {said}"));
    }
    for message in ["merge", "squash"] {
        assert!(
            loaded(&format!(
                "{QUEUE}method = \"{message}\"\nmessage = \"pull\"\n"
            ))
            .is_ok()
        );
    }
    assert!(loaded(&format!("{QUEUE}message = \"pull\"\n")).is_ok());
    assert!(loaded(&format!("{QUEUE}runs = \"herald\"\nmay_fail = [\"flaky\"]\nretries = {{ checks = [\"test\"], times = 1 }}\n")).is_ok());
}

#[test]
fn a_key_of_herald_s_own_queue_is_refused_where_the_forge_runs_it() {
    for (key, value) in [
        ("may_fail", "[\"flaky\"]"),
        ("wait", "\"1h\""),
        ("retries", "{ checks = [\"test\"], times = 1 }"),
        ("update_wait", "\"5s\""),
    ] {
        refused(
            &format!("{QUEUE}{key} = {value}\n"),
            &format!(
                "merge_queues.main.{key}: {key} shapes herald's own queue, and with runs = \"forge\" the forge proves each pull request; give runs = \"herald\", or remove {key}"
            ),
        );
        assert!(loaded(&format!("{QUEUE}runs = \"herald\"\n{key} = {value}\n")).is_ok());
    }
}

#[test]
fn a_lost_role_takes_out_by_default_may_stay_and_is_refused_where_herald_runs_the_queue() {
    let queue =
        |rest: &str| loaded(&format!("{QUEUE}{rest}")).unwrap().merge_queues["main"].role_lost;
    assert_eq!(queue(""), RoleLost::TakeOut);
    assert_eq!(queue("role_lost = \"take-out\"\n"), RoleLost::TakeOut);
    assert_eq!(queue("role_lost = \"stay\"\n"), RoleLost::Stay);
    refused(
        &format!("{QUEUE}runs = \"herald\"\nrole_lost = \"stay\"\n"),
        &format!("merge_queues.main.role_lost: {UNFORGED_ROLE_LOST}"),
    );
    assert!(loaded(&format!("{QUEUE}runs = \"herald\"\n")).is_ok());
}

const CI: &str = "[sources.ci]\nkind = \"github\"\nrepository = \"acme/ci\"\n";

const ON_CI: &str = "[merge_queues.ci]\nsource = \"ci\"\nbranch = \"main\"\n";

#[test]
fn a_queue_s_write_credential_may_be_its_source_s_and_is_else_held_apart_as_a_re_run_s() {
    for apart in [
        format!(
            "{CI}write = {{ token = {{ secret = \"HERALD_WRITE\" }} }}\n{ON_CI}write = {{ token = {{ secret = \"HERALD_WRITE\" }} }}\n"
        ),
        format!(
            "{CI}write = {{ app = \"merger\" }}\n{ON_CI}write = {{ app = \"merger\" }}\n{MERGER}"
        ),
        format!("{CI}{ON_CI}write = {{ token = {{ secret = \"HERALD_MERGE\" }} }}\n"),
        format!(
            "{CI}api = \"https://ghe.example/api/v3\"\n{ON_CI}write = {{ app = \"merger\" }}\n{MERGER}api = \"https://ghe.example/api/v3\"\n"
        ),
        format!(
            "{CI}api = \"https://api.acme.ghe.com\"\n{ON_CI}write = {{ app = \"merger\" }}\n{MERGER}api = \"https://api.acme.ghe.com\"\n"
        ),
    ] {
        assert!(
            loaded(&apart).is_ok(),
            "{apart}: {:?}",
            loaded(&apart).err()
        );
    }
    for (rest, said) in [
        (
            format!("{QUEUE}write = {{ token = {{ secret = \"HERALD_GITHUB_TOKEN\" }} }}\n"),
            "merge_queues.main.write: write.token names the secret HERALD_GITHUB_TOKEN, which the source reads with",
        ),
        (
            format!("{QUEUE}write = {{ token = {{ secret = \"HERALD_DISCORD_TOKEN\" }} }}\n"),
            "merge_queues.main.write: the write token is held in the secret HERALD_DISCORD_TOKEN, which herald also reads",
        ),
        (
            format!("{CI}app = \"merger\"\n{ON_CI}write = {{ app = \"merger\" }}\n{MERGER}"),
            "which the source reads with; the queue's steps are made as another App, with a key of its own",
        ),
        (
            format!(
                "{CI}{ON_CI}write = {{ app = \"merger\" }}\n{MERGER}[sources.other]\nkind = \"github\"\nrepository = \"acme/other\"\napp = \"merger\"\n"
            ),
            "a source reads as the App merger, under this table or another; the queue's steps are made as an App no source reads as, installed with the permissions its steps need alone",
        ),
        (
            format!("{CI}{ON_CI}write = {{ app = \"ghost\" }}\n{MERGER}"),
            "merge_queues.ci.write: no [github_apps.ghost] is given",
        ),
        (
            format!(
                "{CI}{ON_CI}write = {{ app = \"merger\" }}\n{MERGER}api = \"https://ghe.example/api/v3\"\n"
            ),
            "a write credential is sent to the source's own API alone",
        ),
        (
            format!(
                "{CI}write = {{ token = {{ secret = \"HERALD_WRITE\" }} }}\n[merge_queues.lab]\nsource = \"lab\"\nbranch = \"main\"\nwrite = {{ token = {{ secret = \"HERALD_WRITE\" }} }}\n"
            ),
            "which sources.ci.write names at another API or as another credential",
        ),
        (
            format!(
                "{CI}sign_in = {{ client_id = \"Iv23liMerger\" }}\n{ON_CI}write = {{ app = \"merger\" }}\n{MERGER}"
            ),
            "sources.ci.sign_in.client_id: \"Iv23liMerger\" is the client id of a GitHub App a write names",
        ),
    ] {
        refused(&rest, said);
    }
}

#[test]
fn a_queue_says_whether_a_gesture_vouches_for_herald_s_own_update_and_who_signs_it() {
    let config = loaded(&format!(
        "{QUEUE}runs = \"herald\"\nupdated = \"approved\"\nsigner = \"noreply@ghe.example\"\n"
    ))
    .unwrap();
    let main = &config.merge_queues["main"];
    assert_eq!(main.updated, Updated::Approved);
    assert_eq!(main.signer.as_deref(), Some("noreply@ghe.example"));
    for (value, updated) in [("never", Updated::Never), ("signed", Updated::Signed)] {
        let given = format!("{QUEUE}runs = \"herald\"\nupdated = \"{value}\"\n");
        assert_eq!(
            loaded(&given).unwrap().merge_queues["main"].updated,
            updated
        );
    }
    for key in ["updated = \"never\"", "signer = \"noreply@ghe.example\""] {
        refused(&format!("{QUEUE}{key}\n"), "with runs = \"forge\"");
    }
}

#[test]
fn a_signed_update_is_the_default_only_where_herald_can_prove_the_forge_signed_it() {
    let own = "runs = \"herald\"\nmethod = \"merge\"\n";
    let server = "[sources.server]\nkind = \"github\"\nrepository = \"acme/app\"\napi = \"https://ghe.example/api/v3\"\n[sources.tenant]\nkind = \"github\"\nrepository = \"acme/app\"\napi = \"https://api.acme.ghe.com\"\n";
    let signer = "signer = \"noreply@ghe.example\"\n";
    for (source, rest, updated) in [
        ("app", "", Updated::Signed),
        ("server", "", Updated::Never),
        ("server", signer, Updated::Signed),
        ("tenant", "", Updated::Never),
        ("tenant", signer, Updated::Signed),
        ("lab", "", Updated::Never),
        ("forge", "", Updated::Never),
        ("server", "updated = \"signed\"\n", Updated::Signed),
        ("app", "updated = \"approved\"\n", Updated::Approved),
    ] {
        let given = format!(
            "{server}[merge_queues.main]\nsource = \"{source}\"\nbranch = \"main\"\n{own}{rest}"
        );
        assert_eq!(
            loaded(&given).unwrap().merge_queues["main"].updated,
            updated,
            "{source} {rest}"
        );
    }
}

#[test]
fn a_queue_waits_for_its_updated_head_none_at_all_or_up_to_a_minute() {
    for (written, waits) in [
        ("0s", SignedDuration::ZERO),
        ("30s", SignedDuration::from_secs(30)),
        ("1m", UPDATE_WAIT_MOST),
    ] {
        let given = format!("{QUEUE}runs = \"herald\"\nupdate_wait = \"{written}\"\n");
        assert_eq!(
            loaded(&given).unwrap().merge_queues["main"].update_wait,
            waits,
            "{written}"
        );
    }
    assert_eq!(UPDATE_WAIT_MOST, SignedDuration::from_mins(1));
    refused(
        &format!("{QUEUE}runs = \"herald\"\nupdate_wait = \"61s\"\n"),
        "merge_queues.main.update_wait: update_wait is at most \"1m\"; shorten it",
    );
    refused(
        &format!("{QUEUE}runs = \"herald\"\nupdate_wait = \"-1s\"\n"),
        "update_wait",
    );
}
