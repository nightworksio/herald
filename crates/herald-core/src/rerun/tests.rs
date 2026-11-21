use super::*;
use crate::queue::tests::pull;

#[test]
fn a_plan_names_its_check_evidence_and_exact_call_and_is_announced_once_a_run_and_head() {
    let pull = pull();
    let rerun = Rerun {
        run: "42".to_owned(),
        job: "420".to_owned(),
        page: "https://github.com/nightworksio/herald/actions/runs/42".to_owned(),
        call: "POST https://api.github.com/repos/nightworksio/herald/actions/jobs/420/rerun"
            .to_owned(),
    };
    let plan = RerunPlan {
        pull: &pull,
        head: "abc123",
        check: "test",
        flips: 2,
        run: Ok(&rerun),
    };
    let event = rerun_event("flaky", plan).unwrap();
    assert_eq!(event.kind(), &Kind::RERUN);
    assert_eq!(
        event.identity().as_str(),
        "rerun/flaky/github.com/nightworksio/herald/pull/7/test/abc123/42"
    );
    assert_eq!(
        event.attribute(CALL),
        Some(&Attribute::Text(rerun.call.clone()))
    );
    assert_eq!(
        event.attribute(RUN_ID),
        Some(&Attribute::Text("42".to_owned()))
    );
    assert_eq!(event.attribute(FLIPS), Some(&Attribute::Number(2.into())));
    assert_eq!(event.link(RUN_LINK), Some(rerun.page.as_str()));
    let unplanned = rerun_event(
        "flaky",
        RerunPlan {
            run: Err("Forgejo has no API to re-run a run"),
            ..plan
        },
    )
    .unwrap();
    assert!(unplanned.identity().as_str().ends_with("/test/abc123/none"));
    assert_eq!(unplanned.attribute(CALL), None);
    assert_eq!(
        unplanned.attribute(REASON),
        Some(&Attribute::Text(
            "Forgejo has no API to re-run a run".to_owned()
        ))
    );
    let nameless = Pull {
        project: " ".to_owned(),
        ..pull.clone()
    };
    assert!(
        rerun_event(
            "flaky",
            RerunPlan {
                pull: &nameless,
                ..plan
            }
        )
        .is_err()
    );
}
