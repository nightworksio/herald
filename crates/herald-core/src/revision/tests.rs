use jiff::Timestamp;

use super::*;
use crate::key::{Identity, Key, RouteId};
use crate::ledger::{Facts, Plan, plan};
use crate::route::Delivery;

fn key() -> Key {
    Key::derive(&RouteId::new("route"), &Identity::new("github.com/1/v1"))
}

fn sent(message: &str) -> Facts {
    Facts {
        message: Some(message.to_owned()),
        ..Facts::default()
    }
}

fn tag() -> Revision {
    Revision::of(Some(Signal::Tag), ["v1", "more"])
}

fn release() -> Revision {
    Revision::of(Some(Signal::Release), ["gate v1", "notes"])
}

fn posted(revision: Revision) -> Entry {
    Entry::new(key())
        .landed(Step::Post, sent("post"))
        .landed(Step::Chunk(1), sent("one"))
        .landed(Step::Chunk(2), sent("two"))
        .revising(revision)
}

#[test]
fn a_revision_is_told_apart_by_its_signal_and_every_part_it_renders() {
    assert_eq!(tag(), tag());
    assert_ne!(
        Revision::of(None, ["ab", "c"]).digest,
        Revision::of(None, ["a", "bc"]).digest
    );
    assert_eq!(tag().digest.len(), 64);
    assert_eq!(release().signal, Some(Signal::Release));
}

fn late(late_release: LateRelease) -> Delivery {
    Delivery {
        late_release,
        ..Delivery::default()
    }
}

#[test]
fn a_tag_s_post_is_edited_to_its_release_or_posted_anew_as_the_route_says() {
    let entry = posted(tag());
    let edit = late(LateRelease::Edit);
    assert_eq!(revise(&entry, &release(), &edit, true), Revise::Edit);
    assert_eq!(revise(&entry, &release(), &edit, false), Revise::Post);
    assert_eq!(
        revise(&entry, &release(), &late(LateRelease::Post), true),
        Revise::Post
    );
    assert_eq!(
        revise(&entry, &release(), &late(LateRelease::Skip), true),
        Revise::Keep
    );
    assert_eq!(revise(&entry, &tag(), &edit, true), Revise::Keep);
    let other_tag = Revision::of(Some(Signal::Tag), ["v1"]);
    assert_eq!(revise(&entry, &other_tag, &edit, true), Revise::Keep);
    assert_eq!(LateRelease::default(), LateRelease::Edit);
}

#[test]
fn a_release_s_post_is_edited_to_its_edited_notes_only_where_the_route_takes_edits() {
    let entry = posted(release());
    let edited = Revision::of(Some(Signal::Release), ["gate v1", "fixed notes"]);
    let taking = Delivery {
        edits: Edits::Release,
        ..Delivery::default()
    };
    assert_eq!(revise(&entry, &edited, &taking, true), Revise::Edit);
    assert_eq!(revise(&entry, &edited, &taking, false), Revise::Keep);
    assert_eq!(
        revise(&entry, &edited, &Delivery::default(), true),
        Revise::Keep
    );
    assert_eq!(revise(&entry, &release(), &taking, true), Revise::Keep);
    assert_eq!(Edits::default(), Edits::None);
}

#[test]
fn a_post_never_landed_or_never_revised_is_kept_as_it_is() {
    let edit = late(LateRelease::Edit);
    let unposted = Entry::new(key()).revising(tag());
    assert_eq!(revise(&unposted, &release(), &edit, true), Revise::Keep);
    let unrevised = Entry::new(key()).landed(Step::Post, sent("post"));
    assert_eq!(revise(&unrevised, &release(), &edit, true), Revise::Keep);
}

#[test]
fn a_revision_edits_each_part_that_landed_posts_each_more_and_deletes_each_fewer() {
    let entry = posted(tag());
    let digest = release().digest;
    let edit = |part| Step::Edit {
        revision: digest.clone(),
        part,
    };
    assert_eq!(
        revised(&entry, &release(), 4),
        [edit(0), edit(1), edit(2), Step::Chunk(3)]
    );
    assert_eq!(
        revised(&entry, &release(), 2),
        [edit(0), edit(1), Step::Delete(2)]
    );
    let threaded = entry
        .landed(Step::OpenThread, Facts::default())
        .landed(Step::Deliveries, Facts::default());
    assert_eq!(
        revised(&threaded, &release(), 1),
        [
            edit(0),
            Step::Delete(1),
            Step::Delete(2),
            Step::Rename(digest.clone()),
            Step::Updates(digest.clone())
        ]
    );
}

