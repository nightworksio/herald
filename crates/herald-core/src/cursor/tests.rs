use super::*;
use crate::event::Draft;
use crate::event::tests::draft;

fn event(identity: &str) -> Event {
    Event::normalise(Draft {
        identity: identity.to_owned(),
        ..draft()
    })
    .unwrap()
}

fn identities(events: &[Event]) -> Vec<&str> {
    events
        .iter()
        .map(|event| event.identity().as_str())
        .collect()
}

#[test]
fn a_new_source_announces_only_its_backfill() {
    let found = vec![event("c"), event("b"), event("a")];
    assert_eq!(
        identities(&fresh(None, found.clone(), 0)),
        Vec::<&str>::new()
    );
    assert_eq!(identities(&fresh(None, found, 2)), ["c", "b"]);
}

#[test]
fn an_advanced_cursor_takes_the_newest_mark_its_poll_gives_and_else_keeps_its_own() {
    let then = Timestamp::constant(1_700_000_000, 0);
    let later = Timestamp::constant(1_800_000_000, 0);
    let cursor = Cursor {
        newest: Some(then),
        ..Cursor::default()
    };
    let marked = Polled {
        newest: Some(later),
        ..Polled::default()
    };
    assert_eq!(cursor.advanced(&marked, 500).newest, Some(later));
    assert_eq!(cursor.advanced(&Polled::default(), 500).newest, Some(then));
    assert_eq!(
        Cursor::default().advanced(&Polled::default(), 500).newest,
        None
    );
    let written = serde_json::to_string(&Cursor::default()).unwrap();
    assert!(!written.contains("newest"), "{written}");
    let read: Cursor = serde_json::from_str(&serde_json::to_string(&cursor).unwrap()).unwrap();
    assert_eq!(read.newest, Some(then));
}

#[test]
fn an_advanced_cursor_takes_the_sequence_its_pull_read_and_else_keeps_its_own() {
    let cursor = Cursor {
        sequence: Some(4),
        ..Cursor::default()
    };
    let read = Polled {
        sequence: Some(9),
        ..Polled::default()
    };
    assert_eq!(cursor.advanced(&read, 500).sequence, Some(9));
    assert_eq!(cursor.advanced(&Polled::default(), 500).sequence, Some(4));
    let written = serde_json::to_string(&Cursor::default()).unwrap();
    assert!(!written.contains("sequence"), "{written}");
    let back: Cursor = serde_json::from_str(&serde_json::to_string(&cursor).unwrap()).unwrap();
    assert_eq!(back.sequence, Some(4));
}

#[test]
fn a_known_source_announces_what_its_cursor_has_not_seen() {
    let cursor = Cursor {
        seen: vec![Identity::new("b"), Identity::new("a")],
        ..Cursor::default()
    };
    let found = vec![event("c"), event("b"), event("a")];
    assert_eq!(identities(&fresh(Some(&cursor), found, 0)), ["c"]);
}

