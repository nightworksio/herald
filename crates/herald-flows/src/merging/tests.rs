use std::sync::Mutex;

use herald_core::merging::{
    AppliedBy, Check, CheckState, Enter, Message, SKEW, Update, Vouches, WAIT,
};
use herald_core::{Permission, SourceName, Visibility};
use herald_kinds::{Gesture, Merging, Proved, Unpolled};
use herald_ports::Boxed;
use herald_testing::ScriptedHttp;
use jiff::SignedDuration;

use super::*;

#[derive(Debug, Default)]
struct Fixed {
    proved: Vec<(u64, Proved)>,
    branched: Option<Branched>,
    entries: Option<Vec<Entered>>,
    asked: Mutex<Vec<(Enter, Permission, Vouches, SignedDuration)>>,
}

impl Fixed {
    fn proving(proved: Vec<(u64, Proved)>) -> Self {
        Self {
            proved,
            ..Self::default()
        }
    }

    fn listing(branched: Option<Branched>, entries: Option<Vec<Entered>>) -> Self {
        Self {
            proved: Vec::new(),
            branched,
            entries,
            asked: Mutex::default(),
        }
    }
}

fn unasked(what: &str) -> Unpolled {
    Unpolled::Unreadable(format!("{what} unread"))
}

impl MergeReader for Fixed {
    fn branch<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: &'a str,
    ) -> Boxed<'a, Result<Branched, Unpolled>> {
        let found = self.branched.clone().ok_or_else(|| unasked("the branch"));
        Box::pin(async move { found })
    }

    fn entered<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: &'a str,
        asked: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Vec<Entered>, Unpolled>> {
        self.asked.lock().unwrap().push((
            asked.enter.clone(),
            asked.least,
            asked.vouches,
            asked.skew,
        ));
        let found = self.entries.clone().ok_or_else(|| unasked("the entries"));
        Box::pin(async move { found })
    }

    fn proved<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        entered: &'a Entered,
        _: &'a Branched,
    ) -> Boxed<'a, Result<Proved, Unpolled>> {
        let found = self
            .proved
            .iter()
            .find(|(number, _)| *number == entered.number)
            .map(|(_, proved)| proved.clone())
            .ok_or_else(|| Unpolled::Unreadable(format!("#{} proved", entered.number)));
        Box::pin(async move { found })
    }
}

fn now() -> Timestamp {
    "2026-01-01T01:00:00Z".parse().unwrap()
}

fn entry(number: u64, (gestured, head): (&str, &str), whole: bool) -> Entered {
    Entered {
        number,
        url: format!("u{number}"),
        draft: false,
        head: head.to_owned(),
        gesture: Gesture {
            by: "w".to_owned(),
            at: now() - SignedDuration::from_mins(30),
            head: Some(gestured.to_owned()),
            named: None,
            role: Some(Permission::Write),
        },
        arrived: None,
        approved: false,
        whole,
    }
}

fn green(state: CheckState, whole: bool) -> Proved {
    Proved {
        up_to_date: true,
        checks: vec![Check {
            name: "test".to_owned(),
            state,
        }],
        required: vec!["test".to_owned()],
        mergeable: Some(true),
        whole,
    }
}

fn queue(runs: Runs) -> MergeQueue {
    MergeQueue {
        source: SourceName::new("app"),
        forge: Merging {
            forge: "github",
            repository: "o/r".to_owned(),
            api: "https://api.github.com".to_owned(),
            update: Update::Merge,
            queueless: None,
            reads: "a token",
        },
        branch: "main".to_owned(),
        runs,
        enter: Enter::default(),
        least: Permission::Write,
        vouches: Vouches::Gesture,
        skew: SKEW,
        method: None,
        message: Message::Forge,
        may_fail: Vec::new(),
        wait: WAIT,
        retries: None,
        applied_by: AppliedBy::Command,
        write: None,
    }
}

