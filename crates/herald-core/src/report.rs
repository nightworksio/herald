//! When a report is due: each window begins at a wall-clock time in the
//! report's time zone, on the days it names, and the report of a window is
//! announced once.

use jiff::civil::{Date, Time, Weekday};
use jiff::tz::TimeZone;
use std::collections::BTreeMap;

use jiff::{SignedDuration, Timestamp};

use crate::event::{Draft, Event, Problem};
use crate::kind::{Attribute, Kind, PULLS, STALE, VIEW};
use crate::members::Visibility;
use crate::queue::Board;

/// The longest a report's window is, and its step where absent.
pub const DAY: SignedDuration = SignedDuration::from_hours(24);

/// The shortest a report's window is.
pub const SHORTEST: SignedDuration = SignedDuration::from_mins(1);

/// The wall-clock time a report's first window of a day begins at where
/// absent.
pub const AT: Time = Time::constant(9, 0, 0, 0);

/// How many days back a window's start is looked for, so a report naming
/// one weekday finds last week's.
const LOOKBACK: usize = 7;

/// The most windows a day holds: one each [`SHORTEST`].
const MOST_WINDOWS: i32 = 24 * 60;

/// The zone a report's times are read in where it names none.
pub const UTC: &str = "UTC";

/// What an event's identity begins with where it is a report.
pub const REPORT_PREFIX: &str = "report";

/// When a report's windows begin.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportSchedule {
    /// How long each window lasts within a day, from [`SHORTEST`] to
    /// [`DAY`]: a window begins at `at`, then each `every` after it until
    /// the day ends.
    pub every: SignedDuration,
    /// The wall-clock time a day's first window begins at.
    pub at: Time,
    /// The days windows begin on, every day where empty.
    pub days: Vec<Weekday>,
    /// The time zone `at` and `days` are read in.
    pub zone: TimeZone,
}

impl Default for ReportSchedule {
    fn default() -> Self {
        Self {
            every: DAY,
            at: AT,
            days: Vec::new(),
            zone: TimeZone::UTC,
        }
    }
}

impl ReportSchedule {
    /// When the window `now` falls in began: the latest start at or before
    /// it, none where none began in the last week.
    #[must_use]
    pub fn window(&self, now: Timestamp) -> Option<Timestamp> {
        let today = now.to_zoned(self.zone.clone()).date();
        std::iter::successors(Some(today), |date| date.yesterday().ok())
            .take(LOOKBACK + 1)
            .find_map(|date| {
                self.starts(date)
                    .into_iter()
                    .rev()
                    .find(|start| *start <= now)
            })
    }

    /// The windows that begin on `date`, earliest first: none on a day it
    /// does not name. A wall-clock time a change of offset skips begins
    /// once the clock passes it.
    fn starts(&self, date: Date) -> Vec<Timestamp> {
        if !self.days.is_empty() && !self.days.contains(&date.weekday()) {
            return Vec::new();
        }
        let step = self.every.clamp(SHORTEST, DAY);
        (0..MOST_WINDOWS)
            .map_while(|index| self.at.checked_add(step.checked_mul(index)?).ok())
            .filter_map(|time| date.to_datetime(time).to_zoned(self.zone.clone()).ok())
            .map(|zoned| zoned.timestamp())
            .collect()
    }
}

/// The time zone the IANA database names `name`, where this build bundles
/// the database; `UTC` in every build.
///
/// # Errors
///
/// Where no zone has that name, or this build bundles no database.
pub fn zone(name: &str) -> Result<TimeZone, jiff::Error> {
    if name.eq_ignore_ascii_case(UTC) {
        return Ok(TimeZone::UTC);
    }
    TimeZone::get(name)
}

/// The event announcing report `name`'s window that began at `start`,
/// showing `board` of the view `view`, its project the report's name: private
/// where any row it shows is not public, as a repository whose visibility
/// is unknown is not.
///
/// # Errors
///
/// Where `name` is empty.
pub fn report_event(
    name: &str,
    view: &str,
    board: &Board,
    start: Timestamp,
) -> Result<Event, Problem> {
    let private = board
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .chain(board.batches.iter().flat_map(|batch| &batch.rows))
        .any(|row| row.pull.visibility != Visibility::Public);
    let count = |count: usize| Attribute::Number(u64::try_from(count).unwrap_or(u64::MAX).into());
    let event = Event::normalise(Draft {
        identity: report_identity(name, start),
        kind: Kind::REPORT,
        project: name.to_owned(),
        title: name.to_owned(),
        body: String::new(),
        links: BTreeMap::new(),
        attributes: BTreeMap::from([
            (VIEW.to_owned(), Attribute::Text(view.to_owned())),
            (PULLS.to_owned(), count(board.total())),
            (STALE.to_owned(), count(board.stale)),
        ]),
        occurred: start,
    })?;
    Ok(event.seen_as(if private {
        Visibility::Private
    } else {
        Visibility::Public
    }))
}

/// The identity of report `name`'s window that began at `start`, which
/// never-twice keys its announcement by.
#[must_use]
pub fn report_identity(name: &str, start: Timestamp) -> String {
    format!("{REPORT_PREFIX}/{name}/{start}")
}

#[cfg(test)]
mod tests;
