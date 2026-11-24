use jiff::civil::date;

use super::*;
use crate::kind::KindSpecs;
use crate::queue::{Held, Judged};

fn readiness(holds: usize, judged: Judged) -> Readiness {
    Readiness {
        holds: vec![Held::Private; holds],
        more: 0,
        judged,
    }
}

fn milestone(title: &str, due: Option<Date>) -> OpenMilestone {
    OpenMilestone {
        repository: "acme/app".to_owned(),
        visibility: Visibility::Public,
        title: title.to_owned(),
        url: Some(format!("https://forge.test/acme/app/milestone/{title}")),
        due,
        overdue: false,
        counts: None,
    }
}

fn header(milestones: Vec<OpenMilestone>) -> Header {
    Header {
        milestones,
        ..Header::default()
    }
}

const TODAY: Date = date(2026, 11, 24);

fn told<'a>(readiness: &'a Readiness, header: Option<&'a Header>) -> Told<'a> {
    Told {
        audience: Visibility::Public,
        readiness,
        header,
    }
}

const READY: Alerting = Alerting {
    ready: true,
    late: None,
};

#[test]
fn a_release_turns_ready_only_from_another_verdict_told_before() {
    let ready = readiness(0, Judged::Whole);
    let cases = [
        (None, false),
        (Some(ReleaseVerdict::Ready), false),
        (Some(ReleaseVerdict::Held), true),
        (Some(ReleaseVerdict::Unknown), true),
    ];
    for (last, alerts) in cases {
        let alerted = readied(READY, told(&ready, None), last, TODAY);
        assert_eq!(alerted == vec![Readied::Ready], alerts, "after {last:?}");
    }
    let off = Alerting::default();
    assert_eq!(
        readied(off, told(&ready, None), Some(ReleaseVerdict::Held), TODAY),
        Vec::<Readied<'_>>::new()
    );
}

#[test]
fn a_release_held_or_not_known_does_not_turn_ready() {
    for judged in [readiness(1, Judged::Whole), readiness(0, Judged::Unknown)] {
        let alerted = readied(
            READY,
            told(&judged, None),
            Some(ReleaseVerdict::Held),
            TODAY,
        );
        assert!(alerted.is_empty(), "{judged:?}");
    }
}

#[test]
fn a_verdict_is_ready_held_or_unknown_as_the_audience_is_told_it() {
    assert_eq!(
        ReleaseVerdict::of(&readiness(0, Judged::Shown)),
        ReleaseVerdict::Ready
    );
    assert_eq!(
        ReleaseVerdict::of(&readiness(2, Judged::Whole)),
        ReleaseVerdict::Held
    );
    assert_eq!(
        ReleaseVerdict::of(&readiness(0, Judged::Unknown)),
        ReleaseVerdict::Unknown
    );
}

#[test]
fn a_held_release_is_late_from_its_days_before_the_first_due_milestone_on() {
    let held = readiness(1, Judged::Whole);
    let late = Alerting {
        ready: false,
        late: Some(7),
    };
    for (ahead, alerts) in [(8, false), (7, true), (6, true), (0, true), (-3, true)] {
        let due = TODAY.checked_add(jiff::Span::new().days(ahead)).unwrap();
        let shown = header(vec![milestone("v3", None), milestone("v2", Some(due))]);
        let alerted = readied(late, told(&held, Some(&shown)), None, TODAY);
        let expected = vec![Readied::Late {
            milestone: &shown.milestones[1],
            days: ahead,
        }];
        assert_eq!(alerted == expected, alerts, "{ahead} days ahead");
    }
}

