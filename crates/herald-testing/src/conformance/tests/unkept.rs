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
        crate::conformance::unkept::deliveries(&unkept, &mut checks).await;
        crate::conformance::unkept::notes(&unkept, &mut checks).await;
        crate::conformance::unkept::announcements(&unkept, &mut checks).await;
        crate::conformance::unkept::inboxes(&unkept, &mut checks).await;
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