#[test]
fn an_advanced_cursor_keeps_the_newest_first_once_each_with_the_new_validators() {
    let tags = Validators {
        etag: Some("tags".to_owned()),
        last_modified: None,
    };
    let cursor = Cursor {
        seen: vec![Identity::new("b"), Identity::new("a")],
        etag: Some("old".to_owned()),
        signals: BTreeMap::from([(Signal::Release, Validators::default())]),
        ..Cursor::default()
    };
    let advanced = cursor.advanced(
        &Polled {
            events: vec![event("c"), event("b")],
            etag: Some("new".to_owned()),
            last_modified: Some("then".to_owned()),
            signals: BTreeMap::from([(Signal::Tag, tags.clone())]),
            passed: vec![Identity::new("b"), Identity::new("p"), Identity::new("a")],
            watched: 0,
            packages: BTreeMap::from([("container/herald".to_owned(), tags.clone())]),
            unread: vec![Identity::new("u"), Identity::new("c")],
            listings: BTreeMap::from([("status".to_owned(), tags.clone())]),
            newest: None,
            sequence: None,
            withdrawn: Vec::new(),
            proved: Vec::new(),
            unheld: Unheld::default(),
            refused: Vec::new(),
            ahead: Vec::new(),
            each: None,
            relisted: false,
        },
        500,
    );
    assert_eq!(
        advanced,
        Cursor {
            seen: vec![
                Identity::new("c"),
                Identity::new("b"),
                Identity::new("p"),
                Identity::new("a"),
                Identity::new("u")
            ],
            etag: Some("new".to_owned()),
            last_modified: Some("then".to_owned()),
            project: Some(event("c").project().to_owned()),
            signals: BTreeMap::from([(Signal::Tag, tags.clone())]),
            notes: BTreeMap::new(),
            tagged: vec![Identity::new("b"), Identity::new("p"), Identity::new("a")],
            packages: BTreeMap::from([("container/herald".to_owned(), tags.clone())]),
            listings: BTreeMap::from([("status".to_owned(), tags.clone())]),
            newest: None,
            sequence: None,
            ahead: Vec::new(),
            proved: Vec::new(),
            projects: BTreeMap::new(),
        }
    );
    assert_eq!(advanced.validators(Signal::Tag), Some(&tags));
    assert_eq!(advanced.validators(Signal::Release), None);
    let quiet = Cursor {
        project: Some("kept".to_owned()),
        ..Cursor::default()
    }
    .advanced(&Polled::default(), 500);
    assert_eq!(quiet.project.as_deref(), Some("kept"));
}

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap()
}

fn proof(identity: &str, second: i64, tombstone: bool) -> Proof {
    Proof {
        identity: Identity::new(identity),
        created: at(second),
        maker: "p".to_owned(),
        tombstone: tombstone.then_some(Tombstone::Held),
        passed: false,
    }
}

fn unheld(identity: &str, second: i64, maker: &str) -> Proof {
    Proof {
        maker: maker.to_owned(),
        tombstone: Some(Tombstone::Unheld),
        ..proof(identity, second, false)
    }
}

#[test]
fn a_withdrawn_identity_is_unseen_and_the_latest_proofs_taken_are_kept() {
    let cursor = Cursor {
        seen: vec![Identity::new("b"), Identity::new("a")],
        proved: vec![
            proof("a", 10, false),
            proof("b", 30, false),
            proof("old", 5, false),
        ],
        ..Cursor::default()
    };
    let polled = Polled {
        events: vec![event("c")],
        withdrawn: vec![Identity::new("a")],
        proved: vec![
            proof("c", 50, false),
            proof("a", 40, true),
            proof("b", 20, false),
        ],
        ..Polled::default()
    };
    let advanced = cursor.advanced(&polled, 2);
    assert_eq!(advanced.seen, [Identity::new("c"), Identity::new("b")]);
    assert_eq!(
        advanced.proved,
        [
            proof("c", 50, false),
            proof("a", 40, true),
            proof("b", 30, false)
        ]
    );
    assert!(advanced.newer(&Identity::new("a"), at(41)));
    assert!(!advanced.newer(&Identity::new("a"), at(40)));
    assert!(!advanced.newer(&Identity::new("a"), at(39)));
    assert!(advanced.newer(&Identity::new("old"), at(1)));
    assert!(advanced.took(&Identity::new("a")));
    assert!(advanced.took(&Identity::new("c")));
    assert!(!advanced.took(&Identity::new("old")));
    assert_eq!(advanced.proof(&Identity::new("old")), None);
    let seen_only = Cursor {
        seen: vec![Identity::new("s")],
        proved: vec![Proof {
            passed: true,
            ..proof("p", 1, false)
        }],
        ..Cursor::default()
    };
    assert!(!seen_only.took(&Identity::new("s")));
    assert!(!seen_only.took(&Identity::new("p")));
    assert!(!seen_only.newer(&Identity::new("p"), at(1)));
}

#[test]
fn a_proof_made_far_ahead_is_forgotten_as_any_other_taken_before_later_ones() {
    let cursor = Cursor {
        proved: vec![
            proof("ahead", 9_000_000_000, false),
            proof("real", 10, false),
        ],
        ..Cursor::default()
    };
    let polled = Polled {
        proved: vec![proof("next", 20, false)],
        ..Polled::default()
    };
    assert_eq!(
        cursor.advanced(&polled, 2).proved,
        [
            proof("next", 20, false),
            proof("ahead", 9_000_000_000, false)
        ]
    );
    assert_eq!(
        cursor.advanced(&polled, 1).proved,
        [proof("next", 20, false)]
    );
}

