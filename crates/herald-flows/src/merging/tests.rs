use std::sync::Mutex;

use herald_core::merging::{
    AppliedBy, Check, CheckState, Enter, Message, RoleLost, SKEW, Update, Vouches, WAIT,
};
use herald_core::merging::{GestureRecord, HandOff, HandedOff};
use herald_core::{Permission, SourceName, Visibility};
use herald_kinds::{Gesture, Merging, Proved, Unpolled};
use herald_ports::Boxed;
use herald_testing::{FakeStore, ScriptedHttp};
use jiff::SignedDuration;

use super::*;

#[derive(Debug, Default)]
pub(super) struct Fixed {
    pub(super) fresh: Option<Entered>,
    pub(super) made: Option<Vec<String>>,
    pub(super) proved: Vec<(u64, Proved)>,
    pub(super) branched: Option<Branched>,
    pub(super) entries: Option<Vec<Entered>>,
    pub(super) taken: Option<Vec<TakenBack>>,
    pub(super) took: Option<Vec<TakenBack>>,
    pub(super) asked: Mutex<Vec<(Enter, Permission, Vouches, SignedDuration)>>,
}

impl Fixed {
    pub(super) fn proving(proved: Vec<(u64, Proved)>) -> Self {
        Self {
            fresh: None,
            proved,
            ..Self::default()
        }
    }

    fn listing(branched: Option<Branched>, entries: Option<Vec<Entered>>) -> Self {
        Self {
            fresh: None,
            made: None,
            proved: Vec::new(),
            branched,
            entries,
            taken: Some(Vec::new()),
            took: None,
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

    fn entry<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: (&'a str, u64),
        _: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Option<Entered>, Unpolled>> {
        let fresh = self.fresh.clone();
        Box::pin(async move { fresh.map(Some).ok_or_else(|| unasked("the pull request")) })
    }

    fn queued<'a>(&'a self, _: &'a dyn DynHttp, _: u64) -> Boxed<'a, Result<bool, Unpolled>> {
        Box::pin(async move { Err(unasked("the merge queue")) })
    }

    fn taken_back<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: &'a str,
        _: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Vec<TakenBack>, Unpolled>> {
        let found = self.taken.clone().ok_or_else(|| unasked("the cancels"));
        Box::pin(async move { found })
    }

    fn took_back<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: (&'a str, u64),
        _: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Option<TakenBack>, Unpolled>> {
        let took = self
            .took
            .as_ref()
            .map(|took| took.first().cloned())
            .ok_or_else(|| unasked("the marks"));
        Box::pin(async move { took })
    }

    fn merged<'a>(&'a self, _: &'a dyn DynHttp, _: u64) -> Boxed<'a, Result<bool, Unpolled>> {
        Box::pin(async { Ok(false) })
    }

    fn forge_made<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: (&'a str, Option<&'a str>),
    ) -> Boxed<'a, Result<Option<Vec<String>>, Unpolled>> {
        let made = self.made.clone();
        Box::pin(async move { Ok(made) })
    }

    fn methods<'a>(
        &'a self,
        _: &'a dyn DynHttp,
    ) -> Boxed<'a, Result<herald_kinds::Methods, Unpolled>> {
        Box::pin(async { Err(Unpolled::Unreadable("the methods unread".to_owned())) })
    }

    fn failed<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: (&'a str, &'a str),
    ) -> Boxed<'a, Result<herald_kinds::Plan, Unpolled>> {
        Box::pin(async { Err(Unpolled::Unreadable("the runs unread".to_owned())) })
    }
}

pub(super) fn now() -> Timestamp {
    "2026-01-01T01:00:00Z".parse().unwrap()
}

