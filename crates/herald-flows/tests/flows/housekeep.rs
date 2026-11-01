use std::sync::Mutex;

use herald_core::{Chore, DELIVERIES_KEPT, Identity, Key, RouteId, Schedule, Scheme};
use herald_flows::{Chored, Stop, housekeep, prepare};
use herald_ports::{Admitted, Store as _};
use herald_testing::{FakeClock, FakeSigner, FakeStore};
use jiff::SignedDuration;

use crate::harness::*;

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
    let config = config("");
    let prepared = pollster::block_on(prepare(&config, &secrets(), &FakeSigner)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    pollster::block_on(housekeep(&connected, store, &clock)).map(|kept| {
        kept.into_iter()
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
    assert_eq!(pollster::block_on(store.schedules()).unwrap(), [later]);
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
                Chored::Dropped("Discord refused it with 404: 10003 Unknown Channel".to_owned())
            ),
            ("6".to_owned(), Chored::Later(wait.clone())),
            ("5".to_owned(), Chored::Later(wait)),
        ]
    );
    assert_eq!(pollster::block_on(store.schedules()).unwrap().len(), 2);
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
        inner: FakeStore::default(),
        writes: Mutex::new(0),
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
        inner: FakeStore::default(),
        writes: Mutex::new(0),
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
    let week_and_a_day = NOON.checked_sub(DELIVERIES_KEPT + day).unwrap();
    let six_days = NOON.checked_sub(DELIVERIES_KEPT - day).unwrap();
    pollster::block_on(store.admit(Scheme::GitHub, "old", week_and_a_day)).unwrap();
    pollster::block_on(store.admit(Scheme::GitHub, "recent", six_days)).unwrap();
    assert_eq!(kept(&store, &Script::answering(&[])).unwrap(), []);
    assert_eq!(
        pollster::block_on(store.admit(Scheme::GitHub, "old", NOON)).unwrap(),
        Admitted::Fresh
    );
    assert_eq!(
        pollster::block_on(store.admit(Scheme::GitHub, "recent", NOON)).unwrap(),
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
