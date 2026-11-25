use super::*;

fn step(kind: StepKind, head: &str, outcome: Stepped) -> Step {
    Step {
        kind,
        head: head.to_owned(),
        base: "b1".to_owned(),
        clean: true,
        made: None,
        check: None,
        credential: "HERALD_MERGE".to_owned(),
        at: Timestamp::UNIX_EPOCH,
        outcome,
    }
}

fn own(steps: Vec<Step>) -> Own {
    Own { steps, left: None }
}

#[test]
fn a_step_sent_on_a_head_is_never_taken_again_on_it_and_an_unknown_one_holds_every_step() {
    assert!(own(vec![]).holding(StepKind::Update, "h1").is_none());
    let unsent = own(vec![step(StepKind::Update, "h1", Stepped::Unsent)]);
    assert!(unsent.holding(StepKind::Update, "h1").is_none());
    for outcome in [
        Stepped::Took,
        Stepped::Conflict,
        Stepped::Refused,
        Stepped::HeadMoved,
    ] {
        let sent = own(vec![step(StepKind::Update, "h1", outcome)]);
        assert!(
            sent.holding(StepKind::Update, "h1").is_some(),
            "{outcome:?}"
        );
        assert!(
            sent.holding(StepKind::Update, "h2").is_none(),
            "{outcome:?}"
        );
        assert!(sent.holding(StepKind::Merge, "h1").is_none(), "{outcome:?}");
        assert!(!outcome.unknown(), "{outcome:?}");
    }
    for outcome in [Stepped::Calling, Stepped::Unsettled] {
        let unknown = own(vec![step(StepKind::Update, "h1", outcome)]);
        assert_eq!(
            unknown
                .holding(StepKind::Merge, "h2")
                .map(|step| step.outcome),
            Some(outcome)
        );
    }
}

#[test]
fn a_step_that_conflicts_moves_or_is_refused_leaves_with_its_reason() {
    use StepKind::{Merge, Update};
    assert_eq!(Stepped::Conflict.left(Update), Some(Left::Conflict));
    assert_eq!(Stepped::Conflict.left(Merge), Some(Left::Conflict));
    assert_eq!(Stepped::Refused.left(Update), Some(Left::CannotUpdate));
    assert_eq!(Stepped::Refused.left(Merge), Some(Left::MergeRefused));
    assert_eq!(Stepped::HeadMoved.left(Update), Some(Left::HeadMoved));
    assert_eq!(Stepped::HeadMoved.left(Merge), Some(Left::HeadMoved));
    for outcome in [
        Stepped::Calling,
        Stepped::Took,
        Stepped::Unsettled,
        Stepped::Unsent,
    ] {
        assert_eq!(outcome.left(Update), None, "{outcome:?}");
        assert_eq!(outcome.left(Merge), None, "{outcome:?}");
        assert_eq!(outcome.left(StepKind::Rerun), None, "{outcome:?}");
    }
    assert_eq!(
        Stepped::Refused.left(StepKind::Rerun),
        Some(Left::ChecksFailed)
    );
    assert_ne!(Update.said(), Merge.said());
    assert_ne!(StepKind::Rerun.said(), Merge.said());
}

fn rerun_of(head: &str, check: &str, outcome: Stepped) -> Step {
    Step {
        check: Some(check.to_owned()),
        ..step(StepKind::Rerun, head, outcome)
    }
}

#[test]
fn a_re_run_sent_holds_no_further_re_run_and_counts_on_its_own_head_and_check_alone() {
    let reran = own(vec![
        rerun_of("h1", "ci", Stepped::Took),
        rerun_of("h1", "ci", Stepped::Refused),
        rerun_of("h1", "ci", Stepped::Unsent),
        rerun_of("h2", "ci", Stepped::Took),
        rerun_of("h1", "lint", Stepped::Took),
        step(StepKind::Update, "h1", Stepped::Took),
    ]);
    assert!(reran.holding(StepKind::Rerun, "h1").is_none());
    assert_eq!(reran.reruns("ci", "h1"), 2);
    assert_eq!(reran.reruns("lint", "h1"), 1);
    assert_eq!(reran.reruns("ci", "h3"), 0);
    let unnamed = own(vec![step(StepKind::Rerun, "h1", Stepped::Took)]);
    assert_eq!(unnamed.reruns("ci", "h1"), 0);
}

#[test]
fn a_failed_check_is_re_run_where_the_retries_name_every_failed_one_and_times_is_not_reached() {
    let retries = |checks: &[&str], times: u8| Retries {
        checks: checks.iter().map(|&check| check.to_owned()).collect(),
        times: std::num::NonZeroU8::new(times).unwrap(),
    };
    let twice = retries(&["ci", "lint"], 2);
    let once = own(vec![rerun_of("h1", "ci", Stepped::Took)]);
    assert_eq!(rerun(None, &["ci"], None, "h1"), None);
    assert_eq!(rerun(Some(&twice), &[], None, "h1"), None);
    assert_eq!(
        rerun(Some(&twice), &["lint", "ci"], None, "h1"),
        Some("lint")
    );
    assert_eq!(rerun(Some(&twice), &["ci"], Some(&once), "h1"), Some("ci"));
    assert_eq!(rerun(Some(&twice), &["ci", "test"], None, "h1"), None);
    let single = retries(&["ci", "lint"], 1);
    assert_eq!(rerun(Some(&single), &["ci"], Some(&once), "h1"), None);
    assert_eq!(
        rerun(Some(&single), &["lint", "ci"], Some(&once), "h1"),
        None
    );
    assert_eq!(rerun(Some(&single), &["ci"], Some(&once), "h2"), Some("ci"));
}

#[test]
fn an_empty_record_is_written_as_nothing_and_one_with_steps_reads_back_the_same() {
    assert!(Own::default().is_empty());
    let left = Own {
        steps: vec![],
        left: Some(Leaving {
            left: Left::ChecksFailed,
            at: Timestamp::UNIX_EPOCH,
        }),
    };
    assert!(!left.is_empty());
    assert!(!own(vec![step(StepKind::Merge, "h1", Stepped::Took)]).is_empty());
    let written = serde_json::to_string(&Own {
        steps: vec![step(StepKind::Update, "h1", Stepped::HeadMoved)],
        ..left
    })
    .unwrap();
    assert!(written.contains(r#""outcome":"head-moved""#), "{written}");
    assert!(written.contains(r#""left":"checks-failed""#), "{written}");
    let read: Own = serde_json::from_str(&written).unwrap();
    assert_eq!(read.steps.len(), 1);
    assert_eq!(
        read.left.map(|leaving| leaving.left),
        Some(Left::ChecksFailed)
    );
    assert_eq!(serde_json::from_str::<Own>("{}").unwrap(), Own::default());
}

#[test]
fn every_outcome_of_a_step_is_said_in_words_of_its_own() {
    let said: std::collections::BTreeSet<&str> = [
        Stepped::Calling,
        Stepped::Took,
        Stepped::Conflict,
        Stepped::Refused,
        Stepped::HeadMoved,
        Stepped::Unsettled,
        Stepped::Unsent,
    ]
    .iter()
    .map(|outcome| outcome.said())
    .collect();
    assert_eq!(said.len(), 7);
    assert!(said.iter().all(|words| !words.is_empty()));
}
