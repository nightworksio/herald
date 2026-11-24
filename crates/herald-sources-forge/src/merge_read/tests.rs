use herald_ports::Response;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use herald_core::merging::SKEW;

use super::*;
use crate::SourceOptions;

pub(super) const LABEL: &str = "herald:merge";

pub(super) fn ok(body: &Value) -> Response {
    Response {
        status: 200,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

pub(super) fn refused(status: u16) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: b"{}".to_vec(),
    }
}

pub(super) fn at(minute: u32) -> Timestamp {
    format!("2026-01-01T00:{minute:02}:00Z").parse().unwrap()
}

pub(super) fn stamp(minute: u32) -> String {
    at(minute).to_string()
}

pub(super) fn at_second(minute: u32, second: u32) -> Timestamp {
    format!("2026-01-01T00:{minute:02}:{second:02}Z")
        .parse()
        .unwrap()
}

pub(super) fn reader(forge: Forge) -> ForgeMergeReader {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    ForgeMergeReader {
        origin: Origin::new(forge, &options, "o/r"),
        token: None,
    }
}

pub(super) fn entered(
    forge: Forge,
    answers: Vec<Response>,
    enter: &Enter,
) -> Result<Vec<Entered>, Unpolled> {
    entered_least(forge, answers, (enter, Permission::Write, Vouches::Gesture))
}

pub(super) fn entered_least(
    forge: Forge,
    answers: Vec<Response>,
    (enter, least, vouches): (&Enter, Permission, Vouches),
) -> Result<Vec<Entered>, Unpolled> {
    let http = ScriptedHttp::new(answers);
    let reader = reader(forge);
    let asked = EnterAsked {
        enter,
        least,
        vouches,
        skew: SKEW,
    };
    pollster::block_on(reader.entered(&http, "main", asked))
}

pub(super) fn approved_by(
    forge: Forge,
    answers: Vec<Response>,
    least: Permission,
) -> Result<(bool, bool), Unpolled> {
    approved_within(forge, answers, (least, SKEW))
}

pub(super) fn approved_within(
    forge: Forge,
    answers: Vec<Response>,
    asked: (Permission, SignedDuration),
) -> Result<(bool, bool), Unpolled> {
    let http = ScriptedHttp::new(answers);
    let pull = Opened {
        number: 1,
        url: "u1".to_owned(),
        draft: false,
        head: "h2".to_owned(),
        author: Some("author".to_owned()),
        labels: Vec::new(),
        head_repository: None,
        head_branch: None,
    };
    pollster::block_on(reader(forge).approved(&Erased(&http), &pull, asked, &mut BTreeMap::new()))
}

pub(super) fn proved(
    forge: Forge,
    answers: Vec<Response>,
    (entered, branched): (&Entered, &Branched),
) -> Result<Proved, Unpolled> {
    let http = ScriptedHttp::new(answers);
    let reader = reader(forge);
    pollster::block_on(reader.proved(&http, entered, branched))
}

pub(super) fn branched(forge: Forge, answers: Vec<Response>) -> Result<Branched, Unpolled> {
    let http = ScriptedHttp::new(answers);
    let reader = reader(forge);
    pollster::block_on(reader.branch(&http, "main"))
}

pub(super) fn entry(head: &str) -> Entered {
    Entered {
        number: 1,
        url: "https://forge.test/o/r/pull/1".to_owned(),
        draft: false,
        head: head.to_owned(),
        gesture: Gesture {
            by: "w".to_owned(),
            at: at(10),
            head: Some(head.to_owned()),
            named: None,
            role: Some(Permission::Write),
        },
        arrived: None,
        approved: false,
        whole: true,
    }
}

pub(super) fn base(required: &[&str], whole: bool) -> Branched {
    Branched {
        visibility: herald_core::Visibility::Public,
        head: "b1".to_owned(),
        required: required.iter().map(|name| (*name).to_owned()).collect(),
        queue: Some(false),
        whole,
    }
}

/// `pages` full pages of `size` items each made by `item`.
pub(super) fn pages(pages: usize, size: usize, item: impl Fn(usize) -> Value) -> Vec<Response> {
    (0..pages)
        .map(|page| {
            ok(&Value::Array(
                (0..size).map(|index| item(page * size + index)).collect(),
            ))
        })
        .collect()
}

fn actor(login: &str) -> Actor {
    Actor {
        login: login.to_owned(),
        id: None,
    }
}

fn mark(login: &str, minute: u32, into: bool) -> Mark {
    Mark {
        by: actor(login),
        at: at(minute),
        into,
        named: None,
    }
}