#[test]
fn a_deleted_chunk_and_each_edit_superseded_leave_the_ledger() {
    let digest = release().digest;
    let wanted = revised(&posted(tag()), &release(), 2);
    let now = Timestamp::UNIX_EPOCH;
    let edited = posted(tag())
        .landed(
            Step::Edit {
                revision: "old".to_owned(),
                part: 1,
            },
            Facts::default(),
        )
        .landed(
            Step::Edit {
                revision: digest.clone(),
                part: 1,
            },
            Facts::default(),
        )
        .attempted(Step::Delete(2), now);
    assert_eq!(edited.chunks(), [1, 2]);
    assert!(matches!(
        plan(Some(&edited), &wanted, "r1", now),
        Plan::Send(planned) if planned.iter().map(|planned| (&planned.step, planned.verify_first)).eq([
            (&Step::Edit { revision: digest.clone(), part: 0 }, false),
            (&Step::Delete(2), false),
        ])
    ));
    let deleted = edited
        .landed(
            Step::Edit {
                revision: digest.clone(),
                part: 0,
            },
            Facts::default(),
        )
        .landed(Step::Delete(2), Facts::default());
    assert_eq!(deleted.chunks(), [1]);
    assert_eq!(
        deleted
            .records
            .iter()
            .map(|record| &record.step)
            .collect::<Vec<_>>(),
        [
            &Step::Post,
            &Step::Chunk(1),
            &Step::Edit {
                revision: digest.clone(),
                part: 1
            },
            &Step::Edit {
                revision: digest.clone(),
                part: 0
            },
        ]
    );
    assert_eq!(plan(Some(&deleted), &wanted, "r1", now), Plan::Complete);
    let renamed = deleted
        .landed(Step::Rename("old".to_owned()), Facts::default())
        .landed(Step::Rename(digest.clone()), Facts::default());
    assert_eq!(
        renamed.records.last().map(|record| &record.step),
        Some(&Step::Rename(digest))
    );
    assert_eq!(renamed.records.len(), 5);
}

#[test]
fn a_deletion_of_a_message_already_gone_leaves_what_it_would() {
    assert!(already_deleted(&Step::Delete(1), 404));
    assert!(already_deleted(&Step::Delete(1), 410));
    assert!(!already_deleted(&Step::Delete(1), 403));
    assert!(!already_deleted(&Step::Chunk(1), 404));
}

#[test]
fn a_chunk_is_told_apart_by_the_revision_its_entry_brings_the_post_to() {
    let unrevised = Entry::new(key());
    assert_eq!(unrevised.chunk_token(2), format!("{}.2", key()));
    let revised = Entry::new(key()).revising(release());
    assert_eq!(
        revised.chunk_token(2),
        format!("{}.2.{}", key(), release().digest)
    );
}

#[test]
fn a_part_is_the_post_s_message_or_a_chunk_s() {
    let entry = posted(tag());
    assert_eq!(entry.part(0), Some("post"));
    assert_eq!(entry.part(2), Some("two"));
    assert_eq!(entry.part(3), None);
}

#[test]
fn a_group_s_post_is_edited_where_it_landed_renders_otherwise_and_the_destination_edits() {
    let shown = posted(release());
    assert_eq!(regrouped(&shown, &tag(), true), Revise::Edit);
    assert_eq!(regrouped(&shown, &tag(), false), Revise::Keep);
    assert_eq!(regrouped(&shown, &release(), true), Revise::Keep);
    let unposted = Entry::new(key()).revising(release());
    assert_eq!(regrouped(&unposted, &tag(), true), Revise::Keep);
    assert_eq!(regrouped(&Entry::new(key()), &tag(), true), Revise::Keep);
}
