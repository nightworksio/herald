use herald_core::{Reran, Rerun, SourceName};
use herald_flows::{Applied, Done, Planned, UNPLANNED};
use serde_json::json;

use super::world::*;
use crate::rerunning::{applied_line, planned_lines};
use crate::*;

const LISTED: &str =
    "/repos/cli/cli/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1";

fn ruled() -> String {
    format!(
        "{CONFIG}[views.mine]\nmost_pulls = 0\n[rules.flaky]\nview = \"mine\"\nflaky = {{ rerun = true }}\nevery = \"1h\"\nat = \"00:00\"\n[rules.nudges]\nview = \"mine\"\nstages = [\"1d\"]\nevery = \"1h\"\nat = \"00:00\"\n"
    )
}

fn listed(world: &World) {
    world
        .state
        .lock()
        .unwrap()
        .forge
        .insert(LISTED.to_owned(), (200, json!([]).to_string()));
}

#[test]
fn a_plan_is_listed_by_default_and_an_apply_names_each_job_it_re_runs() {
    let setup = setup(&ruled());
    let world = World::default();
    listed(&world);
    let plan = herald(&setup, &world, &["rerun", "flaky"]);
    assert_eq!(
        (plan.code, plan.out.trim(), plan.err.as_str()),
        (
            DONE,
            "no flaky check is failing on the rule's open pull requests, so nothing is planned",
            ""
        )
    );
    let unknown = herald(&setup, &world, &["rerun", "nope", "--plan"]);
    assert!(
        unknown.err.contains("no rule is named \"nope\""),
        "{}",
        unknown.err
    );
    let nudging = herald(&setup, &world, &["rerun", "nudges"]);
    assert!(nudging.err.contains("plans no re-run"), "{}", nudging.err);
    let unnamed = herald(&setup, &world, &["rerun", "flaky", "--apply"]);
    assert_ne!(unnamed.code, DONE);
    let malformed = herald(
        &setup,
        &world,
        &["rerun", "flaky", "--apply", "--job", "42"],
    );
    assert!(
        malformed.err.contains("\"42\" is not a job"),
        "{}",
        malformed.err
    );
    let unplanned = herald(
        &setup,
        &world,
        &["rerun", "flaky", "--apply", "--job", "42@h1"],
    );
    assert_eq!(
        (unplanned.code, unplanned.out.trim()),
        (DONE, format!("job 42@h1: not re-run: {UNPLANNED}").as_str())
    );
}

#[test]
fn a_plan_and_what_an_apply_did_are_printed_in_plain_words() {
    let planned = |run: Result<Rerun, String>| Planned {
        rule: "flaky".to_owned(),
        source: SourceName::new("cli"),
        repository: "cli/cli".to_owned(),
        number: 7,
        check: "test".to_owned(),
        flips: 2,
        head: "h1".to_owned(),
        run,
    };
    let rerun = Rerun {
        run: "42".to_owned(),
        job: "420".to_owned(),
        page: "https://github.com/cli/cli/actions/runs/42".to_owned(),
        call: "POST https://api.github.com/repos/cli/cli/actions/jobs/420/rerun".to_owned(),
    };
    assert_eq!(
        planned_lines(&planned(Ok(rerun))),
        [
            "flaky cli/cli#7: test flipped 2 times; head h1",
            "  run 42 job 420  https://github.com/cli/cli/actions/runs/42",
            "  call: POST https://api.github.com/repos/cli/cli/actions/jobs/420/rerun",
            "  to apply: herald rerun flaky --apply --job 420@h1",
        ]
    );
    assert_eq!(
        planned_lines(&planned(Err("Forgejo cannot".to_owned())))[1],
        "  nothing to re-run: Forgejo cannot"
    );
    let applied = |done: Done| Applied {
        repository: Some("cli/cli".to_owned()),
        number: Some(7),
        job: "420".to_owned(),
        head: "h1".to_owned(),
        done,
    };
    let lines = [
        Done::Rerun,
        Done::Refused("why".to_owned()),
        Done::Declined(403),
        Done::Unsettled,
        Done::Unrecorded(Reran::Rerun),
    ]
    .map(|done| applied_line(&applied(done)));
    assert_eq!(
        lines[0],
        "cli/cli#7 job 420@h1: re-run: the forge re-runs the job"
    );
    assert_eq!(lines[1], "cli/cli#7 job 420@h1: not re-run: why");
    assert!(lines[2].contains("status 403; a later apply may ask again"));
    assert!(lines[3].starts_with("cli/cli#7 job 420@h1: unsettled: the forge's answer was lost"));
    assert!(lines[4].contains("could not be updated"));
}

