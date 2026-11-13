//! What a run did, in the words every driver says it.

#[cfg(test)]
mod told {
    use herald_compose::told;
    use herald_core::{
        Asked, Chore, Command, DestinationName, Identity, Key, Place, RouteId, Schedule,
        SourceName, Step,
    };
    use herald_flows::{
        Announced, Answer, Chored, Did, FollowError, Followed, Housekept, HousekeptSchedule,
        Interacted, Job, Outcome, Remembered, Stop, Sweep, Swept, Worked,
    };

    fn did(did: Did) -> Worked {
        Worked {
            did,
            unreleased: None,
        }
    }

    fn route() -> RouteId {
        RouteId::new("releases")
    }

    fn announced(outcome: Outcome) -> Announced {
        Announced {
            route: route(),
            outcome,
        }
    }

    fn swept(outcome: Sweep) -> Vec<String> {
        told::polled(&Swept {
            source: SourceName::new("gate"),
            outcome,
        })
    }

    fn kept(outcome: Chored) -> HousekeptSchedule {
        HousekeptSchedule {
            schedule: Schedule {
                key: Key::derive(&route(), &Identity::new("1.0.0")),
                route: route(),
                thread: "9".to_owned(),
                chore: Chore::Archive,
                due: jiff::Timestamp::UNIX_EPOCH,
            },
            outcome,
        }
    }

    fn refused() -> Stop {
        Stop::Refused("no".to_owned())
    }

    #[test]
    fn a_poll_says_what_it_found_and_what_became_of_its_cursor() {
        assert_eq!(
            swept(Sweep::Unchanged),
            ["gate: unchanged since the last poll"]
        );
        assert_eq!(
            swept(Sweep::Stopped(refused())),
            ["gate: stopped: no. The next poll asks again."]
        );
        assert_eq!(
            swept(Sweep::Polled {
                first: false,
                found: 2,
                announced: Vec::new(),
                cursor: Remembered::Advanced,
            }),
            ["gate: 2 found, 0 new"]
        );
        assert_eq!(
            swept(Sweep::Polled {
                first: true,
                found: 3,
                announced: Vec::new(),
                cursor: Remembered::Failed(refused()),
            }),
            [
                "gate: first poll found 3; 0 announced, as backfill asks; the cursor stays where it was: no. The next poll offers them again"
            ]
        );
    }

    #[test]
    fn a_chore_says_what_became_of_its_thread() {
        assert_eq!(
            told::chored(&kept(Chored::Done)),
            "releases: thread 9 archived"
        );
        assert_eq!(
            told::kept(&Ok(Housekept {
                kept: vec![kept(Chored::Later(refused()))],
                skipped: vec!["deliveries/app/7.json".to_owned()],
            })),
            [
                "releases: thread 9 not archived yet: no. The next run tries again.",
                "the record deliveries/app/7.json could not be read and was skipped; fix or remove it",
            ]
        );
        assert_eq!(
            told::kept(&Err(refused())),
            ["housekeeping stopped: no. The next round tries again."]
        );
    }

    #[test]
    fn a_stop_says_why_and_steps_are_named() {
        assert_eq!(told::why(&Stop::Source("down".to_owned())), "down");
        assert_eq!(told::why(&Stop::Unread("unread".to_owned())), "unread");
        assert_eq!(
            told::steps(&[
                Step::Post,
                Step::Chunk(2),
                Step::OpenThread,
                Step::Deliveries
            ]),
            "post, chunk 2, thread, deliveries to followers"
        );
    }

    #[test]
    fn a_refused_request_says_why() {
        assert_eq!(
            told::refused("the signature does not match"),
            "a request was refused: the signature does not match"
        );
    }

    #[test]
    fn a_route_says_what_it_did_with_an_announcement() {
        assert_eq!(
            told::report(&announced(Outcome::Sent {
                steps: vec![Step::Post]
            })),
            "releases: announced (post)"
        );
        assert_eq!(
            told::report(&announced(Outcome::Stopped {
                landed: vec![Step::Post],
                remaining: Vec::new(),
                reason: refused(),
            })),
            "releases: stopped: no. Landed: post. The next run sends what remains: nothing."
        );
    }