fn base() -> Branched {
    Branched {
        visibility: Visibility::Public,
        head: "b1".to_owned(),
        required: Vec::new(),
        queue: Some(false),
        whole: true,
    }
}

fn nexts(reader: &Fixed, runs: Runs, entries: Vec<Entered>) -> Vec<Next> {
    let http = ScriptedHttp::new(Vec::new());
    pollster::block_on(judged(
        (reader, &http),
        &queue(runs),
        (entries, &base()),
        now(),
    ))
    .unwrap()
    .into_iter()
    .map(|judged| judged.next)
    .collect()
}

#[test]
fn herald_s_queue_proves_the_oldest_entry_alone_and_the_rest_wait_their_turn() {
    let reader = Fixed::proving(vec![(3, green(CheckState::Passed, true))]);
    let entries = vec![
        entry(1, ("h1", "h1"), false),
        entry(2, ("h1", "h2"), true),
        entry(3, ("h3", "h3"), true),
        entry(4, ("h4", "h4"), true),
    ];
    assert_eq!(
        nexts(&reader, Runs::Herald, entries),
        [
            Next::Verdict(Verdict::Leaves(Left::Unread)),
            Next::Verdict(Verdict::Leaves(Left::HeadMoved)),
            Next::Verdict(Verdict::Merge),
            Next::Behind
        ]
    );
}

#[test]
fn an_entry_that_leaves_hands_the_turn_to_the_next() {
    let reader = Fixed::proving(vec![
        (1, green(CheckState::Failed, true)),
        (2, green(CheckState::Passed, false)),
        (3, green(CheckState::Pending, true)),
    ]);
    let entries = vec![
        entry(1, ("h1", "h1"), true),
        entry(2, ("h2", "h2"), true),
        entry(3, ("h3", "h3"), true),
        entry(4, ("h4", "h4"), true),
    ];
    assert_eq!(
        nexts(&reader, Runs::Herald, entries),
        [
            Next::Verdict(Verdict::Leaves(Left::ChecksFailed)),
            Next::Verdict(Verdict::Leaves(Left::Unread)),
            Next::Verdict(Verdict::Wait),
            Next::Behind
        ]
    );
}

#[test]
fn the_forge_s_queue_takes_each_vouched_entry_but_a_draft() {
    let reader = Fixed::proving(Vec::new());
    let mut draft = entry(3, ("h3", "h3"), true);
    draft.draft = true;
    let entries = vec![
        entry(1, ("h1", "h1"), true),
        entry(2, ("h1", "h2"), true),
        draft,
        entry(4, ("h4", "h4"), false),
    ];
    assert_eq!(
        nexts(&reader, Runs::Forge, entries),
        [
            Next::HandOff,
            Next::Verdict(Verdict::Leaves(Left::HeadMoved)),
            Next::Draft,
            Next::Verdict(Verdict::Leaves(Left::Unread))
        ]
    );
}

#[test]
fn a_queue_s_wait_runs_from_its_gesture_or_its_head_s_arrival_whichever_is_later() {
    let reader = Fixed::proving(vec![(1, green(CheckState::Pending, true))]);
    let mut late = entry(1, ("h1", "h1"), true);
    late.gesture.at = now() - SignedDuration::from_hours(3);
    assert_eq!(
        nexts(&reader, Runs::Herald, vec![late.clone()]),
        [Next::Verdict(Verdict::Leaves(Left::ChecksTimedOut))]
    );
    late.arrived = Some(now() - SignedDuration::from_mins(5));
    assert_eq!(
        nexts(&reader, Runs::Herald, vec![late]),
        [Next::Verdict(Verdict::Wait)]
    );
}

fn found_by(reader: &Fixed, runs: Runs, printed: Printed) -> Found {
    let http = ScriptedHttp::new(Vec::new());
    pollster::block_on(found((reader, &http), &queue(runs), printed, now()))
}

