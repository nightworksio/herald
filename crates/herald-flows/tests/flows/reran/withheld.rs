//! A private or internal repository's plans withheld and counted unless
//! the output is said private, a job of a withheld plan refused naming
//! nothing, and nothing of its rows, conflicts, flaky checks, CI runs or
//! unread lines held by a board or a plan (ADR-0069).

use std::collections::BTreeMap;

use herald_config::{Config, Uses, load};
use herald_core::{Changed, Checks, Flip, KeptPull, Printed, PullKey, Reconciled, Visibility};
use herald_flows::{
    Budget, Connected, Ports, RerunPlans, UNPLANNED_WITHHELD, prepare, rerun_plans,
};
use herald_ports::{Pulls as _, Response};
use herald_testing::{FakeClock, FakeSigner};
use jiff::SignedDuration;
use serde_json::json;

use super::{
    CHOSEN, TOKEN_READ, TOKEN_WRITE, applied_from, config, planned, refused, scripted, secrets,
};
use crate::harness::{Failing, NOON, Script, healthy};

/// The private repository whose name, owner, pull requests and heads no
/// withheld output may hold.
const SECRET: &str = "acme/secret-plans";

/// What of [`SECRET`] the output may not hold: its owner, its name, its
/// pull requests' numbers and titles, and their heads.
const NAMED: [&str; 6] = [
    "acme",
    "secret-plans",
    "7041",
    "7042",
    "Merger plan",
    "c0ffee",
];

#[test]
fn a_private_repository_s_plan_is_withheld_and_counted_unless_the_output_is_said_private() {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    crate::boards::flaky_seen(&store, "Change", herald_core::Visibility::Private);
    let script = scripted(&[planned(), planned()]);
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let flow = (&config, &connected, &ports);
    let withheld = pollster::block_on(rerun_plans(flow, (None, Printed::Withheld))).unwrap();
    assert_eq!(
        withheld,
        RerunPlans {
            planned: Vec::new(),
            withheld: 1
        }
    );
    assert!(script.seen().is_empty(), "{:?}", script.seen());
    let board = pollster::block_on(herald_flows::queue(
        &config,
        &connected,
        &ports,
        ("kept", Printed::Withheld),
    ))
    .unwrap();
    assert_eq!((board.total(), board.withheld), (0, 1));
    assert!(board.groups.iter().all(|group| group.rows.is_empty()));
    for printed in [Printed::Configured, Printed::Asked] {
        let plans = pollster::block_on(rerun_plans(flow, (None, printed))).unwrap();
        assert_eq!((plans.planned.len(), plans.withheld), (1, 0), "{printed:?}");
        let board = pollster::block_on(herald_flows::queue(
            &config,
            &connected,
            &ports,
            ("kept", printed),
        ))
        .unwrap();
        assert_eq!((board.total(), board.withheld), (1, 0), "{printed:?}");
    }
}

#[test]
fn a_job_of_a_withheld_plan_is_refused_saying_the_output_must_be_said_private_and_naming_nothing() {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    crate::boards::flaky_seen(&store, "Change", herald_core::Visibility::Internal);
    let script = scripted(&[]);
    let applied = applied_from(
        &config,
        (&config, &store, &secrets()),
        &script,
        (CHOSEN, None),
        (None, Printed::Withheld),
    );
    assert_eq!(refused(&applied), UNPLANNED_WITHHELD);
    let only = &applied.applied.as_ref().unwrap()[0];
    assert_eq!((&only.repository, only.number), (&None, None));
    assert!(!UNPLANNED_WITHHELD.contains("o/open"));
    assert!(applied.told.is_empty() && script.seen().is_empty());
}

/// A config reading [`SECRET`] kept, predicting conflicts and showing its
/// CI capacity, and a rule planning re-runs of its flaky checks.
fn secret_config() -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.work]
kind = "github"
repository = "{SECRET}"
{TOKEN_READ}
{TOKEN_WRITE}
[views.kept]
sources = ["work"]
mode = "kept"
reconcile = "1d"
conflicts = true
overviews = ["capacity"]
[rules.flaky]
view = "kept"
flaky = {{ rerun = true }}
every = "1h"
at = "00:00"
"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

/// `store` keeping two private pull requests of [`SECRET`] that change one
/// path, the first's check `test` failing on its head and having failed
/// then passed on it an hour before noon, listed at noon.
fn kept_secret(store: &Failing) {
    let key = herald_core::RepositoryKey {
        host: "github.com".to_owned(),
        id: "7".to_owned(),
    };
    let hour_ago = NOON.checked_sub(SignedDuration::from_hours(1)).unwrap();
    for (number, flipped) in [(7041, true), (7042, false)] {
        let head = format!("c0ffee{number}");
        let (checks, flips) = if flipped {
            let flip = Flip {
                check: "test".to_owned(),
                head: head.clone(),
                at: hour_ago,
            };
            (Checks::Failed(vec!["test".to_owned()]), vec![flip])
        } else {
            (Checks::Passed, Vec::new())
        };
        let pull = herald_core::Pull {
            project: "work".to_owned(),
            repository: SECRET.to_owned(),
            visibility: Visibility::Private,
            number,
            title: format!("Merger plan, part {number}"),
            url: format!("https://github.com/{SECRET}/pull/{number}"),
            author: Some("ana".to_owned()),
            bot: false,
            draft: false,
            labels: Vec::new(),
            opened: NOON.checked_sub(SignedDuration::from_hours(48)).unwrap(),
            updated: hour_ago,
            review: herald_core::Review::Approved,
            reviewers: Vec::new(),
            checks,
            mergeable: herald_core::Mergeable::Clean,
            queued: false,
            forge: None,
            head: Some(head),
            unread: false,
            base: Some("main".to_owned()),
            files: Some(Changed {
                paths: ["src/lib.rs".to_owned()].into(),
                whole: true,
            }),
            flips,
        };
        let pulled = PullKey {
            repository: key.clone(),
            number,
        };
        let record = KeptPull::read(None, pulled, pull, NOON);
        pollster::block_on(store.put_pull(&record, None)).unwrap();
    }
    let mark = Reconciled {
        repository: Some(key),
        at: Some(NOON),
        ..Reconciled::default()
    };
    let source = herald_core::SourceName::new("work");
    pollster::block_on(store.put_reconciled(&source, &mark, None)).unwrap();
}