pub(super) fn entry(number: u64, (gestured, head): (&str, &str), whole: bool) -> Entered {
    Entered {
        number,
        title: String::new(),
        body: String::new(),
        url: format!("u{number}"),
        id: Some(format!("PR_{number}")),
        draft: false,
        head: head.to_owned(),
        gesture: Gesture {
            id: Some(70),
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

pub(super) fn green(state: CheckState, whole: bool) -> Proved {
    Proved {
        up_to_date: true,
        clean: Some(true),
        checks: vec![Check {
            name: "test".to_owned(),
            state,
        }],
        required: vec!["test".to_owned()],
        mergeable: Some(true),
        whole,
    }
}

pub(super) fn queue(runs: Runs) -> MergeQueue {
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
        updated: herald_core::merging::Updated::Signed,
        signer: None,
        update_wait: herald_core::merging::UPDATE_WAIT,
        method: None,
        message: Message::Forge,
        may_fail: Vec::new(),
        wait: WAIT,
        retries: None,
        applied_by: AppliedBy::Command,
        role_lost: RoleLost::TakeOut,
        write: None,
    }
}

pub(super) fn base() -> Branched {
    Branched {
        visibility: Visibility::Public,
        head: "b1".to_owned(),
        required: Vec::new(),
        queue: Some(false),
        whole: true,
    }
}

/// What `queue`'s `entries` come to, each gesture's record kept in
/// `store`, written where `applies`; with each record after.
fn judging(
    reader: &Fixed,
    queue: &MergeQueue,
    entries: Vec<Entered>,
    (store, applies): (Option<&FakeStore>, bool),
) -> Vec<(Next, Option<GestureRecord>)> {
    let http = ScriptedHttp::new(Vec::new());
    let keeping = Keeping { store, applies };
    pollster::block_on(judged(
        (reader, &http),
        ("app-main", queue, &keeping),
        (entries, &base()),
        now(),
    ))
    .unwrap()
    .into_iter()
    .map(|(judged, kept)| (judged.next, kept.map(|kept| kept.value)))
    .collect()
}

fn nexts(reader: &Fixed, runs: Runs, entries: Vec<Entered>) -> Vec<Next> {
    judging(reader, &queue(runs), entries, (None, false))
        .into_iter()
        .map(|(next, _)| next)
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

fn found_in(reader: &Fixed, queue: &MergeQueue, printed: Printed) -> Found {
    let http = ScriptedHttp::new(Vec::new());
    let asked = (
        ("app-main", queue),
        None::<&FakeStore>,
        (false, AppliedBy::Command),
    );
    match pollster::block_on(found((reader, &http), asked, printed, now())) {
        Reading::Done(found) => found,
        Reading::Due(Due {
            withheld: Some(withheld),
            ..
        }) => Found::Withheld(withheld),
        Reading::Due(due) => {
            Found::Entries(due.judged.into_iter().map(|(judged, _)| judged).collect())
        }
    }
}

fn found_by(reader: &Fixed, runs: Runs, printed: Printed) -> Found {
    found_in(reader, &queue(runs), printed)
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
fn without_a_store_s_record_of_the_first_seen_head_those_settings_vouch_for_none() {
    let reader = Fixed::proving(vec![(1, green(CheckState::Passed, true))]);
    for vouches in [Vouches::Seen, Vouches::Approved] {
        let mut asked = queue(Runs::Herald);
        asked.vouches = vouches;
        let mut approved = entry(1, ("h1", "h1"), true);
        approved.approved = true;
        for applies in [false, true] {
            let judged = judging(&reader, &asked, vec![approved.clone()], (None, applies));
            assert_eq!(
                judged,
                [(Next::Verdict(Verdict::Leaves(Left::Unrecorded)), None)]
            );
        }
    }
}

#[test]
fn a_queue_s_read_asks_for_its_gesture_role_vouching_and_skew() {
    let reader = Fixed::listing(Some(base()), Some(Vec::new()));
    let mut asked = queue(Runs::Herald);
    asked.enter = Enter::Comment("/merge".to_owned());
    asked.least = Permission::Maintain;
    asked.vouches = Vouches::Seen;
    asked.skew = SignedDuration::from_secs(30);
    assert_eq!(
        found_in(&reader, &asked, Printed::Asked),
        Found::Entries(Vec::new())
    );
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

pub(super) fn record(seen: &str, outcome: Option<HandedOff>) -> GestureRecord {
    GestureRecord {
        key: gesture_key(&queue(Runs::Forge), &entry(1, ("h1", "h1"), true)).unwrap(),
        by: "w".to_owned(),
        at: now() - SignedDuration::from_mins(30),
        queue: "app-main".to_owned(),
        seen: seen.to_owned(),
        seen_at: now(),
        hand_off: outcome.map(|outcome| HandOff {
            head: "h1".to_owned(),
            base: "b1".to_owned(),
            credential: "HERALD_MERGE".to_owned(),
            at: now(),
            outcome,
        }),
        taken_out: Vec::new(),
        own: herald_core::merging::Own::default(),
    }
}

#[test]
fn an_apply_records_the_head_it_first_reads_with_a_gesture_and_a_dry_run_only_reads_it() {
    let reader = Fixed::proving(Vec::new());
    let mut seen = queue(Runs::Forge);
    seen.vouches = Vouches::Seen;
    let store = FakeStore::default();
    let entries = vec![entry(1, ("h1", "h1"), true)];
    assert_eq!(
        judging(&reader, &seen, entries.clone(), (Some(&store), false)),
        [(Next::HandOff, None)]
    );
    let key = gesture_key(&seen, &entries[0]).unwrap();
    assert_eq!(pollster::block_on(store.gesture(&key)).unwrap(), None);
    let applied = judging(&reader, &seen, entries.clone(), (Some(&store), true));
    assert_eq!(applied, [(Next::HandOff, Some(record("h1", None)))]);
    let mut moved = entries[0].clone();
    moved.head = "h2".to_owned();
    assert_eq!(
        judging(&reader, &seen, vec![moved], (Some(&store), true)),
        [(
            Next::Verdict(Verdict::Leaves(Left::HeadMoved)),
            Some(record("h1", None))
        )]
    );
    let gesture = queue(Runs::Forge);
    let fresh = FakeStore::default();
    assert_eq!(
        judging(&reader, &gesture, entries.clone(), (Some(&fresh), true)),
        [(Next::HandOff, None)]
    );
    assert_eq!(pollster::block_on(fresh.gesture(&key)).unwrap(), None);
}

#[test]
fn a_hand_off_recorded_for_a_gesture_is_shown_and_never_due_again_unless_never_sent() {
    let reader = Fixed::proving(Vec::new());
    let forge = queue(Runs::Forge);
    for (outcome, next) in [
        (
            HandedOff::Queued,
            Next::Handed {
                outcome: HandedOff::Queued,
                at: now(),
            },
        ),
        (
            HandedOff::Calling,
            Next::Handed {
                outcome: HandedOff::Calling,
                at: now(),
            },
        ),
        (HandedOff::Unsent, Next::HandOff),
    ] {
        let store = FakeStore::default();
        pollster::block_on(store.put_gesture(&record("h1", Some(outcome)), None)).unwrap();
        let judged = judging(
            &reader,
            &forge,
            vec![entry(1, ("h1", "h1"), true)],
            (Some(&store), false),
        );
        assert_eq!(judged[0].0, next, "{outcome:?}");
    }
}

#[derive(Debug)]
struct Unkept(bool);

impl Actions for Unkept {
    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn action(
        &self,
        _: &herald_core::RunKey,
    ) -> Result<Option<Versioned<herald_core::RerunRecord>>, herald_ports::StoreError> {
        unreachable!("a queue reads no re-run")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_action(
        &self,
        _: &herald_core::RerunRecord,
        _: Option<Version>,
    ) -> Result<Written, herald_ports::StoreError> {
        unreachable!("a queue writes no re-run")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn gesture(
        &self,
        _: &GestureKey,
    ) -> Result<Option<Versioned<GestureRecord>>, herald_ports::StoreError> {
        if self.0 {
            Ok(None)
        } else {
            Err(herald_ports::StoreError::Unavailable {
                reason: "down".to_owned(),
            })
        }
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_gesture(
        &self,
        _: &GestureRecord,
        _: Option<Version>,
    ) -> Result<Written, herald_ports::StoreError> {
        Ok(Written::Conflict)
    }
}

#[test]
fn a_first_seen_head_that_cannot_be_read_or_recorded_vouches_for_none() {
    let mut seen = queue(Runs::Forge);
    seen.vouches = Vouches::Seen;
    let entered = entry(1, ("h1", "h1"), true);
    for (unkept, applies) in [
        (Unkept(false), false),
        (Unkept(false), true),
        (Unkept(true), true),
    ] {
        let keeping = Keeping {
            store: Some(&unkept),
            applies,
        };
        let found = pollster::block_on(super::seen(&keeping, ("app-main", &seen), &entered, now()));
        assert_eq!(found, Ok((None, None)), "{unkept:?} {applies}");
    }
}

#[test]
fn a_queue_says_each_reason_it_takes_no_step_and_holds_back_its_hand_offs() {
    let mut started = queue(Runs::Forge);
    started.method = Some(herald_core::merging::Method::Squash);
    started.write = Some(herald_core::merging::Write {
        token: Some(herald_core::SecretRef {
            secret: "HERALD_MERGE".to_owned(),
        }),
        app: None,
        installation: None,
    });
    let queued = Branched {
        queue: Some(true),
        ..base()
    };
    let store = FakeStore::default();
    assert_eq!(
        idle(&started, (&queued, AppliedBy::Command), Some(&store)),
        Vec::<String>::new()
    );
    let unsaid = Branched {
        queue: None,
        ..base()
    };
    assert_eq!(
        idle(&started, (&unsaid, AppliedBy::Command), Some(&store)),
        Vec::<String>::new()
    );
    let mut gitlab = started.clone();
    gitlab.forge.forge = "gitlab";
    assert_eq!(
        idle(&gitlab, (&unsaid, AppliedBy::Command), Some(&store)),
        [TRAINS_UNSAID]
    );
    assert_eq!(
        idle(&gitlab, (&queued, AppliedBy::Command), Some(&store)),
        Vec::<String>::new()
    );
    gitlab.runs = Runs::Herald;
    assert_eq!(
        idle(&gitlab, (&unsaid, AppliedBy::Command), Some(&store)),
        Vec::<String>::new()
    );
    assert_eq!(
        idle(&started, (&base(), AppliedBy::Command), Some(&store)),
        [NO_FORGE_QUEUE]
    );
    assert_eq!(
        idle(&started, (&queued, AppliedBy::Command), None::<&FakeStore>),
        [UNRECORDED]
    );
    let mut served = started.clone();
    served.applied_by = AppliedBy::Server;
    assert_eq!(
        idle(&served, (&queued, AppliedBy::Command), Some(&store)),
        [SERVED]
    );
    assert_eq!(
        idle(&served, (&queued, AppliedBy::Server), Some(&store)),
        Vec::<String>::new()
    );
    assert_eq!(
        idle(&started, (&queued, AppliedBy::Server), Some(&store)),
        [COMMANDED]
    );
    let mut own = started.clone();
    own.runs = Runs::Herald;
    assert_eq!(
        idle(&own, (&base(), AppliedBy::Command), Some(&store)),
        Vec::<String>::new()
    );
    let bare = queue(Runs::Forge);
    assert_eq!(
        idle(&bare, (&queued, AppliedBy::Command), Some(&store)),
        [METHODLESS.to_owned(), UNWRITTEN.to_owned()]
    );
    let mut forgejo = started;
    forgejo.forge.queueless = Some("Forgejo has none");
    assert_eq!(
        unstarted(&forgejo, false),
        [(
            "runs",
            "Forgejo has none, so with runs = \"forge\" the queue does not start; give runs = \"herald\" to run herald's own".to_owned()
        )]
    );
    forgejo.runs = Runs::Herald;
    assert_eq!(unstarted(&forgejo, false), []);
    let handed = Judged {
        entered: entry(1, ("h1", "h1"), true),
        next: Next::HandOff,
    };
    assert_eq!(idled(handed.clone(), &[]).next, Next::HandOff);
    assert_eq!(idled(handed, &["why".to_owned()]).next, Next::Idle);
    let draft = Judged {
        entered: entry(1, ("h1", "h1"), true),
        next: Next::Draft,
    };
    assert_eq!(idled(draft, &["why".to_owned()]).next, Next::Draft);
}

#[test]
fn a_gesture_the_forge_named_no_id_for_keeps_no_record() {
    let reader = Fixed::proving(Vec::new());
    let mut seen = queue(Runs::Forge);
    seen.vouches = Vouches::Seen;
    let mut unnamed = entry(1, ("h1", "h1"), true);
    unnamed.gesture.id = None;
    let store = FakeStore::default();
    assert_eq!(
        judging(&reader, &seen, vec![unnamed], (Some(&store), true)),
        [(Next::Verdict(Verdict::Leaves(Left::Unrecorded)), None)]
    );
}

#[derive(Debug)]
struct ReadOnly;

impl Actions for ReadOnly {
    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn action(
        &self,
        _: &herald_core::RunKey,
    ) -> Result<Option<Versioned<herald_core::RerunRecord>>, herald_ports::StoreError> {
        unreachable!("a queue reads no re-run")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_action(
        &self,
        _: &herald_core::RerunRecord,
        _: Option<Version>,
    ) -> Result<Written, herald_ports::StoreError> {
        unreachable!("a queue writes no re-run")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn gesture(
        &self,
        _: &GestureKey,
    ) -> Result<Option<Versioned<GestureRecord>>, herald_ports::StoreError> {
        Ok(None)
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_gesture(
        &self,
        _: &GestureRecord,
        _: Option<Version>,
    ) -> Result<Written, herald_ports::StoreError> {
        panic!("a dry run writes no record")
    }
}

#[test]
fn a_dry_run_never_writes_a_record_whatever_its_queue_vouches_for() {
    let reader = Fixed::proving(Vec::new());
    for vouches in [Vouches::Gesture, Vouches::Seen, Vouches::Approved] {
        let mut asked = queue(Runs::Forge);
        asked.vouches = vouches;
        let mut entered = entry(1, ("h1", "h1"), true);
        entered.approved = true;
        let keeping = Keeping {
            store: Some(&ReadOnly),
            applies: false,
        };
        let http = ScriptedHttp::new(Vec::new());
        let judged = pollster::block_on(judged(
            (&reader, &http),
            ("app-main", &asked, &keeping),
            (vec![entered], &base()),
            now(),
        ))
        .unwrap();
        assert_eq!(judged[0].0.next, Next::HandOff, "{vouches:?}");
    }
}

#[test]
fn an_entry_read_in_part_records_no_first_seen_head() {
    let reader = Fixed::proving(Vec::new());
    let mut seen = queue(Runs::Forge);
    seen.vouches = Vouches::Seen;
    let store = FakeStore::default();
    let partial = entry(1, ("h1", "h1"), false);
    assert_eq!(
        judging(&reader, &seen, vec![partial.clone()], (Some(&store), true)),
        [(Next::Verdict(Verdict::Leaves(Left::Unread)), None)]
    );
    let key = gesture_key(&seen, &partial).unwrap();
    assert_eq!(pollster::block_on(store.gesture(&key)).unwrap(), None);
}

pub(super) fn taken_back(role: Option<Permission>) -> TakenBack {
    let canceller = role.map_or(herald_kinds::Holds::Nothing, herald_kinds::Holds::Role);
    TakenBack {
        number: 1,
        url: "u1".to_owned(),
        id: None,
        gestures: vec![herald_kinds::Marked {
            id: Some(10),
            by: "w".to_owned(),
            at: now() - SignedDuration::from_mins(1),
        }],
        cancels: vec![herald_kinds::Marked {
            id: Some(12),
            by: "x".to_owned(),
            at: now(),
        }],
        makers: std::collections::BTreeMap::from([
            ("w".to_owned(), herald_kinds::Holds::Role(Permission::Write)),
            ("x".to_owned(), canceller),
        ]),
        whole: true,
    }
}

#[test]
fn a_take_out_is_held_back_where_the_queue_takes_no_step() {
    let mut gitlab = queue(Runs::Forge);
    gitlab.forge.forge = "gitlab";
    gitlab.method = Some(herald_core::merging::Method::Squash);
    gitlab.write = Some(herald_core::merging::Write {
        token: Some(herald_core::SecretRef {
            secret: "HERALD_MERGE".to_owned(),
        }),
        app: None,
        installation: None,
    });
    let unsaid = Branched {
        queue: None,
        ..base()
    };
    let why = idle(
        &gitlab,
        (&unsaid, AppliedBy::Command),
        Some(&FakeStore::default()),
    );
    assert_eq!(why, [TRAINS_UNSAID]);
    let cancel = |next: Undo| Cancel {
        taken: taken_back(Some(Permission::Write)),
        cancel: None,
        next,
        renamed: None,
    };
    assert_eq!(idled_out(cancel(Undo::TakeOut), &why).next, Undo::Idle);
    assert_eq!(idled_out(cancel(Undo::TakeOut), &[]).next, Undo::TakeOut);
    assert_eq!(idled_out(cancel(Undo::Stays), &why).next, Undo::Stays);
}

#[test]
fn only_a_queue_the_forge_runs_reads_the_forge_s_queue_for_cancels() {
    let mut reader = Fixed::listing(Some(base()), Some(Vec::new()));
    let mut partial = taken_back(Some(Permission::Write));
    partial.whole = false;
    reader.taken = Some(vec![partial]);
    let http = ScriptedHttp::new(Vec::new());
    let cancels = |runs: Runs| {
        let asked = (
            ("app-main", &queue(runs)),
            None::<&FakeStore>,
            (false, AppliedBy::Command),
        );
        match pollster::block_on(found((&reader, &http), asked, Printed::Asked, now())) {
            Reading::Due(due) => due.cancels.unwrap().len(),
            Reading::Done(found) => panic!("{found:?}"),
        }
    };
    assert_eq!(cancels(Runs::Forge), 1);
    assert_eq!(cancels(Runs::Herald), 0);
}

#[test]
fn a_record_naming_another_maker_or_time_for_the_gesture_s_id_writes_nothing() {
    let reader = Fixed::proving(Vec::new());
    let entries = vec![entry(1, ("h1", "h1"), true)];
    for (by, minutes) in [("v", 30), ("w", 31)] {
        let store = FakeStore::default();
        let mut other = record("h1", None);
        other.by = by.to_owned();
        other.at = now() - SignedDuration::from_mins(minutes);
        pollster::block_on(store.put_gesture(&other, None)).unwrap();
        assert_eq!(
            judging(
                &reader,
                &queue(Runs::Forge),
                entries.clone(),
                (Some(&store), true)
            ),
            [(Next::Took(Took::Refused(UNMATCHED.to_owned())), None)]
        );
        let kept = pollster::block_on(store.gesture(&other.key))
            .unwrap()
            .unwrap();
        assert_eq!(kept.value, other);
    }
}

#[test]
fn a_failed_read_of_the_forge_s_queue_leaves_the_entries_judged() {
    let mut reader = Fixed::listing(Some(base()), Some(vec![entry(1, ("h1", "h1"), true)]));
    reader.taken = None;
    let http = ScriptedHttp::new(Vec::new());
    let queue = queue(Runs::Forge);
    let asked = (
        ("app-main", &queue),
        None::<&FakeStore>,
        (false, AppliedBy::Command),
    );
    let Reading::Due(due) =
        pollster::block_on(found((&reader, &http), asked, Printed::Asked, now()))
    else {
        panic!("the entries were not read");
    };
    assert_eq!(due.judged.len(), 1);
    assert!(due.cancels.is_err());
}
