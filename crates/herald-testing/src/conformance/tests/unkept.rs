use super::*;

#[test]
fn every_method_of_a_kind_a_store_keeps_none_of_is_probed() {
    let mut checks = Checks::default();
    let unkept = Option::<&FakeStore>::None;
    pollster::block_on(async {
        crate::conformance::unkept::ledger(&unkept, &mut checks).await;
        crate::conformance::unkept::cursors(&unkept, &mut checks).await;
        crate::conformance::unkept::schedules(&unkept, &mut checks).await;
        crate::conformance::unkept::subscriptions(&unkept, &mut checks).await;
        crate::conformance::unkept::pins(&unkept, &mut checks).await;
        crate::conformance::unkept::positions(&unkept, &mut checks).await;
        crate::conformance::unkept::decisions(&unkept, &mut checks).await;
        crate::conformance::unkept::deliveries(&unkept, &mut checks).await;
        crate::conformance::unkept::notes(&unkept, &mut checks).await;
        crate::conformance::unkept::announcements(&unkept, &mut checks).await;
        crate::conformance::unkept::inboxes(&unkept, &mut checks).await;
        crate::conformance::unkept::pulls(&unkept, &mut checks).await;
    });
    assert_eq!(checks.done(), Ok(()));
    let mut checks = Checks::default();
    let store = FakeStore::default();
    pollster::block_on(crate::conformance::unkept::notes(&store, &mut checks));
    assert_eq!(
        checks.0,
        [
            "note was not refused as unsupported by a store keeping none of its kind",
            "put_note was not refused as unsupported by a store keeping none of its kind",
            "outbox was not refused as unsupported by a store keeping none of its kind",
            "expire_notes was not refused as unsupported by a store keeping none of its kind",
            "forget_note was not refused as unsupported by a store keeping none of its kind",
        ]
    );
}

#[test]
fn a_store_answering_a_kind_it_keeps_none_of_is_refused() {
    let store = FakeStore::default();
    let probed = |checks: Checks, method: &str| {
        assert_eq!(
            checks.done(),
            Err(Failure(format!(
                "{method} was not refused as unsupported by a store keeping none of its kind"
            )))
        );
    };
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::ledger(&store, &mut checks));
    probed(checks, "entry");
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::cursors(&store, &mut checks));
    probed(checks, "cursor");
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::schedules(&store, &mut checks));
    probed(checks, "every_schedule");
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::subscriptions(
        &store,
        &mut checks,
    ));
    probed(checks, "subscribers");
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::pins(&store, &mut checks));
    probed(checks, "pinned");
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::positions(&store, &mut checks));
    probed(checks, "position");
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::decisions(&store, &mut checks));
    probed(checks, "decision");
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::deliveries(&store, &mut checks));
    probed(checks, "admit");
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::notes(&store, &mut checks));
    probed(checks, "note");
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::announcements(
        &store,
        &mut checks,
    ));
    probed(checks, "latest");
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::inboxes(&store, &mut checks));
    probed(checks, "delivery");
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::pulls(&store, &mut checks));
    probed(checks, "pull");
}

#[test]
fn every_method_of_recalls_a_store_keeps_none_of_is_probed() {
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::recalls(
        &Option::<&FakeStore>::None,
        &mut checks,
    ));
    assert_eq!(checks.done(), Ok(()));
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::recalls(
        &FakeStore::default(),
        &mut checks,
    ));
    assert_eq!(
        checks.0,
        [
            "advised was not refused as unsupported by a store keeping none of its kind",
            "put_advised was not refused as unsupported by a store keeping none of its kind",
        ]
    );
}

#[test]
fn every_method_of_the_records_of_re_runs_and_gestures_a_store_keeps_none_of_is_probed() {
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::actions(
        &Option::<&FakeStore>::None,
        &mut checks,
    ));
    assert_eq!(checks.done(), Ok(()));
    let mut checks = Checks::default();
    pollster::block_on(crate::conformance::unkept::actions(
        &FakeStore::default(),
        &mut checks,
    ));
    assert_eq!(
        checks.0,
        [
            "action was not refused as unsupported by a store keeping none of its kind",
            "put_action was not refused as unsupported by a store keeping none of its kind",
            "gesture was not refused as unsupported by a store keeping none of its kind",
            "put_gesture was not refused as unsupported by a store keeping none of its kind",
        ]
    );
}
