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

#[test]
fn members_sharing_a_project_collide_unless_an_image_stands_for_the_other_s_repository() {
    use herald_kinds::Standing;
    let at = |path: &str, image: bool| Some(Standing::new("https://api.github.com", path, image));
    let named = |members: &[Option<Standing>]| {
        members
            .iter()
            .map(|standing| ("m".to_owned(), standing.clone()))
            .collect::<Vec<_>>()
    };
    assert!(!super::online::colliding(&named(&[at("o/a", false)])));
    assert!(!super::online::colliding(&named(&[
        at("o/a", false),
        at("o/a", true)
    ])));
    assert!(super::online::colliding(&named(&[
        at("o/a", false),
        at("o/b", true)
    ])));
    assert!(super::online::colliding(&named(&[
        at("o/a", false),
        at("o/a", false)
    ])));
    assert!(super::online::colliding(&named(&[at("o/a", false), None])));
    assert!(super::online::colliding(&named(&[
        at("o/a", true),
        at("o/a", false),
        at("o/a", true)
    ])));
}

mod merge_queues {
    use std::sync::Arc;

    use herald_core::merging::Runs;
    use herald_kinds::{Branched, Entered, MergeReader, Polling, Proved, Unpolled};
    use herald_ports::{Boxed, DynHttp};
    use herald_testing::ScriptedHttp;

    use crate::prepare::{MergeReading, PreparedSource};

    #[derive(Debug)]
    struct Said(Result<Option<bool>, &'static str>);

    impl MergeReader for Said {
        fn branch<'a>(
            &'a self,
            _: &'a dyn DynHttp,
            _: &'a str,
        ) -> Boxed<'a, Result<Branched, Unpolled>> {
            let said = self
                .0
                .map(|queue| Branched {
                    visibility: herald_core::Visibility::Public,
                    head: "b1".to_owned(),
                    required: Vec::new(),
                    queue,
                    whole: true,
                })
                .map_err(|why| Unpolled::Unreadable(why.to_owned()));
            Box::pin(async move { said })
        }

        fn entered<'a>(
            &'a self,
            _: &'a dyn DynHttp,
            _: &'a str,
            _: herald_kinds::EnterAsked<'a>,
        ) -> Boxed<'a, Result<Vec<Entered>, Unpolled>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn proved<'a>(
            &'a self,
            _: &'a dyn DynHttp,
            _: &'a Entered,
            _: &'a Branched,
        ) -> Boxed<'a, Result<Proved, Unpolled>> {
            Box::pin(async { Err(Unpolled::Unreadable("unasked".to_owned())) })
        }

        fn entry<'a>(
            &'a self,
            _: &'a dyn DynHttp,
            _: (&'a str, u64),
            _: herald_kinds::EnterAsked<'a>,
        ) -> Boxed<'a, Result<Option<Entered>, Unpolled>> {
            Box::pin(async { Err(Unpolled::Unreadable("unasked".to_owned())) })
        }

        fn queued<'a>(&'a self, _: &'a dyn DynHttp, _: u64) -> Boxed<'a, Result<bool, Unpolled>> {
            Box::pin(async { Err(Unpolled::Unreadable("unasked".to_owned())) })
        }

        fn taken_back<'a>(
            &'a self,
            _: &'a dyn DynHttp,
            _: &'a str,
            _: herald_kinds::EnterAsked<'a>,
        ) -> Boxed<'a, Result<Vec<herald_kinds::TakenBack>, Unpolled>> {
            Box::pin(async { Err(Unpolled::Unreadable("unasked".to_owned())) })
        }

        fn took_back<'a>(
            &'a self,
            _: &'a dyn DynHttp,
            _: (&'a str, u64),
            _: herald_kinds::EnterAsked<'a>,
        ) -> Boxed<'a, Result<Option<herald_kinds::TakenBack>, Unpolled>> {
            Box::pin(async { Err(Unpolled::Unreadable("unasked".to_owned())) })
        }
    }

    fn reading(name: &str, runs: Runs, queueless: bool, said: Said) -> MergeReading {
        MergeReading {
            name: name.to_owned(),
            branch: "main".to_owned(),
            runs,
            queueless,
            reader: Arc::new(said),
        }
    }

    #[test]
    fn doctor_asks_the_forge_whether_a_forge_run_queue_s_branch_has_a_queue() {
        let source = PreparedSource {
            name: herald_core::SourceName::new("app"),
            poll: Polling::Unpolled,
            backfill: 0,
            queue: None,
            merging: vec![
                reading("on", Runs::Forge, false, Said(Ok(Some(true)))),
                reading("off", Runs::Forge, false, Said(Ok(Some(false)))),
                reading("unsaid", Runs::Forge, false, Said(Ok(None))),
                reading(
                    "refused",
                    Runs::Forge,
                    false,
                    Said(Err("the forge refused")),
                ),
                reading("own", Runs::Herald, false, Said(Ok(Some(false)))),
                reading("forgejo", Runs::Forge, true, Said(Ok(Some(false)))),
            ],
            hook: None,
        };
        let http = ScriptedHttp::new(Vec::new());
        let findings = pollster::block_on(super::super::online::queued(&[(
            &source,
            &http as &dyn DynHttp,
        )]));
        assert_eq!(
            findings.notes,
            [
                "The merge queue on: the forge has a merge queue or merge train on main.",
                "The merge queue unsaid: the forge would not say whether main has a merge queue or merge train, as on a branch under classic protection; each pull request is handed to it all the same.",
            ]
        );
        let keys: Vec<&str> = findings.gaps.iter().map(|gap| gap.key.as_str()).collect();
        assert_eq!(keys, ["merge_queues.off.runs", "merge_queues.refused"]);
        assert!(
            findings.gaps[0]
                .why
                .contains("no merge queue or merge train on main")
        );
        assert_eq!(findings.gaps[1].why, "the forge refused");
    }
}
