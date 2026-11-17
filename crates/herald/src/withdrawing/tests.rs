use herald_core::{Identity, RouteId, Step};
use herald_flows::{Announced, Outcome, Withdrawable, WithdrawablePost};

use super::{plan, reported};
use crate::{DONE, STOPPED};

fn identity() -> Identity {
    Identity::new("github.com/1/v1")
}

fn post(steps: Vec<Step>, group: Option<&str>) -> WithdrawablePost {
    WithdrawablePost {
        identity: identity(),
        steps,
        group: group.map(ToOwned::to_owned),
    }
}

#[test]
fn a_plan_names_why_a_route_keeps_a_post_its_group_or_what_is_left_to_delete() {
    let route = |posts, kept: Option<&str>| Withdrawable {
        route: RouteId::new("r"),
        posts,
        kept: kept.map(ToOwned::to_owned),
    };
    assert_eq!(
        plan(&identity(), &route(Vec::new(), Some("it cannot"))),
        ["r: keeps github.com/1/v1, as it cannot"]
    );
    assert_eq!(
        plan(
            &identity(),
            &route(
                vec![
                    post(Vec::new(), Some("v1")),
                    post(Vec::new(), None),
                    post(vec![Step::Delete(0)], None),
                ],
                None
            )
        ),
        [
            "r: would take github.com/1/v1 out of the group v1",
            "r: would mark github.com/1/v1 withdrawn; nothing is left to delete",
            "r: would withdraw github.com/1/v1 (deletion of the post)",
        ]
    );
}

#[test]
fn a_withdrawal_another_run_holds_or_that_stopped_ends_stopped() {
    let mut out = Vec::new();
    let done = |outcome| Announced {
        route: RouteId::new("r"),
        outcome,
    };
    let held = done(Outcome::Held {
        by: "r0".to_owned(),
        until: jiff::Timestamp::UNIX_EPOCH,
    });
    assert_eq!(reported(&identity(), &[held], &mut out), STOPPED);
    let skipped = done(Outcome::Skipped("kept".to_owned()));
    assert_eq!(reported(&identity(), &[skipped], &mut out), DONE);
    assert_eq!(reported(&identity(), &[], &mut out), STOPPED);
}