#[test]
fn an_apply_signs_nobody_in_where_the_plan_holds_none_of_the_jobs_named() {
    let config = CONFIG.replace(
        "repository = \"cli/cli\"\n",
        "repository = \"cli/cli\"\nwrite = { token = { secret = \"HERALD_RERUN\" } }\nsign_in = { client_id = \"Iv23liClient\" }\n",
    );
    let mut setup = setup(&format!(
        "{config}[views.mine]\nmost_pulls = 0\n[rules.flaky]\nview = \"mine\"\nflaky = {{ rerun = true }}\nevery = \"1h\"\nat = \"00:00\"\n"
    ));
    setup
        .vars
        .insert("HERALD_RERUN".to_owned(), "write-value".to_owned());
    let world = World::default();
    listed(&world);
    let applied = herald(
        &setup,
        &world,
        &["rerun", "flaky", "--apply", "--job", "420@h1"],
    );
    let lines: Vec<&str> = applied.out.lines().collect();
    assert_eq!(
        lines,
        [format!("job 420@h1: not re-run: {UNPLANNED}").as_str()],
        "{}",
        applied.err
    );
    let seen = world.state.lock().unwrap().seen.clone();
    assert!(
        seen.iter().all(|line| !line.contains("/login/")),
        "{seen:?}"
    );
}

#[test]
fn a_line_said_drops_every_control_character_a_forge_could_hand_it() {
    let mut out = Vec::new();
    crate::output::say(
        &mut out,
        "open \u{1b}]52;c;payload\u{7}https://github.com/login/device\u{1b}[2J and\r\nenter \u{9b}31m\u{7f}WDJB\tMJHT",
    );
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "open ]52;c;payloadhttps://github.com/login/device[2J and  enter 31mWDJBMJHT\n"
    );
}

#[test]
fn a_line_said_drops_every_format_character_that_could_make_a_job_s_name_read_otherwise() {
    let planned = Planned {
        rule: "flaky".to_owned(),
        source: SourceName::new("cli"),
        repository: "cli/cli".to_owned(),
        number: 7,
        check: "li\u{202e}n\u{202c}\u{2066}t\u{2069}\u{200b}\u{200f}\u{feff}\u{e0041}\u{e007f}\u{ad}\u{61c}\u{180e}\u{2060}\u{2064}\u{600}"
            .to_owned(),
        flips: 2,
        head: "h1".to_owned(),
        run: Err("Forgejo cannot".to_owned()),
    };
    let mut out = Vec::new();
    for line in planned_lines(&planned) {
        crate::output::say(&mut out, &line);
    }
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "flaky cli/cli#7: lint flipped 2 times; head h1\n  nothing to re-run: Forgejo cannot\n"
    );
    let mut kept = Vec::new();
    crate::output::say(&mut kept, "\u{e9}t\u{e9} \u{5d0}\u{5d1} \u{1f600} \u{2065}");
    assert_eq!(
        String::from_utf8(kept).unwrap(),
        "\u{e9}t\u{e9} \u{5d0}\u{5d1} \u{1f600} \u{2065}\n"
    );
    let mut broken = Vec::new();
    crate::output::say(&mut broken, "a\u{2028}b\u{2029}c");
    assert_eq!(String::from_utf8(broken).unwrap(), "a b c\n");
}

