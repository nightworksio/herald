use jiff::civil::date;

use super::*;

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn window(schedule: &ReportSchedule, now: &str) -> Option<String> {
    schedule.window(at(now)).map(|start| start.to_string())
}

#[test]
fn a_bare_schedule_begins_a_window_each_day_at_nine_utc() {
    let daily = ReportSchedule::default();
    assert_eq!(
        window(&daily, "2026-11-18T10:00:00Z").as_deref(),
        Some("2026-11-18T09:00:00Z")
    );
    assert_eq!(
        window(&daily, "2026-11-18T09:00:00Z").as_deref(),
        Some("2026-11-18T09:00:00Z")
    );
    assert_eq!(
        window(&daily, "2026-11-18T08:59:59Z").as_deref(),
        Some("2026-11-17T09:00:00Z")
    );
}

#[test]
fn a_day_s_windows_begin_each_step_after_its_first_until_the_day_ends() {
    let quarterly = ReportSchedule {
        every: SignedDuration::from_hours(6),
        at: Time::constant(1, 0, 0, 0),
        ..ReportSchedule::default()
    };
    assert_eq!(
        window(&quarterly, "2026-11-18T13:00:00Z").as_deref(),
        Some("2026-11-18T13:00:00Z")
    );
    assert_eq!(
        window(&quarterly, "2026-11-18T12:59:00Z").as_deref(),
        Some("2026-11-18T07:00:00Z")
    );
    assert_eq!(
        window(&quarterly, "2026-11-18T00:30:00Z").as_deref(),
        Some("2026-11-17T19:00:00Z")
    );
}

#[test]
fn a_step_is_held_between_a_minute_and_a_day() {
    let never = ReportSchedule {
        every: SignedDuration::ZERO,
        at: Time::constant(23, 58, 0, 0),
        ..ReportSchedule::default()
    };
    assert_eq!(
        window(&never, "2026-11-18T23:59:30Z").as_deref(),
        Some("2026-11-18T23:59:00Z")
    );
    let weekly = ReportSchedule {
        every: SignedDuration::from_hours(24 * 7),
        ..ReportSchedule::default()
    };
    assert_eq!(
        window(&weekly, "2026-11-18T10:00:00Z").as_deref(),
        Some("2026-11-18T09:00:00Z")
    );
}

#[test]
fn a_day_holds_a_window_each_minute_and_none_past_its_end() {
    let minutely = ReportSchedule {
        every: SHORTEST,
        at: Time::midnight(),
        ..ReportSchedule::default()
    };
    assert_eq!(
        window(&minutely, "2026-11-18T23:59:30Z").as_deref(),
        Some("2026-11-18T23:59:00Z")
    );
    let monday = date(2026, 11, 16);
    let mondays = ReportSchedule {
        every: DAY,
        at: Time::midnight(),
        days: vec![monday.weekday()],
        ..ReportSchedule::default()
    };
    assert_eq!(
        window(&mondays, "2026-11-17T00:30:00Z").as_deref(),
        Some("2026-11-16T00:00:00Z")
    );
}

#[test]
fn windows_begin_only_on_the_days_named_looking_a_week_back() {
    let today = date(2026, 11, 18);
    let named = today.checked_sub(SignedDuration::from_hours(48)).unwrap();
    let schedule = ReportSchedule {
        days: vec![named.weekday()],
        ..ReportSchedule::default()
    };
    assert_eq!(
        window(&schedule, "2026-11-18T10:00:00Z"),
        Some(format!("{named}T09:00:00Z"))
    );
    let today_only = ReportSchedule {
        days: vec![today.weekday()],
        ..ReportSchedule::default()
    };
    assert_eq!(
        window(&today_only, "2026-11-18T08:00:00Z").as_deref(),
        Some("2026-11-11T09:00:00Z")
    );
    assert_eq!(ReportSchedule::default().window(Timestamp::MIN), None);
}

#[cfg(feature = "tzdb")]
#[test]
fn a_named_zone_reads_the_wall_clock_there_through_a_change_of_offset() {
    let amsterdam = ReportSchedule {
        zone: TimeZone::get("Europe/Amsterdam").unwrap(),
        ..ReportSchedule::default()
    };
    assert_eq!(
        window(&amsterdam, "2026-11-18T10:00:00Z").as_deref(),
        Some("2026-11-18T08:00:00Z")
    );
    assert_eq!(
        window(&amsterdam, "2026-07-18T10:00:00Z").as_deref(),
        Some("2026-07-18T07:00:00Z")
    );
    let half_past_two = ReportSchedule {
        at: Time::constant(2, 30, 0, 0),
        ..amsterdam
    };
    assert_eq!(
        window(&half_past_two, "2026-03-29T12:00:00Z").as_deref(),
        Some("2026-03-29T01:30:00Z")
    );
    assert_eq!(
        window(&half_past_two, "2026-10-25T12:00:00Z").as_deref(),
        Some("2026-10-25T00:30:00Z")
    );
}