#[test]
fn a_comment_is_the_command_whole_or_the_command_and_cancel() {
    assert_eq!(commanded("/merge", "  /merge \n"), Some((true, None)));
    assert_eq!(commanded("/merge", "/merge cancel"), Some((false, None)));
    assert_eq!(commanded("/merge", "/merge\tcancel "), Some((false, None)));
    assert_eq!(commanded("/merge", "/mergecancel"), None);
    assert_eq!(commanded("/merge", "/merge please"), None);
    assert_eq!(commanded("/merge", "/merge cancelled"), None);
    assert_eq!(commanded("/merge", "please /merge"), None);
}

#[test]
fn the_last_gesture_stands_unless_a_later_mark_counted_it_out() {
    let marks = vec![mark("a", 3, true), mark("b", 1, true), mark("c", 2, false)];
    assert_eq!(stood(marks), Some((actor("a"), at(3))));
    let marks = vec![mark("b", 2, true), mark("a", 1, true)];
    assert_eq!(stood(marks), Some((actor("b"), at(2))));
    assert_eq!(
        standing(vec![mark("a", 1, true), mark("x", 2, false)]),
        None
    );
    assert_eq!(stood(Vec::new()), None);
}

#[test]
fn the_head_then_is_the_head_now_or_the_one_the_first_later_push_moved_from() {
    let push = |minute, before: &str| Push {
        at: at(minute),
        before: Some(before.to_owned()),
    };
    let pushes = vec![push(5, "h1"), push(20, "h3"), push(15, "h2")];
    assert_eq!(
        head_then(&(pushes.clone(), true), (at(10), SKEW), "h4"),
        Some("h2".to_owned())
    );
    assert_eq!(
        head_then(&(pushes.clone(), true), (at(15), SKEW), "h4"),
        Some("h2".to_owned())
    );
    assert_eq!(
        head_then(&(pushes.clone(), true), (at(30), SKEW), "h4"),
        Some("h4".to_owned())
    );
    assert_eq!(
        head_then(&(pushes.clone(), false), (at(10), SKEW), "h4"),
        Some("h2".to_owned())
    );
    assert_eq!(head_then(&(pushes, false), (at(1), SKEW), "h4"), None);
    assert_eq!(
        head_then(&(Vec::new(), true), (at(1), SKEW), "h4"),
        Some("h4".to_owned())
    );
    assert_eq!(head_then(&(Vec::new(), false), (at(1), SKEW), "h4"), None);
    let unsaid = vec![Push {
        at: at(20),
        before: None,
    }];
    assert_eq!(
        head_then(&(unsaid.clone(), true), (at(10), SKEW), "h4"),
        None
    );
    assert_eq!(arrived(&unsaid), Some(at(20)));
    assert_eq!(arrived(&[]), None);
}

#[test]
fn a_push_stamped_within_the_skew_before_a_gesture_counts_as_after_it() {
    let pushes = (
        vec![
            Push {
                at: at(1),
                before: Some("h1".to_owned()),
            },
            Push {
                at: at(10),
                before: Some("h2".to_owned()),
            },
        ],
        true,
    );
    let skew = SignedDuration::from_secs(5);
    let then = |second| head_then(&pushes, (at_second(10, second), skew), "h3");
    assert_eq!(then(3), Some("h2".to_owned()));
    assert_eq!(then(5), Some("h2".to_owned()));
    assert_eq!(then(6), Some("h3".to_owned()));
    assert_eq!(
        head_then(
            &pushes,
            (at_second(10, 6), SignedDuration::from_secs(10)),
            "h3"
        ),
        Some("h2".to_owned())
    );
    assert_eq!(
        head_then(&pushes, (Timestamp::MIN, skew), "h3"),
        Some("h1".to_owned())
    );
    let listed = (pushes.0[1..].to_vec(), false);
    assert_eq!(head_then(&listed, (at_second(10, 5), skew), "h3"), None);
    assert_eq!(
        head_then(&listed, (at_second(10, 6), skew), "h3"),
        Some("h3".to_owned())
    );
}

#[test]
fn an_edited_comment_is_no_gesture_but_still_a_cancel() {
    let said = |body: &str, edited| Said {
        by: actor("w"),
        at: at(1),
        body: body.to_owned(),
        edited,
    };
    let marks = comment_marks(
        vec![
            said("/merge", true),
            said("/merge", false),
            said("hello", false),
            said("/merge cancel", true),
        ],
        "/merge",
    );
    assert_eq!(marks, vec![mark("w", 1, true), mark("w", 1, false)]);
}

