//! Tests of what doctor finds in a config.

use herald_core::MentionStyle;

use super::*;

const NONE: Features = Features {
    threads: Threads::None,
    crosspost: false,
    pins: Pins::None,
    reads_back: false,
    edits: false,
    mentions: MentionStyle::None,
    interactions: false,
};

const ALL: Features = Features {
    threads: Threads::FromMessage,
    crosspost: true,
    pins: Pins::Many,
    reads_back: true,
    edits: true,
    mentions: MentionStyle::Roles,
    interactions: true,
};

#[test]
fn what_a_destination_lacks_is_only_what_a_bot_adds() {
    assert_eq!(
        lacked(NONE, ALL),
        [
            "crosspost",
            "open a thread from a message",
            "pin",
            "edit a post",
            "read back its posts for markers",
            "take commands",
        ]
    );
    assert_eq!(lacked(ALL, ALL), Vec::<&str>::new());
    assert_eq!(lacked(NONE, NONE), Vec::<&str>::new());
    let replies = Features {
        threads: Threads::Replies,
        ..NONE
    };
    assert_eq!(
        lacked(replies, ALL).get(1),
        Some(&"open a thread from a message")
    );
    assert_eq!(lacked(NONE, replies), Vec::<&str>::new());
    for (having, kept) in [
        (
            Features {
                crosspost: true,
                ..NONE
            },
            "crosspost",
        ),
        (
            Features {
                threads: Threads::FromMessage,
                ..NONE
            },
            "open a thread from a message",
        ),
        (
            Features {
                edits: true,
                ..NONE
            },
            "edit a post",
        ),
        (
            Features {
                reads_back: true,
                ..NONE
            },
            "read back its posts for markers",
        ),
        (
            Features {
                interactions: true,
                ..NONE
            },
            "take commands",
        ),
    ] {
        let lacking = lacked(having, ALL);
        assert_eq!(lacking.len(), 5, "{kept}");
        assert!(!lacking.contains(&kept), "{kept}");
    }
    let one = Features {
        pins: Pins::One,
        ..NONE
    };
    assert_eq!(
        lacked(
            one,
            Features {
                pins: Pins::Many,
                ..NONE
            }
        ),
        Vec::<&str>::new()
    );
}

#[test]
fn names_are_listed_as_a_sentence_and_tiers_as_adr_0002_names_them() {
    assert_eq!(listed(&[], "or"), "");
    assert_eq!(listed(&["a"], "or"), "a");
    assert_eq!(listed(&["a", "b"], "and"), "a and b");
    assert_eq!(listed(&["a", "b", "c"], "or"), "a, b or c");
    assert_eq!(
        [Tier::Actions, Tier::Serverless, Tier::Container].map(Tier::name),
        ["actions", "serverless", "container"]
    );
    assert!(Tier::Actions < Tier::Serverless && Tier::Serverless < Tier::Container);
}

#[test]
fn only_true_says_a_host_s_egress_is_guarded() {
    assert!(egress_guarded(Some("true")));
    assert!(!egress_guarded(Some("yes")));
    assert!(!egress_guarded(None));
}
