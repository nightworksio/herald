use std::sync::Mutex;

use herald_core::{Chore, Identity, Key, RouteId, Schedule};
use herald_flows::{Chored, Stop, housekeep, prepare};
use herald_ports::Store as _;
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