#[test]
fn json_said_escapes_every_character_a_line_drops_and_reads_back_unchanged() {
    let value = serde_json::json!({
        "check": "li\u{202e}n\u{2066}t\u{2069}\u{200b}\u{feff}\u{e0041}\u{2028}\u{2029}\u{85}\u{1b}]52;\u{7}",
        "kept": ["\u{e9}t\u{e9}", "\u{1f600}", "a\nb"],
    });
    for text in [
        serde_json::to_string(&value).unwrap(),
        serde_json::to_string_pretty(&value).unwrap(),
    ] {
        let mut out = Vec::new();
        crate::output::say_json(&mut out, &text);
        let said = String::from_utf8(out).unwrap();
        let line = said.strip_suffix('\n').unwrap();
        assert!(!line.contains('\n'), "{line}");
        for hidden in [
            '\u{202e}',
            '\u{2066}',
            '\u{2069}',
            '\u{200b}',
            '\u{feff}',
            '\u{e0041}',
            '\u{2028}',
            '\u{2029}',
            '\u{85}',
            '\u{1b}',
            '\u{7}',
        ] {
            assert!(!line.contains(hidden), "{line}");
        }
        assert!(
            line.contains("\\u202e") && line.contains("\\udb40\\udc41"),
            "{line}"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(line).unwrap(),
            value
        );
    }
}

/// `world`'s store keeping `cli/cli#1` on its head `h1`, whose check `test`
/// fails now and failed then passed on it an hour ago, read now.
fn flaky(world: &World) {
    flaky_seen(world, herald_core::Visibility::Public);
}

/// [`flaky`], its repository seen as `visibility`.
fn flaky_seen(world: &World, visibility: herald_core::Visibility) {
    use herald_core::{Checks, Flip, KeptPull, PullKey, Reconciled, RepositoryKey};
    use herald_ports::{Pulls as _, Store as _};
    let now = jiff::Timestamp::now();
    let ago = now - jiff::SignedDuration::from_hours(1);
    let key = RepositoryKey {
        host: "github.com".to_owned(),
        id: "1".to_owned(),
    };
    let pull = herald_core::Pull {
        project: "cli".to_owned(),
        repository: "cli/cli".to_owned(),
        visibility,
        number: 1,
        title: "Change".to_owned(),
        url: "https://github.com/cli/cli/pull/1".to_owned(),
        author: Some("ana".to_owned()),
        bot: false,
        draft: false,
        labels: Vec::new(),
        opened: ago,
        updated: ago,
        review: herald_core::Review::Approved,
        reviewers: Vec::new(),
        checks: Checks::Failed(vec!["test".to_owned()]),
        mergeable: herald_core::Mergeable::Clean,
        queued: false,
        forge: None,
        head: Some("h1".to_owned()),
        unread: false,
        base: None,
        files: None,
        areas: Vec::new(),
        flips: vec![Flip {
            check: "test".to_owned(),
            head: "h1".to_owned(),
            at: ago,
        }],
    };
    let pulled = PullKey {
        repository: key.clone(),
        number: 1,
    };
    let store = kept(world);
    let pulls = store.pulls().unwrap();
    let record = KeptPull::read(None, pulled, pull, now);
    pollster::block_on(pulls.put_pull(&record, None)).unwrap();
    let mark = Reconciled {
        repository: Some(key),
        at: Some(now),
        ..Reconciled::default()
    };
    pollster::block_on(pulls.put_reconciled(&SourceName::new("cli"), &mark, None)).unwrap();
}

