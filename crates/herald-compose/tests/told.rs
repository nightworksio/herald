//! What a run did, in the words every driver says it.

#[cfg(test)]
mod told {
    use herald_compose::told;
    use herald_core::{
        Asked, Chore, Command, DestinationName, Identity, Key, Place, RouteId, Schedule,
        SourceName, Step,
    };
    use herald_flows::{
        Announced, Answer, Chored, FollowError, Followed, Housekept, Interacted, Kept, Outcome,
        Remembered, Stop, Sweep, Swept, Worked,
    };

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

    fn kept(outcome: Chored) -> Kept {
        Kept {
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
            told::worked(&Worked::Announced(vec![announced(
                Outcome::AlreadyAnnounced
            )])),
            ["releases: already announced; nothing sent"]
        );
        assert_eq!(
            told::worked(&Worked::Followed(Ok(Followed::Undone {
                follower: "ana".to_owned(),
                actor: "gate".to_owned(),
            }))),
            ["gate: ana no longer follows it"]
        );
        assert_eq!(
            told::worked(&Worked::Followed(Ok(Followed::Rejected {
                follower: "bo".to_owned(),
                actor: "gate".to_owned(),
                answered: herald_core::Sent::Landed(herald_core::Facts::default()),
            }))),
            ["gate: bo was turned away, as its followers are full"]
        );
        assert_eq!(
            told::worked(&Worked::Followed(Err(FollowError::Store(
                "down".to_owned()
            ))))
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
            told::worked(&Worked::Interacted(vec![Interacted {
                asked,
                answer,
                sent: Ok(()),
            }])),
            ["chat: latest was answered in 100"]
        );
    }
}