#[test]
fn a_private_queue_s_entries_are_counted_alone_unless_the_output_is_private() {
    let private = Branched {
        visibility: Visibility::Private,
        ..base()
    };
    let entries = vec![entry(1, ("h1", "h1"), true), entry(2, ("h2", "h2"), true)];
    let reader = Fixed::listing(Some(private), Some(entries.clone()));
    assert_eq!(
        found_by(&reader, Runs::Forge, Printed::Withheld),
        Found::Withheld(2)
    );
    let Found::Entries(judged) = found_by(&reader, Runs::Forge, Printed::Configured) else {
        panic!("a private output prints the entries");
    };
    assert_eq!(
        judged
            .into_iter()
            .map(|judged| (judged.entered, judged.next))
            .collect::<Vec<_>>(),
        entries
            .into_iter()
            .map(|entered| (entered, Next::HandOff))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_queue_any_of_whose_reads_fails_is_unread_saying_why_only_where_printed() {
    let entries = Some(vec![entry(1, ("h1", "h1"), true)]);
    let unbranched = Fixed::listing(None, entries.clone());
    let unentered = Fixed::listing(Some(base()), None);
    let unproved = Fixed::listing(Some(base()), entries);
    for (reader, runs, why) in [
        (&unbranched, Runs::Forge, "the branch unread"),
        (&unentered, Runs::Forge, "the entries unread"),
        (&unproved, Runs::Herald, "#1 proved"),
    ] {
        assert_eq!(
            found_by(reader, runs, Printed::Asked),
            Found::Unread(Some(why.to_owned()))
        );
        assert_eq!(
            found_by(reader, runs, Printed::Withheld),
            Found::Unread(None)
        );
    }
}

#[test]
fn a_dry_run_keeps_no_first_seen_head_so_those_settings_vouch_for_none() {
    let reader = Fixed::proving(vec![(1, green(CheckState::Passed, true))]);
    let http = ScriptedHttp::new(Vec::new());
    for vouches in [Vouches::Seen, Vouches::Approved] {
        let mut asked = queue(Runs::Herald);
        asked.vouches = vouches;
        let mut approved = entry(1, ("h1", "h1"), true);
        approved.approved = true;
        let judged = pollster::block_on(judged(
            (&reader, &http),
            &asked,
            (vec![approved], &base()),
            now(),
        ))
        .unwrap();
        assert_eq!(
            judged[0].next,
            Next::Verdict(Verdict::Leaves(Left::Unrecorded))
        );
    }
}

#[test]
fn a_queue_s_read_asks_for_its_gesture_role_vouching_and_skew() {
    let reader = Fixed::listing(Some(base()), Some(Vec::new()));
    let http = ScriptedHttp::new(Vec::new());
    let mut asked = queue(Runs::Herald);
    asked.enter = Enter::Comment("/merge".to_owned());
    asked.least = Permission::Maintain;
    asked.vouches = Vouches::Seen;
    asked.skew = SignedDuration::from_secs(30);
    let found = pollster::block_on(found((&reader, &http), &asked, Printed::Asked, now()));
    assert_eq!(found, Found::Entries(Vec::new()));
    assert_eq!(
        *reader.asked.lock().unwrap(),
        [(
            Enter::Comment("/merge".to_owned()),
            Permission::Maintain,
            Vouches::Seen,
            SignedDuration::from_secs(30)
        )]
    );
}

#[test]
fn a_queue_whose_installation_token_was_not_minted_says_why_where_it_can() {
    assert_eq!(
        unminted(&Stop::Source("no installation".to_owned())),
        "its source's installation token could not be minted: no installation"
    );
    assert_eq!(
        unminted(&Stop::Unreachable("down".to_owned())),
        "its source's installation token could not be minted: down"
    );
    assert_eq!(
        unminted(&Stop::Store("sealed".to_owned())),
        "its source's installation token could not be minted"
    );
}
