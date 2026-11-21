use jiff::Timestamp;

use super::*;
use crate::queue::tests::pull;
use crate::queue::{Flip, Pull, View, rows};

fn failing(number: u64, names: &[&str]) -> Pull {
    Pull {
        number,
        checks: Checks::Failed(names.iter().map(|name| (*name).to_owned()).collect()),
        ..pull()
    }
}

fn flip(check: &str, second: i64) -> Flip {
    Flip {
        check: check.to_owned(),
        head: "abc123".to_owned(),
        at: Timestamp::from_second(second).unwrap(),
    }
}

#[test]
fn checks_failing_on_the_most_pull_requests_come_first_and_break_their_base_past_its_mark() {
    let view = View::default();
    let now = Timestamp::from_second(10_000).unwrap();
    let pulls = [
        failing(1, &["lint", "test"]),
        failing(2, &["test"]),
        failing(3, &["test"]),
        pull(),
    ];
    let failures = view.triage.failures(&rows(&view, &pulls, now));
    let read: Vec<(&str, usize, bool)> = failures
        .iter()
        .map(|failure| (failure.check.as_str(), failure.pulls.len(), failure.broken))
        .collect();
    assert_eq!(read, [("test", 3, true), ("lint", 1, false)]);
    let alike = Triage {
        broken_at: 4,
        ..Triage::default()
    };
    assert!(!alike.failures(&rows(&view, &pulls, now))[0].broken);
}

#[test]
fn a_check_that_flipped_within_the_window_often_enough_is_a_flaky_candidate() {
    let now = Timestamp::from_second(30 * 86_400).unwrap();
    let recent = now.as_second() - 86_400;
    let old = now.as_second() - 8 * 86_400;
    let flipped = Pull {
        flips: vec![
            flip("test", recent),
            flip("test", old),
            flip("lint", recent),
        ],
        ..failing(1, &["test"])
    };
    let view = View::default();
    let shown = rows(&view, std::slice::from_ref(&flipped), now);
    let kept: Vec<(&str, i64)> = shown[0]
        .pull
        .flips
        .iter()
        .map(|flip| (flip.check.as_str(), flip.at.as_second()))
        .collect();
    assert_eq!(kept, [("test", recent), ("lint", recent)]);
    let failures = view.triage.failures(&shown);
    assert_eq!((failures[0].flips, failures[0].flaky), (1, true));
    let strict = View {
        triage: Triage {
            flaky_least: 2,
            ..Triage::default()
        },
        ..View::default()
    };
    let failures = strict.triage.failures(&rows(&strict, &[flipped], now));
    assert_eq!((failures[0].flips, failures[0].flaky), (1, false));
    assert_eq!(Triage::default().flaky_window.as_hours(), 7 * 24);
}