#[test]
fn a_refused_read_names_the_credential_a_merge_queue_reads_with() {
    let error = branched(Forge::GitHub, vec![refused(403)]).unwrap_err();
    assert!(error.into_reason().contains(scopes(Forge::GitHub)));
    let missing = branched(Forge::GitHub, vec![refused(404)]).unwrap_err();
    assert_eq!(missing.into_reason(), "the forge found no repository");
    let limited = branched(Forge::GitHub, vec![refused(429)]).unwrap_err();
    assert!(matches!(limited, Unpolled::RateLimited(_)), "{limited:?}");
    assert!(limited.into_reason().contains("rate limit was reached"));
    let listing = entered(Forge::GitLab, vec![refused(401)], &Enter::default()).unwrap_err();
    assert!(listing.into_reason().contains(scopes(Forge::GitLab)));
}

#[test]
fn more_open_pull_requests_than_herald_reads_stop_the_read() {
    let pull = |index: usize| json!({"iid": index, "web_url": "u", "sha": "h", "labels": []});
    let error = entered(Forge::GitLab, pages(5, 100, pull), &Enter::default()).unwrap_err();
    assert_eq!(
        error.into_reason(),
        "more than 500 pull requests are open, and herald reads no more"
    );
}

#[test]
fn more_pull_requests_that_may_hold_a_gesture_than_herald_reads_stop_the_read() {
    let pull = |index: usize| json!({"iid": index, "web_url": "u", "sha": "h", "labels": []});
    let error = entered(
        Forge::GitLab,
        pages(1, 51, pull),
        &Enter::Comment("/merge".to_owned()),
    )
    .unwrap_err();
    assert_eq!(
        error.into_reason(),
        "more than 50 open pull requests into the branch may hold a gesture, and herald reads no more"
    );
}

fn stood(marks: Vec<Mark>) -> Option<(Actor, Timestamp)> {
    standing(marks).map(|mark| (mark.by, mark.at))
}

fn review(login: &str, commit: &str, approves: Option<bool>) -> Review {
    Review {
        by: actor(login),
        commit: Some(commit.to_owned()),
        approves,
    }
}

#[test]
fn each_reviewer_s_last_verdict_alone_approves_the_head() {
    let reviews = vec![
        review("a", "h2", Some(true)),
        review("b", "h1", Some(true)),
        review("c", "h2", Some(true)),
        review("c", "h2", Some(false)),
        review("d", "h2", Some(false)),
        review("d", "h2", Some(true)),
        review("e", "h2", Some(true)),
        review("e", "h2", None),
    ];
    let logins: Vec<String> = approving(reviews, "h2")
        .into_iter()
        .map(|by| by.login)
        .collect();
    assert_eq!(logins, ["a", "d", "e"]);
}

#[test]
fn a_comment_names_the_commit_it_vouches_for_by_its_full_id_alone() {
    let sha1 = "0123456789abcdef0123456789abcdef01234567";
    let sha256 = format!("{sha1}{}", "0123456789abcdef01234567");
    assert_eq!(
        commanded("/merge", &format!("/merge {sha1}")),
        Some((true, Some(sha1.to_owned())))
    );
    assert_eq!(
        commanded("/merge", &format!("/merge  {sha256} ")),
        Some((true, Some(sha256.clone())))
    );
    for short in [
        &sha1[..39],
        &sha1.to_uppercase(),
        &format!("{}g", &sha1[..39]),
    ] {
        assert_eq!(commanded("/merge", &format!("/merge {short}")), None);
    }
    assert_eq!(commanded("/merge", &format!("/merge{sha1}")), None);
}

#[test]
fn a_forge_source_reads_a_merge_queue_s_repository_on_its_own_forge() {
    let source = crate::ForgeSource {
        forge: Forge::GitLab,
        options: SourceOptions {
            repository: Some("o/r".to_owned()),
            ..SourceOptions::default()
        },
        names: None,
    };
    let merging = herald_kinds::Merging {
        forge: "gitlab",
        repository: "group/app".to_owned(),
        api: "https://gitlab.com/api/v4".to_owned(),
        update: herald_core::merging::Update::Rebase,
        queueless: None,
        reads: scopes(Forge::GitLab),
    };
    let reader = herald_kinds::SourceConfig::merge_reader(&source, &merging, None).unwrap();
    let http = ScriptedHttp::new(vec![refused(404)]);
    let read = pollster::block_on(reader.branch(&http, "main"));
    assert!(read.is_err());
    assert_eq!(
        http.requests()[0].url.expose(),
        "https://gitlab.com/api/v4/projects/group%2Fapp"
    );
}

#[test]
fn as_many_pull_requests_that_may_hold_a_gesture_as_herald_reads_are_read() {
    let pull = |index: usize| json!({"iid": index, "web_url": "u", "sha": "h", "labels": []});
    let mut answers = vec![ok(&Value::Array((0..MOST_ENTERED).map(pull).collect()))];
    answers.extend((0..MOST_ENTERED).map(|_| ok(&json!([]))));
    let read = entered(Forge::GitLab, answers, &Enter::Comment("/merge".to_owned()));
    assert_eq!(read.unwrap(), []);
}