#[test]
fn a_tombstone_is_never_forgotten_however_many_proofs_follow_it() {
    let cursor = Cursor {
        proved: vec![
            proof("one", 1, false),
            proof("gone", 2, true),
            proof("two", 3, false),
        ],
        ..Cursor::default()
    };
    let polled = Polled {
        proved: vec![proof("new", 4, false), proof("also", 5, true)],
        ..Polled::default()
    };
    let advanced = cursor.advanced(&polled, 1);
    assert_eq!(
        advanced.proved,
        [
            proof("new", 4, false),
            proof("also", 5, true),
            proof("gone", 2, true)
        ]
    );
    assert!(!advanced.newer(&Identity::new("gone"), at(2)));
    assert_eq!(cursor.advanced(&polled, 0).proved.len(), 2);
}

#[test]
fn a_proof_older_than_the_one_kept_leaves_the_kept_one_in_its_new_place() {
    let cursor = Cursor {
        proved: vec![proof("a", 30, true), proof("b", 10, false)],
        ..Cursor::default()
    };
    let polled = Polled {
        proved: vec![proof("b", 20, false), proof("a", 20, false)],
        ..Polled::default()
    };
    assert_eq!(
        cursor.advanced(&polled, 5).proved,
        [proof("b", 20, false), proof("a", 30, true)]
    );
    let equal = Polled {
        proved: vec![proof("a", 30, false)],
        ..Polled::default()
    };
    assert_eq!(
        cursor.advanced(&equal, 5).proved,
        [proof("a", 30, false), proof("b", 10, false)]
    );
}

#[test]
fn the_tombstones_of_identities_never_taken_are_kept_latest_first_within_both_bounds() {
    let cursor = Cursor {
        proved: vec![
            unheld("p3", 3, "p"),
            proof("held", 1, true),
            unheld("q1", 1, "q"),
            unheld("p2", 2, "p"),
            unheld("p1", 1, "p"),
        ],
        ..Cursor::default()
    };
    let polled = |most: usize, each: usize| Polled {
        proved: vec![unheld("p4", 4, "p")],
        unheld: Unheld { most, each },
        ..Polled::default()
    };
    assert_eq!(
        cursor.advanced(&polled(3, 2), 0).proved,
        [
            unheld("p4", 4, "p"),
            unheld("p3", 3, "p"),
            proof("held", 1, true),
            unheld("q1", 1, "q")
        ]
    );
    assert_eq!(
        cursor.advanced(&polled(5, 2), 0).proved,
        [
            unheld("p4", 4, "p"),
            unheld("p3", 3, "p"),
            proof("held", 1, true),
            unheld("q1", 1, "q")
        ]
    );
    assert_eq!(cursor.advanced(&polled(5, 5), 0).proved.len(), 6);
    assert_eq!(
        cursor.advanced(&polled(2, 5), 0).proved,
        [
            unheld("p4", 4, "p"),
            unheld("p3", 3, "p"),
            proof("held", 1, true)
        ]
    );
    assert_eq!(
        cursor.advanced(&polled(0, 5), 0).proved,
        [proof("held", 1, true)]
    );
    assert_eq!(
        cursor.advanced(&Polled::default(), 0).proved,
        [proof("held", 1, true)]
    );
}

#[test]
fn a_tombstone_of_an_identity_never_taken_guards_its_replay_but_takes_nothing() {
    let cursor = Cursor {
        proved: vec![unheld("stranger", 5, "p")],
        ..Cursor::default()
    };
    let stranger = Identity::new("stranger");
    assert!(!cursor.took(&stranger));
    assert!(!cursor.newer(&stranger, at(5)));
    assert!(cursor.newer(&stranger, at(6)));
}