#[test]
fn a_window_s_report_is_named_by_its_report_and_its_start() {
    assert_eq!(
        report_identity("queue", at("2026-11-18T09:00:00Z")),
        "report/queue/2026-11-18T09:00:00Z"
    );
}

#[test]
fn utc_is_a_zone_in_every_build_and_an_unknown_name_is_none() {
    assert_eq!(zone("UTC").unwrap(), TimeZone::UTC);
    assert_eq!(zone("utc").unwrap(), TimeZone::UTC);
    assert!(zone("Nowhere/Atlantis").is_err());
}

#[cfg(feature = "tzdb")]
#[test]
fn a_bundled_zone_is_found_by_its_iana_name() {
    assert_eq!(
        zone("Europe/Amsterdam").unwrap().iana_name(),
        Some("Europe/Amsterdam")
    );
}

#[test]
fn a_report_is_an_event_of_its_window_private_where_a_row_is_not_public() {
    use crate::queue::tests::pull;
    use crate::queue::{View, board};
    let start = Timestamp::from_second(86_400).unwrap();
    let shown = board(&View::default(), &[pull()], start);
    let event = report_event(("daily", "queue"), &shown, start, false).unwrap();
    assert_eq!(
        event.identity().as_str(),
        "report/daily/1970-01-02T00:00:00Z"
    );
    assert_eq!(event.kind(), &Kind::REPORT);
    assert_eq!(event.project(), "daily");
    assert_eq!(event.title(), "daily");
    assert_eq!(event.occurred(), start);
    assert_eq!(event.visibility(), Some(Visibility::Public));
    assert_eq!(
        event.attribute(VIEW),
        Some(&Attribute::Text("queue".to_owned()))
    );
    assert_eq!(event.attribute(PULLS), Some(&Attribute::Number(1.into())));
    assert_eq!(event.attribute(STALE), Some(&Attribute::Number(0.into())));
    let hidden = crate::queue::Pull {
        visibility: Visibility::Internal,
        ..pull()
    };
    let shown = board(&View::default(), &[pull(), hidden], start);
    let event = report_event(("daily", "queue"), &shown, start, false).unwrap();
    assert_eq!(event.visibility(), Some(Visibility::Private));
    assert_eq!(
        report_event((" ", "queue"), &shown, start, false),
        Err(Problem::NoProject)
    );
}

#[test]
fn a_living_report_names_no_window_so_every_window_edits_one_post() {
    use crate::queue::tests::pull;
    use crate::queue::{View, board};
    let start = Timestamp::from_second(86_400).unwrap();
    let shown = board(&View::default(), &[pull()], start);
    let event = report_event(("daily", "queue"), &shown, start, true).unwrap();
    assert_eq!(event.identity().as_str(), "report/daily");
    assert_eq!(living_identity("daily"), "report/daily");
    assert_eq!(event.occurred(), start);
}

#[test]
fn a_report_s_event_is_the_same_whatever_advisories_its_view_found_on_every_surface() {
    use crate::queue::tests::pull;
    use crate::queue::{Advised, Advisories, Advisory, Surface, Unread, View, board as boarded};
    let start = Timestamp::from_second(86_400).unwrap();
    let advisory = |number: usize| Advisory {
        repository: format!("o/r{number}"),
        tool: "dependency-scanning".to_owned(),
        severity: Some("critical".to_owned()),
        summary: Some("Prototype pollution".to_owned()),
        state: "open".to_owned(),
        url: format!("https://github.com/o/r/security/dependabot/{number}"),
        opened: start,
    };
    let many = Advisories {
        open: (0..250).map(advisory).collect(),
        unread: vec![Unread {
            repository: "o/vault".to_owned(),
            visibility: Visibility::Private,
            reason: "refused".to_owned(),
        }],
        more: true,
        at: start,
    };
    for surface in Surface::ALL {
        for living in [false, true] {
            let event = |found: &Advisories| {
                let mut shown = boarded(&View::default(), &[pull()], start);
                shown.overviews.advisories = Some(Advised::to(found, surface));
                report_event(("daily", "queue"), &shown, start, living).unwrap()
            };
            let (with, without) = (event(&many), event(&Advisories::none(start)));
            assert_eq!(with, without, "{surface:?} {living}");
            assert_eq!(with.visibility(), Some(Visibility::Public), "{surface:?}");
        }
    }
}