/// [`SECRET`]'s CI runs: a full page queued on the first head, so the
/// listing is named unread as holding more, one running on a head no
/// pull request is on any more, and the failed run 42 on the first head,
/// whose job `test` failed.
fn secret_runs() -> Script {
    let runs = |listed: &[(u64, &str)]| {
        let runs: Vec<_> = listed
            .iter()
            .map(|(id, sha)| {
                json!({ "id": id, "html_url": format!("https://github.com/{SECRET}/actions/runs/{id}"),
                    "head_sha": sha, "event": "pull_request", "created_at": "2026-11-20T10:00:00Z" })
            })
            .collect();
        Response {
            status: 200,
            headers: Vec::new(),
            body: json!({ "workflow_runs": runs }).to_string().into_bytes(),
        }
    };
    let full: Vec<(u64, &str)> = (1..=100).map(|id| (id, "c0ffee7041")).collect();
    let api = format!("https://api.github.com/repos/{SECRET}/actions/runs");
    let answer = |body: serde_json::Value| Response {
        status: 200,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    };
    Script::keyed(BTreeMap::from([
        (format!("{api}?status=queued&per_page=100"), runs(&full)),
        (
            format!("{api}?status=in_progress&per_page=100"),
            runs(&[(101, "c0ffee7040")]),
        ),
        (
            format!("{api}?head_sha=c0ffee7041&status=failure&per_page=5"),
            runs(&[(42, "c0ffee7041")]),
        ),
        (
            format!("{api}/42/jobs?filter=latest&per_page=100"),
            answer(json!({ "jobs": [{ "id": 420, "name": "test", "conclusion": "failure" }] })),
        ),
    ]))
}

#[test]
fn a_private_repository_s_conflicts_flaky_check_runs_and_unread_line_name_it_nowhere_unless_the_output_is_said_private()
 {
    let config = secret_config();
    let store = healthy();
    kept_secret(&store);
    let script = secret_runs();
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let flow = (&config, &connected, &ports);
    let shown = |printed| {
        let board = pollster::block_on(herald_flows::queue(
            &config,
            &connected,
            &ports,
            ("kept", printed),
        ))
        .unwrap();
        let plans = pollster::block_on(rerun_plans(flow, (None, printed))).unwrap();
        (board, plans)
    };
    let (board, plans) = shown(Printed::Asked);
    let rows: Vec<(u64, Vec<u64>)> = board
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .map(|row| (row.pull.number, row.overlaps.clone()))
        .collect();
    assert!(rows.contains(&(7041, vec![7042])), "{rows:?}");
    assert_eq!(board.failures[0].pulls, [(SECRET.to_owned(), 7041)]);
    assert_eq!(board.overviews.capacity.as_ref().map(Vec::len), Some(101));
    assert_eq!(board.overviews.superseded().len(), 1);
    assert_eq!(board.overviews.unread[0].repository, SECRET);
    assert_eq!(plans.planned.len(), 1);
    let said = format!("{board:?}{plans:?}");
    for named in NAMED {
        assert!(said.contains(named), "{named}: {said}");
    }
    let (board, plans) = shown(Printed::Withheld);
    assert_eq!((board.withheld, plans.withheld), (2, 2));
    let said = format!("{board:?}{plans:?}");
    for named in NAMED {
        assert!(!said.contains(named), "{named}: {said}");
    }
}

#[test]
fn a_plan_whose_runs_fail_to_read_is_said_in_full_only_where_the_output_is_said_private() {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    crate::boards::flaky_seen(&store, "Change", herald_core::Visibility::Public);
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let said = |printed| {
        let script = Script::answering(&[(super::RUNS, 404, "{}")]);
        let prepared =
            pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
        let connected: Connected<Script> =
            prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
        pollster::block_on(rerun_plans((&config, &connected, &ports), (None, printed)))
    };
    assert_eq!(
        said(Printed::Withheld),
        Err(herald_flows::Stop::Source(format!(
            "the source \"open\" could not be read: its forge's answer could not be read; {}",
            herald_flows::UNSAID_FAILURE
        )))
    );
    let full = format!("{:?}", said(Printed::Configured));
    assert!(full.contains("answered with 404"), "{full}");
}