#[test]
fn a_relisting_source_keeps_the_digests_it_watched_for_what_it_still_sees() {
    let watching = Cursor {
        seen: vec![Identity::new("a")],
        ..Cursor::default()
    }
    .advanced(
        &Polled {
            events: vec![signalled("a", Signal::Release)],
            watched: 1,
            relisted: true,
            ..Polled::default()
        },
        2,
    );
    let relisted = |events: Vec<Event>, withdrawn: Vec<Identity>| {
        watching.advanced(
            &Polled {
                watched: events.len(),
                events,
                withdrawn,
                relisted: true,
                ..Polled::default()
            },
            2,
        )
    };
    let both = relisted(vec![signalled("b", Signal::Release)], Vec::new());
    assert_eq!(
        both.notes.keys().collect::<Vec<_>>(),
        [&Identity::new("a"), &Identity::new("b")]
    );
    let quiet = relisted(Vec::new(), Vec::new());
    assert_eq!(quiet.notes, watching.notes);
    let gone = relisted(Vec::new(), vec![Identity::new("a")]);
    assert!(gone.notes.is_empty());
    let unrelisted = watching.advanced(
        &Polled {
            events: vec![signalled("b", Signal::Release)],
            watched: 1,
            ..Polled::default()
        },
        2,
    );
    assert_eq!(
        unrelisted.notes.keys().collect::<Vec<_>>(),
        [&Identity::new("b")]
    );
}

fn signalled(identity: &str, signal: Signal) -> Event {
    let mut attributes = crate::versioned("v1", false);
    attributes.insert(
        crate::SIGNAL.to_owned(),
        crate::Attribute::Text(signal.name().to_owned()),
    );
    Event::normalise(Draft {
        identity: identity.to_owned(),
        attributes,
        ..draft()
    })
    .unwrap()
}

#[test]
fn a_release_of_a_version_seen_only_as_a_tag_is_unseen_until_it_is_seen() {
    let polled = |events: Vec<Event>| Polled {
        events,
        ..Polled::default()
    };
    let tagged = Cursor::default().advanced(
        &polled(vec![
            signalled("v2", Signal::Tag),
            signalled("v1", Signal::Release),
        ]),
        500,
    );
    assert_eq!(tagged.tagged, [Identity::new("v2")]);
    let release = signalled("v2", Signal::Release);
    assert!(tagged.unseen(&release));
    assert!(!tagged.unseen(&signalled("v2", Signal::Tag)));
    assert!(!tagged.unseen(&signalled("v1", Signal::Release)));
    assert!(tagged.unseen(&signalled("v3", Signal::Tag)));
    assert_eq!(
        identities(&fresh(
            Some(&tagged),
            vec![release.clone(), signalled("v1", Signal::Release)],
            0
        )),
        ["v2"]
    );
    let released = tagged.advanced(&polled(vec![release.clone()]), 500);
    assert_eq!(released.tagged, Vec::<Identity>::new());
    assert!(!released.unseen(&release));
    let quiet = tagged.advanced(&polled(Vec::new()), 500);
    assert_eq!(quiet.tagged, [Identity::new("v2")]);
    let forgotten = tagged.advanced(&polled(vec![signalled("v9", Signal::Release)]), 1);
    assert_eq!(forgotten.tagged, Vec::<Identity>::new());
    let twice = Cursor::default().advanced(
        &Polled {
            events: vec![signalled("v4", Signal::Tag)],
            passed: vec![Identity::new("v4")],
            ..Polled::default()
        },
        500,
    );
    assert_eq!(twice.tagged, [Identity::new("v4")]);
}

#[test]
fn a_cursor_remembers_a_bounded_number_of_identities() {
    let found: Vec<Event> = (0..30).map(|index| event(&index.to_string())).collect();
    let advanced = Cursor::default().advanced(
        &Polled {
            events: found,
            ..Polled::default()
        },
        20,
    );
    assert_eq!(advanced.seen.len(), 20);
    assert_eq!(advanced.seen.first().map(Identity::as_str), Some("0"));
    assert_eq!(advanced.seen.last().map(Identity::as_str), Some("19"));
}