#[test]
fn only_the_milestone_due_first_is_late_and_only_while_known_held() {
    let held = readiness(1, Judged::Whole);
    let late = Alerting {
        ready: true,
        late: Some(0),
    };
    let mut other = milestone("v2", Some(TODAY));
    other.repository = "acme/web".to_owned();
    let shown = header(vec![
        milestone("v3", Some(date(2026, 12, 1))),
        other,
        milestone("v2", Some(TODAY)),
    ]);
    let alerted = readied(late, told(&held, Some(&shown)), None, TODAY);
    assert_eq!(
        alerted,
        vec![Readied::Late {
            milestone: &shown.milestones[2],
            days: 0,
        }]
    );
    for judged in [readiness(0, Judged::Whole), readiness(0, Judged::Unknown)] {
        let alerted = readied(late, told(&judged, Some(&shown)), None, TODAY);
        assert!(
            !alerted
                .iter()
                .any(|alert| matches!(alert, Readied::Late { .. }))
        );
    }
    let undated = header(vec![milestone("v2", None)]);
    assert_eq!(
        readied(late, told(&held, Some(&undated)), None, TODAY),
        Vec::<Readied<'_>>::new()
    );
    assert_eq!(
        readied(late, told(&held, None), None, TODAY),
        Vec::<Readied<'_>>::new()
    );
}

#[test]
fn an_alert_names_its_audience_and_is_seen_as_it() {
    let mut held = readiness(2, Judged::Whole);
    held.more = 1;
    let shown = header(vec![milestone("v2", Some(date(2026, 11, 30)))]);
    let telling = Told {
        audience: Visibility::Private,
        readiness: &held,
        header: Some(&shown),
    };
    let late = Readied::Late {
        milestone: &shown.milestones[0],
        days: 6,
    };
    let now = Timestamp::from_second(1_800_000_000).unwrap();
    let event = readied_event(("weekly", "release"), telling, late, ("window", now)).unwrap();
    assert_eq!(event.kind(), &Kind::READINESS);
    assert_eq!(
        event.identity().as_str(),
        "rule/weekly/release/private/late/v2/2026-11-30"
    );
    assert_eq!(event.bounded_by(), Some(Visibility::Private));
    let attributes = event.attributes();
    assert_eq!(
        attributes.get(VERDICT),
        Some(&Attribute::Text(LATE_VERDICT.to_owned()))
    );
    assert_eq!(attributes.get(HOLDS), Some(&Attribute::Number(3.into())));
    assert_eq!(attributes.get(DAYS), Some(&Attribute::Number(6.into())));
    assert_eq!(
        attributes.get(DUE),
        Some(&Attribute::Text("2026-11-30".to_owned()))
    );
    assert!(event.links().contains_key(MILESTONE_LINK));
    let specs = KindSpecs::default();
    let spec = specs.get(&Kind::READINESS).unwrap();
    assert_eq!(spec.check(attributes), Ok(()));
    let ready = readied_event(
        ("weekly", "release"),
        told(&readiness(0, Judged::Whole), None),
        Readied::Ready,
        ("window", now),
    )
    .unwrap();
    assert_eq!(
        ready.identity().as_str(),
        "rule/weekly/release/public/ready/window"
    );
    assert_eq!(
        ready.attributes().get(VERDICT),
        Some(&Attribute::Text(READY_VERDICT.to_owned()))
    );
    assert!(ready.attributes().get(DAYS).is_none());
    assert!(Kind::READINESS.own());
}

#[test]
fn a_cursor_kept_before_verdicts_were_remembered_reads_as_none_told_yet() {
    let kept: crate::Cursor = serde_json::from_str(r#"{"seen":["rule/weekly/1"]}"#).unwrap();
    assert!(kept.verdicts.is_empty());
    let last = kept.verdicts.get(&Visibility::Public).copied();
    let ready = readiness(0, Judged::Whole);
    assert_eq!(
        readied(READY, told(&ready, None), last, TODAY),
        Vec::<Readied<'_>>::new()
    );
    let mut remembered = kept;
    remembered
        .verdicts
        .insert(Visibility::Private, ReleaseVerdict::Held);
    let written = serde_json::to_string(&remembered).unwrap();
    assert!(
        written.contains(r#""verdicts":{"private":"held"}"#),
        "{written}"
    );
    let read: crate::Cursor = serde_json::from_str(&written).unwrap();
    assert_eq!(
        read.verdicts.get(&Visibility::Private),
        Some(&ReleaseVerdict::Held)
    );
}
