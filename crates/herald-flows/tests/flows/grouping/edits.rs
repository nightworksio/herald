use herald_core::{Signal, Step};
use herald_flows::{Outcome, Stop};
use herald_ports::Waiting;

use super::*;

/// A group of `cli` and `core` held and announced at half past noon on a
/// config grouping as `group` says, its route given `extra` too, the post's
/// answer scripted first in `answers`.
fn announced(group: &str, extra: &str, answers: &[(&str, u16, &str)]) -> (Config, Failing, Script) {
    let (config, store) = (grouped(group, extra), healthy());
    let script = Script::answering(answers);
    sent(
        &config,
        (&store, NOON),
        &script,
        &member("cli", 2, Signal::Tag, "tagged"),
    );
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("core", 1)),
        gathered(30)
    );
    assert_eq!(
        round_outcomes(&swept(&config, (&store, at(30)), &script)),
        [posted()]
    );
    assert!(body(&script, 0).contains("core with notes and cli with tagged"));
    (config, store, script)
}

#[test]
fn a_group_that_edits_posts_its_first_member_at_once_and_edits_in_each_other_within_its_window() {
    let (config, store) = (grouped(", mode = \"edit\"", ""), healthy());
    let script = Script::answering(&[POSTED, EDITED, SECOND]);
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("cli", 2)),
        posted()
    );
    assert!(body(&script, 0).contains("v1.2.0 of 1"));
    assert_eq!(
        unrevised(sent(
            &config,
            (&store, at(29)),
            &script,
            &release("core", 1)
        )),
        edited()
    );
    assert!(body(&script, 1).contains("core with notes and cli with notes and"));
    assert_eq!(
        swept(&config, (&store, at(31)), &script),
        Vec::<Swept>::new()
    );
    assert_eq!(
        sent(&config, (&store, at(31)), &script, &release("web", 3)),
        posted()
    );
    assert!(body(&script, 2).contains("web alone"));
}

#[test]
fn a_member_after_its_group_was_announced_is_posted_alone_skipped_or_edited_in_as_its_route_says() {
    let (config, store, script) = announced("", "", &[POSTED, SECOND]);
    assert_eq!(
        sent(&config, (&store, at(40)), &script, &release("web", 3)),
        posted()
    );
    assert!(body(&script, 1).contains("web alone"));
    let (config, store, script) = announced(", late = \"skip\"", "", &[POSTED]);
    assert_eq!(
        sent(&config, (&store, at(40)), &script, &release("web", 3)),
        Outcome::Skipped("its group was announced before it came".to_owned())
    );
    assert_eq!(script.seen().len(), 1);
    let (config, store, script) = announced(", late = \"edit\"", "", &[POSTED, EDITED]);
    assert_eq!(
        unrevised(sent(&config, (&store, at(40)), &script, &release("web", 3))),
        edited()
    );
    assert!(
        body(&script, 1).contains("core with notes and cli with tagged and web with notes and")
    );
    let kept = pollster::block_on(store.inner.pending(&hold()))
        .unwrap()
        .unwrap();
    assert_eq!(kept.value.events.len(), 3);
}

#[test]
fn a_late_member_whose_group_s_record_is_gone_is_posted_alone_or_skipped() {
    for (late, outcome) in [
        ("", posted()),
        (", late = \"edit\"", posted()),
        (
            ", late = \"skip\"",
            Outcome::Skipped("its group was announced before it came".to_owned()),
        ),
    ] {
        let (config, store, script) = announced(late, "", &[POSTED, SECOND]);
        pollster::block_on(store.inner.drop_pending(&hold())).unwrap();
        assert_eq!(
            sent(&config, (&store, at(40)), &script, &release("web", 3)),
            outcome,
            "{late}"
        );
    }
}

#[test]
fn a_release_of_a_tag_its_group_shows_edits_the_post_posts_anew_or_leaves_it_as_late_release_says()
{
    let (config, store, script) = announced("", "", &[POSTED, EDITED]);
    let released = member("cli", 2, Signal::Release, "the release");
    assert_eq!(
        unrevised(sent(&config, (&store, at(40)), &script, &released)),
        edited()
    );
    assert!(body(&script, 1).contains("cli with the release and"));
    assert_eq!(
        sent(&config, (&store, at(41)), &script, &released),
        Outcome::AlreadyAnnounced
    );
    let (config, store, script) = announced("", "late_release = \"post\"", &[POSTED, SECOND]);
    assert_eq!(
        sent(&config, (&store, at(40)), &script, &released),
        posted()
    );
    assert!(body(&script, 1).contains("cli alone"));
    let (config, store, script) = announced("", "late_release = \"skip\"", &[POSTED]);
    assert_eq!(
        sent(&config, (&store, at(40)), &script, &released),
        Outcome::AlreadyAnnounced
    );
    assert_eq!(script.seen().len(), 1);
}

#[test]
fn a_release_of_a_tag_whose_group_s_record_is_gone_is_posted_anew() {
    let (config, store, script) = announced("", "", &[POSTED, SECOND]);
    pollster::block_on(store.inner.drop_pending(&hold())).unwrap();
    let released = member("cli", 2, Signal::Release, "the release");
    assert_eq!(
        sent(&config, (&store, at(40)), &script, &released),
        posted()
    );
    assert!(body(&script, 1).contains("cli alone"));
}

#[test]
fn an_edited_release_its_group_shows_edits_the_post_only_where_the_route_edits_releases() {
    let fixed = member("core", 1, Signal::Release, "fixed").marked_edited();
    let (config, store, script) = announced("", "edits = \"release\"", &[POSTED, EDITED]);
    assert_eq!(
        unrevised(sent(&config, (&store, at(40)), &script, &fixed)),
        edited()
    );
    assert!(body(&script, 1).contains("core with fixed and"));
    let (config, store, script) = announced("", "", &[POSTED]);
    assert_eq!(
        sent(&config, (&store, at(40)), &script, &fixed),
        Outcome::AlreadyAnnounced
    );
    assert_eq!(script.seen().len(), 1);
}

#[test]
fn a_group_whose_record_cannot_be_closed_or_landed_or_members_recorded_stops() {
    let (config, store) = (
        grouped("", ""),
        Failing {
            fault: Fault::ClosingConflicts,
            ..healthy()
        },
    );
    let script = Script::default();
    sent(&config, (&store, NOON), &script, &release("cli", 2));
    let raced = Outcome::Stopped {
        landed: Vec::new(),
        remaining: Vec::new(),
        reason: Stop::Store("another run wrote the group v1.2.0 first".to_owned()),
    };
    assert_eq!(
        round_outcomes(&swept(&config, (&store, at(30)), &script)),
        [raced]
    );
    assert_eq!(script.seen(), Vec::<String>::new());
    let (config, store) = (
        grouped(", mode = \"edit\"", ""),
        Failing {
            fault: Fault::ClosingConflicts,
            ..healthy()
        },
    );
    let script = Script::answering(&[POSTED]);
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("cli", 2)),
        Outcome::Stopped {
            landed: vec![Step::Post],
            remaining: Vec::new(),
            reason: Stop::Store("another run wrote the group v1.2.0 first".to_owned()),
        }
    );
    let (config, store) = (
        grouped(", mode = \"edit\"", ""),
        Failing {
            fault: Fault::GroupedConflicts,
            ..healthy()
        },
    );
    let script = Script::answering(&[POSTED]);
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("cli", 2)),
        Outcome::Stopped {
            landed: vec![Step::Post],
            remaining: Vec::new(),
            reason: Stop::Store(
                "another run wrote the entry of github.com/2/v1.2.0 first".to_owned()
            ),
        }
    );
}
