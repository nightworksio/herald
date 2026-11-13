//! Whatever another server sends as an actor's or a key's document, or as
//! an activity, reading it answers or refuses: a key herald takes is held by
//! the actor its id names, or claimed by an owner on the key's own origin;
//! every inbox herald delivers to is on the follower's own origin; and an
//! undo herald takes is the sender's own.

#![no_main]

use herald_dest_fediverse::{
    InboxActivity, KeyOwner, actor_key, actor_of, asked, confirms, document, inboxes, same_origin,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (named, body) = match data.iter().position(|&byte| byte == 0) {
        Some(at) => (&data[..at], &data[at + 1..]),
        None => (&[][..], data),
    };
    let named = String::from_utf8_lossy(named);
    match actor_key(body, &named) {
        Ok(KeyOwner::Held(key)) => assert_eq!(key.actor, document(&named)),
        Ok(KeyOwner::Claimed { owner, .. }) => assert!(same_origin(&owner, &named)),
        Err(_) => {}
    }
    let _ = confirms(body, &named, &named);
    let _ = actor_of(body);
    if let Some(found) = inboxes(body, &named) {
        assert!(same_origin(&found.personal, &named));
        if let Some(shared) = &found.shared {
            assert!(same_origin(shared, &named));
        }
    }
    if let InboxActivity::Undo { follower, .. } = asked(body, &named) {
        assert_eq!(follower, named);
    }
});