    #[test]
    fn a_job_says_what_it_did() {
        assert_eq!(
            told::worked(&did(Did::Announced(vec![announced(
                Outcome::AlreadyAnnounced
            )]))),
            ["releases: already announced; nothing sent"]
        );
        assert_eq!(
            told::worked(&did(Did::Followed(Ok(Followed::Undone {
                follower: "ana".to_owned(),
                actor: "gate".to_owned(),
            })))),
            ["gate: ana no longer follows it"]
        );
        assert_eq!(
            told::worked(&did(Did::Followed(Ok(Followed::Rejected {
                follower: "bo".to_owned(),
                actor: "gate".to_owned(),
                answered: herald_core::Sent::Landed(herald_core::Facts::default()),
            })))),
            ["gate: bo was turned away"]
        );
        assert_eq!(
            told::worked(&did(Did::Followed(Ok(Followed::Waiting {
                follower: "cy".to_owned(),
                actor: "gate".to_owned(),
            })))),
            ["gate: cy asks to follow it; approve or reject with `herald followers`"]
        );
        assert_eq!(
            told::worked(&did(Did::Followed(Err(FollowError::Store(
                "down".to_owned()
            )))))
            .len(),
            1
        );
        let asked = Asked {
            destination: DestinationName::new("chat"),
            place: Place::new("100"),
            user: "ana".to_owned(),
            command: Command::Latest,
            project: None,
            share: false,
            toggles: false,
            member: None,
        };
        let answer = Answer {
            said: "1.0.0".to_owned(),
            public: false,
            trouble: None,
        };
        assert_eq!(
            told::worked(&did(Did::Interacted(vec![Interacted {
                asked,
                answer,
                sent: Ok(()),
            }]))),
            ["chat: latest was answered in 100"]
        );
    }

    #[test]
    fn a_delivery_that_could_not_be_released_is_said_after_what_its_job_did() {
        let worked = Worked {
            did: Did::Interacted(Vec::new()),
            unreleased: Some("its delivery could not be released: down".to_owned()),
        };
        assert_eq!(
            told::worked(&worked),
            [
                "its delivery could not be released: down; its sender's next delivery of it is refused until housekeeping forgets it"
            ]
        );
    }

    #[test]
    fn a_job_left_undone_names_what_it_was_and_a_release_that_failed() {
        let event = herald_core::Event::normalise(herald_core::Draft {
            identity: "forge/gate/v1".to_owned(),
            kind: herald_core::Kind::RELEASE,
            project: "gate".to_owned(),
            attributes: herald_core::versioned("1.0.0".to_owned(), false),
            title: "gate".to_owned(),
            body: String::new(),
            links: std::collections::BTreeMap::new(),
            occurred: jiff::Timestamp::UNIX_EPOCH,
        })
        .unwrap();
        let verified = herald_flows::Verified {
            delivery: Some("d".to_owned()),
            signer: None,
        };
        let announcing = Job::App {
            app: "ci".to_owned(),
            event,
            verified: verified.clone(),
        };
        assert_eq!(
            told::unworked(&announcing, &Ok(())),
            "herald stopped before it worked the announcement of forge/gate/v1"
        );
        assert_eq!(
            told::unworked(
                &Job::Follow {
                    verified,
                    body: Vec::new()
                },
                &Err("its delivery could not be released: down".to_owned())
            ),
            "herald stopped before it worked an activity; its delivery could not be released: down"
        );
        assert_eq!(
            told::unworked(
                &Job::Interact {
                    destination: herald_core::DestinationName::new("chat"),
                    asked: Vec::new()
                },
                &Ok(())
            ),
            "herald stopped before it worked the commands given on chat"
        );
    }
}
