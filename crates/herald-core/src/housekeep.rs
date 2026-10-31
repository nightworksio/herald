use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::key::Key;
use crate::route::Thread;

/// What housekeeping does to a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Chore {
    /// Archive it.
    Archive,
    /// Lock it.
    Lock,
}

/// A chore that falls due on an announcement's thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    /// The announcement whose thread it is.
    pub key: Key,
    /// The destination's id of the thread.
    pub thread: String,
    /// What to do to it.
    pub chore: Chore,
    /// When it falls due.
    pub due: Timestamp,
}

/// The chores a route's `thread` asks for on the thread `id`, opened at
/// `opened` for the announcement `key`.
#[must_use]
pub fn schedules(key: Key, thread: &Thread, id: &str, opened: Timestamp) -> Vec<Schedule> {
    [
        (Chore::Archive, thread.archive_after),
        (Chore::Lock, thread.lock_after),
    ]
    .into_iter()
    .filter_map(|(chore, after)| {
        after.map(|after| Schedule {
            key,
            thread: id.to_owned(),
            chore,
            due: later(opened, after),
        })
    })
    .collect()
}

/// The schedules of `schedules` due at `now`, the earliest first.
#[must_use]
pub fn due(schedules: &[Schedule], now: Timestamp) -> Vec<&Schedule> {
    let mut due: Vec<&Schedule> = schedules
        .iter()
        .filter(|schedule| schedule.due <= now)
        .collect();
    due.sort_by_key(|schedule| (schedule.due, schedule.chore));
    due
}

fn later(at: Timestamp, after: SignedDuration) -> Timestamp {
    at.checked_add(after).unwrap_or(Timestamp::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{Identity, RouteId};

    fn key() -> Key {
        Key::derive(&RouteId::new("route"), &Identity::new("identity"))
    }

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn thread(archive_after: Option<SignedDuration>, lock_after: Option<SignedDuration>) -> Thread {
        Thread {
            name: "{{ event.title }}".to_owned(),
            archive_after,
            lock_after,
        }
    }

    #[test]
    fn a_thread_is_archived_and_locked_after_the_routes_durations() {
        let thread = thread(
            Some(SignedDuration::from_hours(24 * 14)),
            Some(SignedDuration::from_hours(24 * 30)),
        );
        assert_eq!(
            schedules(key(), &thread, "42", at("2026-10-31T12:00:00Z")),
            [
                Schedule {
                    key: key(),
                    thread: "42".to_owned(),
                    chore: Chore::Archive,
                    due: at("2026-11-14T12:00:00Z")
                },
                Schedule {
                    key: key(),
                    thread: "42".to_owned(),
                    chore: Chore::Lock,
                    due: at("2026-11-30T12:00:00Z")
                },
            ]
        );
    }

    #[test]
    fn a_thread_with_no_durations_has_no_chores() {
        assert_eq!(
            schedules(key(), &thread(None, None), "42", at("2026-10-31T12:00:00Z")),
            []
        );
    }

    #[test]
    fn a_duration_past_the_end_of_time_falls_due_at_the_end_of_time() {
        let thread = thread(Some(SignedDuration::MAX), None);
        let due = schedules(key(), &thread, "42", at("2026-10-31T12:00:00Z"));
        assert_eq!(
            due.iter().map(|schedule| schedule.due).collect::<Vec<_>>(),
            [Timestamp::MAX]
        );
    }

    #[test]
    fn the_schedules_due_are_those_at_or_before_now_the_earliest_first() {
        let schedule = |chore, due: &str| Schedule {
            key: key(),
            thread: "42".to_owned(),
            chore,
            due: at(due),
        };
        let all = [
            schedule(Chore::Lock, "2026-11-02T00:00:00Z"),
            schedule(Chore::Lock, "2026-11-01T00:00:00Z"),
            schedule(Chore::Archive, "2026-11-01T00:00:00Z"),
            schedule(Chore::Archive, "2026-11-03T00:00:00Z"),
        ];
        let due: Vec<&Schedule> = due(&all, at("2026-11-02T00:00:00Z"));
        assert_eq!(due, [&all[2], &all[1], &all[0]]);
    }

    #[test]
    fn a_schedule_round_trips_through_json() {
        let schedule = Schedule {
            key: key(),
            thread: "42".to_owned(),
            chore: Chore::Lock,
            due: at("2026-11-01T00:00:00Z"),
        };
        let json = serde_json::to_string(&schedule).unwrap();
        assert_eq!(serde_json::from_str::<Schedule>(&json).unwrap(), schedule);
    }
}