#[test]
fn a_cursor_round_trips_through_json() {
    let cursor = Cursor {
        seen: vec![Identity::new("a")],
        etag: Some("e".to_owned()),
        ..Cursor::default()
    };
    let json = serde_json::to_string(&cursor).unwrap();
    assert_eq!(
        json,
        r#"{"seen":["a"],"etag":"e","last_modified":null,"project":null}"#
    );
    assert_eq!(serde_json::from_str::<Cursor>(&json).unwrap(), cursor);
    let tagged = Cursor {
        signals: BTreeMap::from([(
            Signal::Tag,
            Validators {
                etag: Some("t".to_owned()),
                last_modified: None,
            },
        )]),
        ..cursor
    };
    let json = serde_json::to_string(&tagged).unwrap();
    assert!(
        json.ends_with(r#""signals":{"tag":{"etag":"t","last_modified":null}}}"#),
        "{json}"
    );
    assert_eq!(serde_json::from_str::<Cursor>(&json).unwrap(), tagged);
    let proving = Cursor {
        proved: vec![
            proof("live", 1, false),
            proof("gone", 2, true),
            unheld("never", 3, "p"),
            Proof {
                passed: true,
                ..proof("over", 4, false)
            },
        ],
        ..Cursor::default()
    };
    let json = serde_json::to_string(&proving).unwrap();
    assert!(
        json.ends_with(r#""proved":[{"identity":"live","created":"1970-01-01T00:00:01Z","maker":"p"},{"identity":"gone","created":"1970-01-01T00:00:02Z","maker":"p","tombstone":"held"},{"identity":"never","created":"1970-01-01T00:00:03Z","maker":"p","tombstone":"unheld"},{"identity":"over","created":"1970-01-01T00:00:04Z","maker":"p","passed":true}]}"#),
        "{json}"
    );
    assert_eq!(serde_json::from_str::<Cursor>(&json).unwrap(), proving);
}

/// The Release `identity`, its notes `body`.
fn released(identity: &str, body: &str) -> Event {
    let mut attributes = crate::versioned("v1", false);
    attributes.insert(
        crate::SIGNAL.to_owned(),
        crate::Attribute::Text(Signal::Release.name().to_owned()),
    );
    Event::normalise(Draft {
        identity: identity.to_owned(),
        attributes,
        body: body.to_owned(),
        ..draft()
    })
    .unwrap()
}

#[test]
fn a_release_watched_whose_notes_change_is_fresh_again_marked_as_an_edit() {
    let first = Polled {
        events: vec![released("a", "notes"), released("b", "notes"), event("c")],
        watched: 1,
        ..Polled::default()
    };
    let cursor = Cursor::default().advanced(&first, 500);
    assert_eq!(
        cursor
            .notes
            .keys()
            .map(Identity::as_str)
            .collect::<Vec<_>>(),
        ["a"]
    );
    let edited = released("a", "fixed notes");
    assert!(cursor.edited(&edited));
    assert!(!cursor.edited(&released("a", "notes")));
    assert!(!cursor.edited(&released("b", "fixed notes")));
    let found = fresh(
        Some(&cursor),
        vec![edited.clone(), released("b", "fixed notes"), event("d")],
        0,
    );
    assert_eq!(identities(&found), ["a", "d"]);
    assert!(found[0].edited() && !found[1].edited());
    let kept = cursor.advanced(&Polled::default(), 500);
    assert_eq!(kept.notes, cursor.notes);
    let unwatched = cursor.advanced(
        &Polled {
            events: vec![edited],
            ..Polled::default()
        },
        500,
    );
    assert!(unwatched.notes.is_empty());
}

#[test]
fn a_cursor_counts_the_tombstones_it_holds_of_each_instance() {
    let cursor = Cursor {
        proved: vec![
            Proof {
                maker: "q".to_owned(),
                ..proof("q1", 1, true)
            },
            proof("p1", 1, true),
            proof("p2", 2, false),
            unheld("u", 3, "p"),
            Proof {
                passed: true,
                ..proof("over", 4, false)
            },
        ],
        ..Cursor::default()
    };
    assert_eq!(cursor.held(), BTreeMap::from([("p", 1), ("q", 1)]));
    let ahead = Polled {
        ahead: vec!["p".to_owned()],
        ..Polled::default()
    };
    assert_eq!(cursor.advanced(&ahead, 10).ahead, ["p"]);
    assert_eq!(
        cursor
            .advanced(&ahead, 10)
            .advanced(&Polled::default(), 10)
            .ahead,
        Vec::<String>::new()
    );
}

#[test]
fn an_instance_forgets_only_its_own_proofs_however_many_it_has_taken() {
    let of = |maker: &str, identity: &str, second: i64| Proof {
        maker: maker.to_owned(),
        ..proof(identity, second, false)
    };
    let cursor = Cursor {
        proved: vec![of("b", "b1", 10), of("a", "a0", 1)],
        ..Cursor::default()
    };
    let polled = Polled {
        proved: vec![of("a", "a1", 2), of("a", "a2", 3), of("a", "a3", 4)],
        each: Some(2),
        ..Polled::default()
    };
    let advanced = cursor.advanced(&polled, 1);
    assert_eq!(
        advanced.proved,
        [of("a", "a1", 2), of("a", "a2", 3), of("b", "b1", 10)]
    );
    assert!(!advanced.newer(&Identity::new("b1"), at(5)));
    assert_eq!(
        cursor.advanced(&Polled::default(), 1).proved,
        [of("b", "b1", 10)]
    );
}

#[test]
fn a_cursor_keeps_the_project_of_each_event_a_feed_proved_while_it_sees_it() {
    let cursor = Cursor {
        seen: vec![Identity::new("old"), Identity::new("gone")],
        projects: BTreeMap::from([
            (Identity::new("old"), "kept".to_owned()),
            (Identity::new("gone"), "left".to_owned()),
        ]),
        ..Cursor::default()
    };
    let polled = Polled {
        events: vec![event("fed"), event("forge")],
        withdrawn: vec![Identity::new("gone")],
        proved: vec![proof("fed", 1, false), proof("gone", 2, true)],
        ..Polled::default()
    };
    let advanced = cursor.advanced(&polled, 10);
    assert_eq!(
        advanced.projects,
        BTreeMap::from([
            (Identity::new("fed"), event("fed").project().to_owned()),
            (Identity::new("old"), "kept".to_owned()),
        ])
    );
    assert_eq!(advanced.project_of(&Identity::new("old")), Some("kept"));
    assert_eq!(advanced.project_of(&Identity::new("forge")), None);
    assert_eq!(
        cursor.advanced(&Polled::default(), 0).projects,
        BTreeMap::new()
    );
}

#[test]
fn what_a_peer_pushed_leaves_the_place_a_poll_keeps_as_it_was() {
    let validators = Validators {
        etag: Some("e".to_owned()),
        last_modified: Some("m".to_owned()),
    };
    let polled = Cursor {
        seen: vec![Identity::new("a")],
        etag: Some("tag".to_owned()),
        last_modified: Some("then".to_owned()),
        project: Some("gate".to_owned()),
        signals: BTreeMap::from([(Signal::Tag, validators.clone())]),
        packages: BTreeMap::from([("p".to_owned(), validators.clone())]),
        listings: BTreeMap::from([("l".to_owned(), validators)]),
        newest: Some(Timestamp::UNIX_EPOCH),
        sequence: Some(7),
        ahead: vec!["far".to_owned()],
        ..Cursor::default()
    };
    let pushed = Polled {
        events: vec![event("b")],
        proved: vec![Proof {
            identity: Identity::new("b"),
            created: Timestamp::UNIX_EPOCH,
            maker: "upstream".to_owned(),
            tombstone: None,
            passed: false,
        }],
        relisted: true,
        sequence: Some(9),
        newest: Some(Timestamp::MAX),
        ..Polled::default()
    };
    let received = polled.received(&pushed, 10);
    assert_eq!(
        received,
        Cursor {
            seen: vec![Identity::new("b"), Identity::new("a")],
            proved: pushed.proved.clone(),
            projects: BTreeMap::from([(Identity::new("b"), event("b").project().to_owned())]),
            notes: received.notes.clone(),
            tagged: received.tagged.clone(),
            ..polled.clone()
        }
    );
    let unprojected = Cursor::default().received(&pushed, 10);
    assert_eq!(unprojected.project.as_deref(), Some(event("b").project()));
}
