use herald_config::{Config, Uses};
use herald_core::{Chore, Entry, Facts, Identity, Key, Retention, RouteId, Schedule, State};
use herald_flows::{Chored, Stop, housekeep, prepare};
use herald_ports::Keyring as _;
use herald_ports::{Admitted, Deliveries, Inboxes, Ledger, Notes, Schedules};
use herald_testing::{FakeClock, FakeSigner};
use jiff::SignedDuration;

use crate::harness::*;
use crate::inbound::{Answering, keepless};

fn schedule(route: &str, thread: &str, chore: Chore, minutes: i64) -> Schedule {
    Schedule {
        key: Key::derive(&RouteId::new(route), &Identity::new(thread)),
        route: RouteId::new(route),
        thread: thread.to_owned(),
        chore,
        due: NOON
            .checked_add(SignedDuration::from_mins(minutes))
            .unwrap(),
    }
}

fn kept(store: &Failing, script: &Script) -> Result<Vec<(String, Chored)>, Stop> {
    kept_for(store, script, &Retention::default())
}

fn kept_for(
    store: &Failing,
    script: &Script,
    retention: &Retention,
) -> Result<Vec<(String, Chored)>, Stop> {
    kept_in(&config(""), store, script, retention)
}

fn kept_in(
    config: &Config,
    store: &Failing,
    script: &Script,
    retention: &Retention,
) -> Result<Vec<(String, Chored)>, Stop> {
    let prepared =
        pollster::block_on(prepare(config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    housekept(&connected, store, retention).map(|kept| {
        kept.kept
            .into_iter()
            .map(|kept| (kept.schedule.thread, kept.outcome))
            .collect()
    })
}

const PATCHED: (&str, u16, &str) = ("/channels/7", 200, "{}");

#[test]
fn chores_that_fall_due_are_done_through_their_route_and_their_schedules_removed() {
    let store = healthy();
    let later = schedule("releases", "9", Chore::Archive, 60);
    let all = [
        schedule("releases", "7", Chore::Archive, -60),
        schedule("releases", "7", Chore::Lock, -30),
        schedule("removed", "8", Chore::Archive, -10),
        later.clone(),
    ];
    pollster::block_on(store.put_schedules(&all, &[])).unwrap();
    let script = Script::answering(&[PATCHED, PATCHED, PATCHED]);
    assert_eq!(
        kept(&store, &script).unwrap(),
        [
            ("7".to_owned(), Chored::Done),
            ("7".to_owned(), Chored::Done),
            (
                "8".to_owned(),
                Chored::Dropped("the route is no longer in the config".to_owned())
            ),
        ]
    );
    assert_eq!(pollster::block_on(store.every_schedule()).unwrap(), [later]);
}

#[test]
fn a_round_reports_the_records_the_store_skipped() {
    let store = Failing {
        fault: Fault::Skipped,
        ..healthy()
    };
    let config = config("");
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared
        .connect(|_| Ok::<_, ()>(Script::default()))
        .unwrap();
    let kept = housekept(&connected, &store, &herald_core::Retention::default()).unwrap();
    assert_eq!(kept.skipped, ["latest/bad.json"]);
}

#[test]
fn a_chore_refused_is_dropped_and_one_asked_to_wait_holds_the_rest_for_later() {
    let store = healthy();
    let all = [
        schedule("releases", "7", Chore::Archive, -60),
        schedule("releases", "6", Chore::Archive, -50),
        schedule("releases", "5", Chore::Archive, -40),
    ];
    pollster::block_on(store.put_schedules(&all, &[])).unwrap();
    let script = Script::answering(&[
        (
            "/channels/7",
            404,
            r#"{"code":10003,"message":"Unknown Channel"}"#,
        ),
        ("/channels/6", 429, r#"{"retry_after":3}"#),
    ]);
    let wait = Stop::RetryAfter(SignedDuration::from_secs(3));
    assert_eq!(
        kept(&store, &script).unwrap(),
        [
            (
                "7".to_owned(),
                Chored::Dropped("Discord refused it with 404: 10003 Unknown Channel; check the names and ids the config gives".to_owned())
            ),
            ("6".to_owned(), Chored::Later(wait.clone())),
            ("5".to_owned(), Chored::Later(wait)),
        ]
    );
    assert_eq!(pollster::block_on(store.every_schedule()).unwrap().len(), 2);
}

#[test]
fn a_destination_asking_to_wait_holds_only_its_own_chores_for_later() {
    let config = config(
        r#"[destinations.second]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[[routes]]
id = "elsewhere"
sources = ["gate"]
destination = "second"
channel = "1566200000000000200"
template = "release""#,
    );
    let store = healthy();
    let all = [
        schedule("releases", "7", Chore::Archive, -60),
        schedule("elsewhere", "6", Chore::Archive, -50),
        schedule("releases", "5", Chore::Archive, -40),
        schedule("elsewhere", "4", Chore::Archive, -30),
    ];
    pollster::block_on(store.put_schedules(&all, &[])).unwrap();
    let script = Script::answering(&[
        ("/channels/7", 429, r#"{"retry_after":3}"#),
        ("/channels/6", 200, "{}"),
        ("/channels/4", 200, "{}"),
    ]);
    let wait = Stop::RetryAfter(SignedDuration::from_secs(3));
    assert_eq!(
        kept_in(&config, &store, &script, &Retention::default()).unwrap(),
        [
            ("7".to_owned(), Chored::Later(wait.clone())),
            ("6".to_owned(), Chored::Done),
            ("5".to_owned(), Chored::Later(wait)),
            ("4".to_owned(), Chored::Done),
        ]
    );
    assert_eq!(script.seen().len(), 3);
    assert_eq!(
        pollster::block_on(store.every_schedule())
            .unwrap()
            .into_iter()
            .map(|schedule| schedule.thread)
            .collect::<Vec<_>>(),
        ["7", "5"]
    );
}

#[test]
fn keys_that_cannot_be_forgotten_stop_housekeeping() {
    let config = config("");
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared
        .connect(|_| Ok::<_, ()>(Script::default()))
        .unwrap();
    let store = healthy();
    pollster::block_on(store.put_schedules(&[schedule("releases", "7", Chore::Archive, -1)], &[]))
        .unwrap();
    assert_eq!(
        pollster::block_on(housekeep(
            &connected,
            &store,
            &FakeClock::at(NOON),
            &keys_in(&Answering {
                forget: Err(broken()),
                ..keepless()
            }),
            (&Retention::default(), herald_config::Federation::KEEP_AUDIT),
        )),
        Err(Stop::Store(broken().to_string()))
    );
    assert_eq!(pollster::block_on(store.every_schedule()).unwrap().len(), 1);
}

#[test]
fn housekeeping_that_cannot_reach_its_destination_or_its_store_keeps_its_schedules() {
    let store = healthy();
    pollster::block_on(store.put_schedules(&[schedule("releases", "7", Chore::Archive, -1)], &[]))
        .unwrap();
    let unreachable = kept(&store, &Script::default()).unwrap();
    assert!(matches!(
        &unreachable[0].1,
        Chored::Later(Stop::Unreachable(_))
    ));
    let failing = Failing {
        fault: Fault::Schedules,
        ..healthy()
    };
    pollster::block_on(
        failing
            .inner
            .put_schedules(&[schedule("releases", "7", Chore::Archive, -1)], &[]),
    )
    .unwrap();
    assert_eq!(
        kept(&failing, &Script::answering(&[PATCHED])).unwrap()[0].1,
        Chored::Later(Stop::Store(
            "the store could not be reached: broken".to_owned()
        ))
    );
    let listing = Failing {
        fault: Fault::ListsSchedules,
        ..healthy()
    };
    assert_eq!(
        kept(&listing, &Script::default()),
        Err(Stop::Store(
            "the store could not be reached: broken".to_owned()
        ))
    );
}

#[test]
fn housekeeping_forgets_a_delivery_admitted_more_than_a_week_ago_and_keeps_the_rest() {
    let store = healthy();
    let day = SignedDuration::from_hours(24);
    let week_and_a_day = NOON.checked_sub(Retention::DELIVERIES + day).unwrap();
    let six_days = NOON.checked_sub(Retention::DELIVERIES - day).unwrap();
    pollster::block_on(store.admit(
        herald_sources_forge::Forge::GitHub.scheme().name,
        "old",
        week_and_a_day,
    ))
    .unwrap();
    pollster::block_on(store.admit(
        herald_sources_forge::Forge::GitHub.scheme().name,
        "recent",
        six_days,
    ))
    .unwrap();
    assert_eq!(kept(&store, &Script::answering(&[])).unwrap(), []);
    assert_eq!(
        pollster::block_on(store.admit(
            herald_sources_forge::Forge::GitHub.scheme().name,
            "old",
            NOON
        ))
        .unwrap(),
        Admitted::Fresh
    );
    assert_eq!(
        pollster::block_on(store.admit(
            herald_sources_forge::Forge::GitHub.scheme().name,
            "recent",
            NOON
        ))
        .unwrap(),
        Admitted::Seen
    );
}

#[test]
fn deliveries_that_cannot_be_expired_stop_housekeeping() {
    let store = Failing {
        fault: Fault::ExpiresDeliveries,
        ..healthy()
    };
    assert_eq!(
        kept(&store, &Script::answering(&[])),
        Err(Stop::Store(broken().to_string()))
    );
}

#[test]
fn a_store_keeping_no_deliveries_is_never_asked_to_expire_them() {
    let store = Failing {
        fault: Fault::KeepsNoDeliveries,
        ..healthy()
    };
    assert_eq!(kept(&store, &Script::answering(&[])), Ok(Vec::new()));
}

fn days(count: i64) -> SignedDuration {
    SignedDuration::from_hours(24 * count)
}

fn ago(count: i64) -> jiff::Timestamp {
    NOON.checked_sub(days(count)).unwrap()
}

fn claimed(identity: &str, days_ago: i64) -> Entry {
    Entry::new(Key::derive(
        &RouteId::new("releases"),
        &Identity::new(identity),
    ))
    .claimed_by("run", ago(days_ago), ago(days_ago))
}

#[test]
fn housekeeping_forgets_deliveries_as_long_ago_as_the_store_is_set_to_keep_them() {
    let store = healthy();
    pollster::block_on(store.admit(
        herald_sources_forge::Forge::GitHub.scheme().name,
        "old",
        ago(4),
    ))
    .unwrap();
    pollster::block_on(store.admit(
        herald_sources_forge::Forge::GitHub.scheme().name,
        "recent",
        ago(2),
    ))
    .unwrap();
    let retention = Retention {
        deliveries: days(3),
        ..Retention::default()
    };
    assert_eq!(
        kept_for(&store, &Script::default(), &retention).unwrap(),
        []
    );
    assert_eq!(
        pollster::block_on(store.admit(
            herald_sources_forge::Forge::GitHub.scheme().name,
            "old",
            NOON
        ))
        .unwrap(),
        Admitted::Fresh
    );
    assert_eq!(
        pollster::block_on(store.admit(
            herald_sources_forge::Forge::GitHub.scheme().name,
            "recent",
            NOON
        ))
        .unwrap(),
        Admitted::Seen
    );
}

#[test]
fn housekeeping_forgets_entries_no_run_claimed_for_90_days_with_their_deliveries_to_inboxes() {
    let store = healthy();
    let old = claimed("old", 91);
    let recent = claimed("recent", 89);
    let landed = State::Landed {
        facts: Facts::default(),
    };
    for entry in [&old, &recent] {
        pollster::block_on(store.put_entry(entry, None)).unwrap();
        pollster::block_on(store.put_delivery(&entry.key, "https://a.example/inbox", &landed))
            .unwrap();
    }
    assert_eq!(kept(&store, &Script::default()).unwrap(), []);
    assert_eq!(pollster::block_on(store.entry(&old.key)).unwrap(), None);
    assert_eq!(
        pollster::block_on(store.delivery(&old.key, "https://a.example/inbox")).unwrap(),
        None
    );
    assert!(
        pollster::block_on(store.entry(&recent.key))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        pollster::block_on(store.delivery(&recent.key, "https://a.example/inbox")).unwrap(),
        Some(landed)
    );
}

#[test]
fn a_store_keeping_no_inboxes_forgets_entries_without_asking_after_their_deliveries() {
    let store = Failing {
        fault: Fault::KeepsNoInboxes,
        ..healthy()
    };
    let old = claimed("old", 91);
    pollster::block_on(store.put_entry(&old, None)).unwrap();
    assert_eq!(kept(&store, &Script::default()).unwrap(), []);
    assert_eq!(pollster::block_on(store.entry(&old.key)).unwrap(), None);
}

#[test]
fn housekeeping_keeps_every_entry_and_note_where_they_are_kept_for_ever() {
    let store = healthy();
    let old = claimed("old", 10_000);
    pollster::block_on(store.put_entry(&old, None)).unwrap();
    pollster::block_on(store.put_note("herald", &old.key, "{}", ago(10_000))).unwrap();
    let retention = Retention {
        ledger: None,
        ..Retention::default()
    };
    assert_eq!(
        kept_for(&store, &Script::default(), &retention).unwrap(),
        []
    );
    assert!(pollster::block_on(store.entry(&old.key)).unwrap().is_some());
    assert_eq!(
        pollster::block_on(store.note(&old.key)).unwrap().as_deref(),
        Some("{}")
    );
}

#[test]
fn housekeeping_forgets_notes_published_longer_ago_than_the_store_keeps_them() {
    let store = healthy();
    let old = claimed("old", 10);
    let recent = claimed("recent", 5);
    pollster::block_on(store.put_note("herald", &old.key, "{\"n\":1}", ago(10))).unwrap();
    pollster::block_on(store.put_note("herald", &recent.key, "{\"n\":2}", ago(5))).unwrap();
    let retention = Retention {
        notes: Some(days(7)),
        ..Retention::default()
    };
    assert_eq!(
        kept_for(&store, &Script::default(), &retention).unwrap(),
        []
    );
    assert_eq!(pollster::block_on(store.note(&old.key)).unwrap(), None);
    assert_eq!(
        pollster::block_on(store.note(&recent.key))
            .unwrap()
            .as_deref(),
        Some("{\"n\":2}")
    );
}

#[test]
fn housekeeping_forgets_decisions_made_longer_ago_than_the_store_keeps_them() {
    let store = healthy();
    let made = |task: &str, days_ago: i64| herald_core::Decided {
        key: herald_core::Key::decided(task, "github.com/o/r/pull/1", "{}"),
        task: task.to_owned(),
        subject: "github.com/o/r/pull/1".to_owned(),
        answer: herald_core::Answer {
            model: "m".to_owned(),
            choice: None,
            probabilities: std::collections::BTreeMap::new(),
        },
        at: ago(days_ago),
    };
    let (old, recent) = (made("old", 31), made("recent", 29));
    for decided in [&old, &recent] {
        pollster::block_on(herald_ports::Decisions::put_decision(&store, decided)).unwrap();
    }
    assert_eq!(
        kept_for(&store, &Script::default(), &Retention::default()).unwrap(),
        []
    );
    let kept = |key| pollster::block_on(herald_ports::Decisions::decision(&store, key)).unwrap();
    assert_eq!(kept(&old.key), None);
    assert_eq!(kept(&recent.key), Some(recent.clone()));
    let forever = Retention {
        decisions: None,
        ..Retention::default()
    };
    let older = made("older", 400);
    pollster::block_on(herald_ports::Decisions::put_decision(&store, &older)).unwrap();
    assert_eq!(kept_for(&store, &Script::default(), &forever).unwrap(), []);
    assert_eq!(kept(&older.key), Some(older.clone()));
}

#[test]
fn housekeeping_forgets_pull_requests_closed_longer_ago_than_the_store_keeps_them() {
    let store = healthy();
    let kept = |number: u64, closed: Option<i64>| {
        let pull = herald_core::Pull {
            project: "r".to_owned(),
            repository: "o/r".to_owned(),
            visibility: herald_core::Visibility::Public,
            number,
            title: "t".to_owned(),
            url: format!("https://github.com/o/r/pull/{number}"),
            author: None,
            bot: false,
            draft: false,
            labels: Vec::new(),
            opened: ago(40),
            updated: ago(40),
            review: herald_core::Review::Unasked,
            reviewers: Vec::new(),
            checks: herald_core::Checks::Absent,
            mergeable: herald_core::Mergeable::Unknown,
            queue: None,
            forge: None,
            head: None,
            unread: false,
            base: None,
            files: None,
            areas: Vec::new(),
            head_branch: None,
            milestone: None,
            flips: Vec::new(),
        };
        let key = herald_core::PullKey {
            repository: herald_core::RepositoryKey::of(&pull.url, "1").unwrap(),
            number,
        };
        let read = herald_core::KeptPull::read(None, key, pull, ago(40));
        match closed {
            Some(days) => read.vanished(ago(days)),
            None => read,
        }
    };
    let (old, recent, open) = (kept(1, Some(31)), kept(2, Some(29)), kept(3, None));
    for kept in [&old, &recent, &open] {
        pollster::block_on(herald_ports::Pulls::put_pull(&store, kept, None)).unwrap();
    }
    assert_eq!(
        kept_for(&store, &Script::default(), &Retention::default()).unwrap(),
        []
    );
    let read = |kept: &herald_core::KeptPull| {
        pollster::block_on(herald_ports::Pulls::pull(&store, &kept.key))
            .unwrap()
            .map(|read| read.value)
    };
    assert_eq!(read(&old), None);
    assert_eq!(read(&recent), Some(recent.clone()));
    assert_eq!(read(&open), Some(open.clone()));
    let forever = Retention {
        pulls: None,
        ..Retention::default()
    };
    let older = kept(4, Some(400));
    pollster::block_on(herald_ports::Pulls::put_pull(&store, &older, None)).unwrap();
    assert_eq!(kept_for(&store, &Script::default(), &forever).unwrap(), []);
    assert_eq!(read(&older), Some(older.clone()));
}

#[test]
fn what_cannot_be_expired_stops_housekeeping() {
    let retention = Retention {
        notes: Some(days(7)),
        ..Retention::default()
    };
    for fault in [
        Fault::ExpiresEntries,
        Fault::ForgetsDeliveries,
        Fault::ExpiresNotes,
        Fault::ExpiresAudits,
        Fault::ExpiresDecisions,
        Fault::ExpiresPulls,
    ] {
        let store = Failing { fault, ..healthy() };
        pollster::block_on(store.put_entry(&claimed("old", 91), None)).unwrap();
        assert_eq!(
            kept_for(&store, &Script::default(), &retention),
            Err(Stop::Store(broken().to_string()))
        );
    }
}

#[test]
fn housekeeping_forgets_the_keys_read_longer_ago_than_their_refresh() {
    let keyring = herald_records::MemoryKeyring::new(4);
    let read = |minutes: i64| herald_ports::KnownKey {
        owner: "https://social.example/users/ann".to_owned(),
        pem: "key".to_owned(),
        fetched: NOON
            .checked_sub(SignedDuration::from_mins(minutes))
            .unwrap(),
    };
    pollster::block_on(keyring.know("stale", &read(61))).unwrap();
    pollster::block_on(keyring.know("fresh", &read(59))).unwrap();
    let config = config("");
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared
        .connect(|_| Ok::<_, ()>(Script::default()))
        .unwrap();
    pollster::block_on(housekeep(
        &connected,
        &healthy(),
        &FakeClock::at(NOON),
        &keys_in(&keyring),
        (&Retention::default(), herald_config::Federation::KEEP_AUDIT),
    ))
    .unwrap();
    assert_eq!(pollster::block_on(keyring.known("stale")).unwrap(), None);
    assert_eq!(
        pollster::block_on(keyring.known("fresh")).unwrap(),
        Some(read(59))
    );
}

#[test]
fn housekeeping_forgets_each_peer_s_audit_of_a_day_longer_ago_than_keep_audit() {
    use herald_ports::Published as _;
    let store = healthy();
    let day = |days_ago: i64| {
        herald_core::AuditDay::of(
            "upstream",
            NOON.checked_sub(SignedDuration::from_hours(24 * days_ago))
                .unwrap(),
        )
    };
    for days_ago in [91, 90, 1] {
        pollster::block_on(store.put_audit(&day(days_ago), None)).unwrap();
    }
    kept_for(&store, &Script::default(), &Retention::default()).unwrap();
    let kept: Vec<String> = pollster::block_on(store.audits("upstream"))
        .unwrap()
        .into_iter()
        .map(|audit| audit.day)
        .collect();
    assert_eq!(kept, [day(90).day, day(1).day]);
}

#[test]
fn a_store_keeping_no_audit_is_housekept_all_the_same() {
    let store = Failing {
        fault: Fault::KeepsNoPublished,
        ..healthy()
    };
    assert!(kept_for(&store, &Script::default(), &Retention::default()).is_ok());
}
