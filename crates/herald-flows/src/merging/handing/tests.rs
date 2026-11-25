use herald_core::Permission;
use herald_core::merging::Vouches;
use jiff::SignedDuration;

use super::super::tests::{Fixed, entry, queue};
use super::*;
use herald_core::merging::Runs;

fn checked(change: impl Fn(&mut Entered), vouches: Vouches, seen: Option<&str>) -> Option<Took> {
    let mut queued = queue(Runs::Forge);
    queued.vouches = vouches;
    queued.least = Permission::Maintain;
    let entered = {
        let mut entered = entry(7, ("h7", "h7"), true);
        entered.gesture.role = Some(Permission::Maintain);
        entered
    };
    let mut fresh = entered.clone();
    change(&mut fresh);
    unauthorised(&queued, (&entered, &fresh), seen)
}

fn refused(why: &str) -> Took {
    Took::Refused(why.to_owned())
}

#[test]
fn every_input_read_again_right_before_the_step_must_still_stand_at_the_head_handed_over() {
    assert_eq!(checked(|_| {}, Vouches::Gesture, None), None);
    assert_eq!(
        checked(|fresh| fresh.whole = false, Vouches::Gesture, None),
        Some(refused(UNREAD_AGAIN))
    );
    assert_eq!(
        checked(
            |fresh| fresh.gesture.by = "x".to_owned(),
            Vouches::Gesture,
            None
        ),
        Some(refused(REGESTURED))
    );
    for id in [None, Some(71)] {
        assert_eq!(
            checked(|fresh| fresh.gesture.id = id, Vouches::Gesture, None),
            Some(refused(REGESTURED))
        );
    }
    assert_eq!(
        checked(
            |fresh| fresh.gesture.at += SignedDuration::from_secs(1),
            Vouches::Gesture,
            None
        ),
        Some(refused(REGESTURED))
    );
    for role in [None, Some(Permission::Write)] {
        assert_eq!(
            checked(|fresh| fresh.gesture.role = role, Vouches::Gesture, None),
            Some(refused(UNROLED))
        );
    }
    assert_eq!(
        checked(
            |fresh| fresh.gesture.role = Some(Permission::Admin),
            Vouches::Gesture,
            None
        ),
        None
    );
    assert_eq!(
        checked(|fresh| fresh.draft = true, Vouches::Gesture, None),
        Some(refused(DRAFTED))
    );
    assert_eq!(
        checked(|fresh| fresh.head = "h8".to_owned(), Vouches::Gesture, None),
        Some(Took::HeadMoved)
    );
    assert_eq!(
        checked(
            |fresh| fresh.gesture.head = Some("h6".to_owned()),
            Vouches::Gesture,
            None
        ),
        Some(Took::HeadMoved)
    );
    assert_eq!(
        checked(|_| {}, Vouches::Seen, None),
        Some(refused(UNRECORDED_HEAD))
    );
    assert_eq!(checked(|_| {}, Vouches::Seen, Some("h7")), None);
    assert_eq!(
        checked(|_| {}, Vouches::Seen, Some("h6")),
        Some(Took::HeadMoved)
    );
}

#[test]
fn an_approved_head_must_still_be_approved_when_read_again() {
    let approved = checked(|fresh| fresh.approved = true, Vouches::Approved, Some("h7"));
    assert_eq!(approved, None);
    let unapproved = checked(
        |fresh| fresh.approved = false,
        Vouches::Approved,
        Some("h7"),
    );
    assert_eq!(unapproved, Some(Took::HeadMoved));
}

#[test]
fn what_an_apply_took_says_each_outcome() {
    assert_eq!(took(HandedOff::Queued, None, None), Took::Queued);
    assert_eq!(
        took(HandedOff::Refused, Some("no".to_owned()), None),
        Took::Declined("no".to_owned())
    );
    assert_eq!(
        took(HandedOff::Refused, None, None),
        Took::Declined(String::new())
    );
    assert_eq!(took(HandedOff::HeadMoved, None, None), Took::HeadMoved);
    assert_eq!(took(HandedOff::Unsettled, None, None), Took::Unsettled);
    assert_eq!(
        took(HandedOff::Unsent, None, Some("minting failed".to_owned())),
        Took::Refused("minting failed".to_owned())
    );
    assert_eq!(
        took(HandedOff::Unsent, None, None),
        Took::Refused(HandedOff::Unsent.said().to_owned())
    );
}

#[test]
fn a_refusal_read_back_with_another_head_is_a_moved_head() {
    let queued = queue(Runs::Forge);
    let fresh = entry(1, ("h1", "h1"), true);
    let http = herald_testing::ScriptedHttp::new(Vec::new());
    let back = |reader: &Fixed| pollster::block_on(refused_back((reader, &http), &queued, &fresh));
    let moved = Fixed {
        fresh: Some(entry(1, ("h1", "h2"), true)),
        ..Fixed::default()
    };
    assert_eq!(back(&moved), HandedOff::HeadMoved);
    let same = Fixed {
        fresh: Some(fresh.clone()),
        ..Fixed::default()
    };
    assert_eq!(back(&same), HandedOff::Refused);
    assert_eq!(back(&Fixed::default()), HandedOff::Refused);
}
