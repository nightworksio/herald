//! A push one herald makes, taken by its peer's inbox as it was sent: its
//! signature, digest, nonce and proof hold, and the event is kept seen by
//! those its envelope names.

use herald_core::Visibility;

use super::*;
use crate::pushing::{PUSHED_TO, Sender};

fn delivered(sender: &Sender) -> Answered {
    let [mut headers] = sender.script.asked().try_into().unwrap();
    headers.push(("host".to_owned(), "herald.example".to_owned()));
    let world = world();
    world.at(crate::harness::NOON);
    push(&world, &sender.script.bodies()[0], &headers)
}

#[test]
fn a_push_herald_makes_is_taken_by_the_peer_s_inbox_as_private_as_it_was_sent() {
    let sender = Sender::new(true, &[(PUSHED_TO, 202, "taken")]);
    sender.announced_to_a_peer(&crate::harness::event("v1").seen_as(Visibility::Private));
    let taken = delivered(&sender);
    assert_eq!(taken.reply.status, 202, "{:?}", taken.refused);
    let Some(Job::Pushed {
        event: Some(event), ..
    }) = taken.job
    else {
        panic!("{:?}", taken.job);
    };
    assert_eq!(event.identity().as_str(), "herald/upstream/github.com/1/v1");
    assert_eq!(event.visibility(), Some(Visibility::Private));
    let public = Sender::new(false, &[(PUSHED_TO, 202, "taken")]);
    public.announced_to_a_peer(&crate::harness::event("v1"));
    let Some(Job::Pushed {
        event: Some(event), ..
    }) = delivered(&public).job
    else {
        panic!("not taken");
    };
    assert_eq!(event.visibility(), None);
}