#[test]
fn a_held_plan_is_printed_and_an_apply_tells_the_operator_where_to_sign_in() {
    let config = CONFIG.replace(
        "repository = \"cli/cli\"\n",
        "repository = \"cli/cli\"\nwrite = { token = { secret = \"HERALD_RERUN\" } }\nsign_in = { client_id = \"Iv23liClient\" }\n",
    );
    let mut setup = setup(&format!(
        "{config}[views.kept]\nsources = [\"cli\"]\nmode = \"kept\"\nreconcile = \"1d\"\n[rules.flaky]\nview = \"kept\"\nflaky = {{ rerun = true }}\nevery = \"1h\"\nat = \"00:00\"\n"
    ));
    setup
        .vars
        .insert("HERALD_RERUN".to_owned(), "write-value".to_owned());
    let world = World::default();
    flaky(&world);
    let runs = json!({ "workflow_runs": [
        { "id": 42, "html_url": "https://github.com/cli/cli/actions/runs/42" }
    ] });
    let jobs = json!({ "jobs": [{ "id": 420, "name": "test", "conclusion": "failure" }] });
    let device = json!({ "device_code": "device-value", "user_code": "WDJB-MJHT",
        "verification_uri": "https://github.com/login/device", "interval": 1 });
    {
        let forge = &mut world.state.lock().unwrap().forge;
        for (path, body) in [
            (
                "/repos/cli/cli/actions/runs?head_sha=h1&status=failure&per_page=5",
                runs,
            ),
            (
                "/repos/cli/cli/actions/runs/42/jobs?filter=latest&per_page=100",
                jobs,
            ),
            ("/login/device/code", device),
            (
                "/login/oauth/access_token",
                json!({ "error": "access_denied" }),
            ),
        ] {
            forge.insert(path.to_owned(), (200, body.to_string()));
        }
    }
    let plan = herald(&setup, &world, &["rerun", "flaky"]);
    assert_eq!(
        plan.out.lines().collect::<Vec<_>>(),
        [
            "flaky cli/cli#1: test flipped 1 times; head h1",
            "  run 42 job 420  https://github.com/cli/cli/actions/runs/42",
            "  call: POST https://api.github.com/repos/cli/cli/actions/jobs/420/rerun",
            "  to apply: herald rerun flaky --apply --job 420@h1",
        ],
        "{}",
        plan.err
    );
    let applied = herald(
        &setup,
        &world,
        &["rerun", "flaky", "--apply", "--job", "420@h1"],
    );
    let lines: Vec<&str> = applied.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}{}", applied.out, applied.err);
    assert!(
        lines[0].starts_with(
            "To apply the re-runs of cli, open https://github.com/login/device and enter the code WDJB-MJHT within "
        ),
        "{}",
        lines[0]
    );
    assert_eq!(
        lines[1],
        format!(
            "cli/cli#1 job 420@h1: not re-run: {}",
            herald_sources_forge::DENIED
        )
    );
}

#[test]
fn a_job_named_without_its_id_or_head_is_refused_and_an_apply_of_no_rule_says_why() {
    let setup = setup(&ruled());
    let world = World::default();
    listed(&world);
    for named in ["@h1", "42@", "@"] {
        let refused = herald(
            &setup,
            &world,
            &["rerun", "flaky", "--apply", "--job", named],
        );
        assert!(
            refused.err.contains(&format!("\"{named}\" is not a job")),
            "{named}: {}",
            refused.err
        );
    }
    let unknown = herald(
        &setup,
        &world,
        &["rerun", "nope", "--apply", "--job", "42@h1"],
    );
    assert_ne!(unknown.code, DONE);
    assert!(
        unknown.err.contains("no rule is named \"nope\""),
        "{}",
        unknown.err
    );
}

#[test]
fn a_private_repository_s_plan_is_withheld_and_its_job_refused_unless_the_output_is_said_private() {
    let setup = setup(&format!(
        "{CONFIG}[views.kept]\nsources = [\"cli\"]\nmode = \"kept\"\nreconcile = \"1d\"\n[rules.flaky]\nview = \"kept\"\nflaky = {{ rerun = true }}\nevery = \"1h\"\nat = \"00:00\"\n"
    ));
    let world = World::default();
    flaky_seen(&world, herald_core::Visibility::Private);
    let plan = herald(&setup, &world, &["rerun", "flaky"]);
    assert_eq!(
        plan.out.lines().collect::<Vec<_>>(),
        [
            "no flaky check is failing on the rule's open pull requests, so nothing is planned",
            "the plans of 1 pull requests of private or internal repositories withheld: --private, or [cli] private, prints them",
        ],
        "{}",
        plan.err
    );
    let applied = herald(
        &setup,
        &world,
        &["rerun", "flaky", "--apply", "--job", "420@h1"],
    );
    assert_eq!(
        applied.out.trim(),
        format!(
            "job 420@h1: not re-run: {}",
            herald_flows::UNPLANNED_WITHHELD
        )
    );
    assert!(!applied.out.contains("cli/cli"), "{}", applied.out);
}
