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

    use herald_core::merging::{Method, Runs};
    use herald_kinds::{
        Branched, Entered, MergeReader, Methods, Polling, Proved, Queued, Unpolled,
    };
    use herald_ports::{Boxed, DynHttp};
    use herald_testing::ScriptedHttp;

    use crate::prepare::{MergeReading, PreparedSource};

    #[derive(Debug)]
    struct Said {
        branched: Result<Branched, &'static str>,
        methods: Result<Methods, &'static str>,
    }

    fn said(queue: Option<bool>, queued: Queued) -> Said {
        Said {
            branched: Ok(Branched {
                visibility: herald_core::Visibility::Public,
                head: "b1".to_owned(),
                required: Vec::new(),
                queue,
                queued,
                whole: true,
            }),
            methods: Ok(Methods {
                allowed: vec![Method::Squash],
                whole: true,
            }),
        }
    }

    fn queued(unhanded: Option<&str>, only: bool, method: Option<Method>) -> Queued {
        Queued {
            unhanded: unhanded.map(ToOwned::to_owned),
            only,
            method,
        }
    }

    impl MergeReader for Said {
        fn branch<'a>(
            &'a self,
            _: &'a dyn DynHttp,
            _: &'a str,
        ) -> Boxed<'a, Result<Branched, Unpolled>> {
            let said = self
                .branched
                .clone()
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

        fn merged<'a>(&'a self, _: &'a dyn DynHttp, _: u64) -> Boxed<'a, Result<bool, Unpolled>> {
            Box::pin(async { Ok(false) })
        }

        fn forge_made<'a>(
            &'a self,
            _: &'a dyn DynHttp,
            _: (&'a str, Option<&'a str>),
        ) -> Boxed<'a, Result<Option<Vec<String>>, Unpolled>> {
            Box::pin(async { Ok(None) })
        }

        fn methods<'a>(
            &'a self,
            _: &'a dyn DynHttp,
        ) -> Boxed<'a, Result<herald_kinds::Methods, Unpolled>> {
            let said = self
                .methods
                .clone()
                .map_err(|why| Unpolled::Unreadable(why.to_owned()));
            Box::pin(async move { said })
        }

        fn failed<'a>(
            &'a self,
            _: &'a dyn DynHttp,
            _: (&'a str, &'a str),
        ) -> Boxed<'a, Result<herald_kinds::Plan, Unpolled>> {
            Box::pin(async { Err(Unpolled::Unreadable("the runs unread".to_owned())) })
        }
    }

    fn reading(name: &str, (runs, forge): (Runs, &'static str), said: Said) -> MergeReading {
        MergeReading {
            name: name.to_owned(),
            branch: "main".to_owned(),
            runs,
            method: Some(Method::Squash),
            forge,
            queueless: forge == "forgejo",
            reader: Arc::new(said),
        }
    }

    fn doctored(merging: Vec<MergeReading>) -> crate::doctor::Findings {
        let source = PreparedSource {
            name: herald_core::SourceName::new("app"),
            poll: Polling::Unpolled,
            backfill: 0,
            queue: None,
            merging,
            hook: None,
        };
        let http = ScriptedHttp::new(Vec::new());
        pollster::block_on(super::super::merging::forge_said(&[(
            &source,
            &http as &dyn DynHttp,
        )]))
    }

    const FORGE: (Runs, &str) = (Runs::Forge, "github");

    const OWN: (Runs, &str) = (Runs::Herald, "github");

    #[test]
    fn doctor_asks_the_forge_whether_a_forge_run_queue_s_branch_has_a_queue() {
        let refused = Said {
            branched: Err("the forge refused"),
            methods: Err("the settings unread"),
        };
        let findings = doctored(vec![
            reading("on", FORGE, said(Some(true), queued(None, true, None))),
            reading(
                "squashed",
                FORGE,
                said(Some(true), queued(None, true, Some(Method::Squash))),
            ),
            reading("off", FORGE, said(Some(false), Queued::default())),
            reading(
                "personal",
                FORGE,
                said(Some(false), queued(Some("a personal one"), false, None)),
            ),
            reading(
                "server",
                FORGE,
                said(
                    Some(true),
                    queued(Some("no enqueuePullRequest"), true, None),
                ),
            ),
            reading(
                "merged",
                FORGE,
                said(Some(true), queued(None, true, Some(Method::Merge))),
            ),
            reading("unsaid", FORGE, said(None, Queued::default())),
            MergeReading {
                method: None,
                ..reading("bare", FORGE, said(Some(true), Queued::default()))
            },
            reading(
                "train",
                (Runs::Forge, "gitlab"),
                said(None, Queued::default()),
            ),
            reading("refused", FORGE, refused),
            reading(
                "forgejo",
                (Runs::Forge, "forgejo"),
                said(Some(false), Queued::default()),
            ),
        ]);
        let allows = |name: &str| {
            format!("The merge queue {name}: the repository allows a merge by \"squash\".")
        };
        assert_eq!(
            findings.notes,
            [
                allows("on"),
                "The merge queue on: the forge has a merge queue or merge train on main.".to_owned(),
                allows("squashed"),
                "The merge queue squashed: the forge has a merge queue or merge train on main, merging by \"squash\".".to_owned(),
                allows("off"),
                allows("personal"),
                allows("server"),
                allows("merged"),
                allows("unsaid"),
                "The merge queue unsaid: the forge would not say whether main has a merge queue or merge train, as on a branch under classic protection; each pull request is handed to it all the same.".to_owned(),
                "The merge queue bare: the forge has a merge queue or merge train on main.".to_owned(),
                allows("train"),
                allows("forgejo"),
            ]
        );
        let keys: Vec<&str> = findings.gaps.iter().map(|gap| gap.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "merge_queues.off.runs",
                "merge_queues.personal.runs",
                "merge_queues.server.runs",
                "merge_queues.merged.method",
                "merge_queues.train.runs",
                "merge_queues.refused.method",
                "merge_queues.refused",
            ]
        );
        let why: Vec<&str> = findings.gaps.iter().map(|gap| gap.why.as_str()).collect();
        assert!(
            why[0].starts_with("the forge says the branch has no merge queue or merge train, so")
        );
        assert!(why[1].starts_with("a personal one, so with runs"));
        assert!(why[2].starts_with("no enqueuePullRequest, so"));
        assert!(why[3].ends_with("give method = \"merge\""), "{}", why[3]);
        assert!(why[4].starts_with("GitLab does not say"));
        assert_eq!(why[5..], ["the settings unread", "the forge refused"]);
    }

    #[test]
    fn doctor_says_the_forge_refuses_herald_s_own_merge_on_a_branch_only_its_queue_merges() {
        let unsquashed = Said {
            methods: Ok(Methods {
                allowed: vec![Method::Merge],
                whole: true,
            }),
            ..said(Some(true), queued(None, false, None))
        };
        let findings = doctored(vec![
            reading("only", OWN, said(Some(true), queued(None, true, None))),
            reading("free", OWN, said(Some(true), queued(None, false, None))),
            reading("unsquashed", OWN, unsquashed),
            reading(
                "forgejo",
                (Runs::Herald, "forgejo"),
                said(Some(false), Queued::default()),
            ),
        ]);
        let keys: Vec<&str> = findings.gaps.iter().map(|gap| gap.key.as_str()).collect();
        assert_eq!(
            keys,
            ["merge_queues.only.runs", "merge_queues.unsquashed.method"]
        );
        assert!(findings.gaps[0].why.contains("refuses herald's own merge"));
        assert!(
            findings.gaps[1]
                .why
                .contains("does not allow a merge by \"squash\"")
        );
        assert_eq!(findings.notes.len(), 3, "{:?}", findings.notes);
    }
}
