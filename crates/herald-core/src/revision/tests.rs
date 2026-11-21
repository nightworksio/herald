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

#[test]
fn a_replacement_deletes_every_part_then_resumes_only_what_it_left() {
    let at = Timestamp::from_second(1_000).unwrap();
    let entry = posted(tag());
    let replacing = replaced(&entry, &release(), at);
    assert_eq!(replacing.deleting, [0, 2, 1]);
    assert_eq!(
        replacing.deletions(),
        [Step::Delete(0), Step::Delete(2), Step::Delete(1)]
    );
    let entry = Entry {
        replacing: Some(replacing.clone()),
        ..entry
    };
    assert_eq!(entry.reposting().records, []);
    let deleted = entry
        .clone()
        .landed(Step::Delete(2), Facts::default())
        .landed(Step::Delete(1), Facts::default())
        .landed(Step::Delete(0), Facts::default())
        .attempted(Step::Post, at);
    let resumed = replaced(
        &deleted,
        &release(),
        at + jiff::SignedDuration::from_hours(1),
    );
    assert_eq!(resumed.deleting, Vec::<u32>::new());
    assert_eq!(resumed.at, at);
    let Plan::Send(planned) = plan(Some(&deleted.reposting()), &[Step::Post], "run", at) else {
        panic!("the post is planned again");
    };
    assert!(planned[0].verify_first);
    let moved = replaced(&deleted, &tag(), at + jiff::SignedDuration::from_hours(1));
    assert_ne!(moved.at, at);
    assert_eq!(deleted.clone().settled().revision, Some(release()));
    assert_eq!(deleted.clone().settled().replacing, None);
}

#[test]
fn a_replacement_s_post_and_chunks_are_told_apart_from_any_earlier_one_s() {
    let at = Timestamp::from_second(1_000).unwrap();
    let entry = posted(tag());
    assert_eq!(entry.post_token(), key().to_string());
    let first = Entry {
        replacing: Some(replaced(&entry, &release(), at)),
        ..entry.clone()
    };
    let again = Entry {
        replacing: Some(replaced(
            &entry,
            &release(),
            at + jiff::SignedDuration::from_hours(1),
        )),
        ..entry.clone()
    };
    assert_ne!(first.post_token(), entry.post_token());
    assert_ne!(first.post_token(), again.post_token());
    assert!(first.post_token().contains(&release().digest));
    assert_ne!(first.chunk_token(1), entry.chunk_token(1));
    assert_ne!(first.chunk_token(1), again.chunk_token(1));
    assert_eq!(first.post_token(), first.clone().post_token());
}

#[test]
fn a_post_and_its_chunks_are_told_apart_by_when_they_were_sent() {
    let at = Timestamp::from_second(1_000).unwrap();
    let later = at + jiff::SignedDuration::from_hours(1);
    let first = Entry::new(key())
        .revising(release())
        .attempted(Step::Post, at)
        .attempted(Step::Chunk(1), at);
    let forgotten = Entry::new(key())
        .revising(release())
        .attempted(Step::Post, later)
        .attempted(Step::Chunk(1), later);
    assert_eq!(first.post_token(), format!("{}.1000", key()));
    assert_ne!(first.post_token(), forgotten.post_token());
    assert_ne!(first.chunk_token(1), forgotten.chunk_token(1));
    assert_eq!(first.sent_at(&Step::Chunk(1)), Some(at));
    assert_eq!(posted(tag()).sent_at(&Step::Post), None);
}

#[test]
fn a_new_replacement_deletes_what_an_earlier_one_sent_unrecorded_and_takes_none_for_its_own() {
    let at = Timestamp::from_second(1_000).unwrap();
    let earlier = Entry {
        replacing: Some(replaced(&posted(tag()), &release(), at)),
        ..Entry::new(key())
    }
    .attempted(Step::Post, at);
    assert!(resumes(&earlier, &release()));
    assert!(!resumes(&earlier, &tag()));
    assert_eq!(earlier.unrecorded(), [Step::Post]);
    let read = Entry::new(key()).landed(Step::Post, sent("unrecorded"));
    let found = earlier.clone().found(Some(&read));
    assert_eq!(found.part(0), Some("unrecorded"));
    assert_eq!(replaced(&found, &tag(), at).deleting, [0]);
    let token = earlier.post_token();
    let lost = earlier.found(None);
    assert_eq!(lost.strays, [Stray { part: 0, token }]);
    assert_eq!(lost.unrecorded(), []);
    assert_eq!(replaced(&lost, &tag(), at).deleting, Vec::<u32>::new());
    let chunked = posted(tag()).attempted(Step::Chunk(3), at);
    let read = Entry::new(key()).landed(Step::Chunk(3), sent("three"));
    assert_eq!(
        replaced(&chunked.clone().found(Some(&read)), &release(), at).deleting,
        [0, 3, 2, 1]
    );
    let strayed = chunked.found(None).strayed(&Step::Pin);
    let parts: Vec<u32> = strayed.strays.iter().map(|stray| stray.part).collect();
    assert_eq!(parts, [3]);
}

#[test]
fn a_send_again_after_an_unanswered_one_goes_under_the_first_send_s_key() {
    let at = Timestamp::from_second(1_000).unwrap();
    let later = at + jiff::SignedDuration::from_hours(1);
    let once = Entry::new(key())
        .revising(release())
        .attempted(Step::Post, at)
        .attempted(Step::Chunk(1), at);
    let again = once
        .clone()
        .attempted(Step::Post, later)
        .attempted(Step::Chunk(1), later);
    assert_eq!(again.post_token(), once.post_token());
    assert_eq!(again.chunk_token(1), once.chunk_token(1));
    assert_eq!(again.sent_at(&Step::Post), Some(at));
    let landed = again
        .landed(Step::Post, sent("post"))
        .attempted(Step::Post, later);
    assert_eq!(landed.sent_at(&Step::Post), Some(later));
}

#[test]
fn a_stray_is_dropped_once_a_send_under_its_own_key_lands() {
    let at = Timestamp::from_second(1_000).unwrap();
    let under_way = Entry {
        replacing: Some(replaced(&posted(tag()), &release(), at)),
        ..Entry::new(key())
    };
    let strayed = under_way.attempted(Step::Post, at).strayed(&Step::Post);
    assert_eq!(strayed.strays.len(), 1);
    let resent = strayed.clone().attempted(Step::Post, at);
    assert_eq!(resent.landed(Step::Post, sent("post")).strays, []);
    let other = Entry {
        replacing: Some(replaced(&posted(tag()), &tag(), at)),
        ..strayed
    };
    assert_eq!(other.landed(Step::Post, sent("post")).strays.len(), 1);
}

#[test]
fn a_post_or_chunk_answered_without_landing_leaves_nothing_to_find_again() {
    let at = Timestamp::from_second(1_000).unwrap();
    let entry = Entry::new(key())
        .attempted(Step::Post, at)
        .attempted(Step::Pin, at)
        .refused(&Step::Post)
        .refused(&Step::Pin);
    assert_eq!(entry.unrecorded(), []);
    assert_eq!(entry.sent_at(&Step::Pin), Some(at));
    let landed = posted(tag()).refused(&Step::Chunk(1));
    assert_eq!(landed.part(1), Some("one"));
}

#[test]
fn a_replacement_reposts_only_the_parts_still_to_delete_and_waits_on_a_deletion_only_attempted() {
    let at = Timestamp::from_second(1_000).unwrap();
    let pinned = posted(tag()).landed(Step::Pin, Facts::default());
    let entry = Entry {
        replacing: Some(replaced(&pinned, &release(), at)),
        ..pinned
    };
    let deleting = |entry: &Entry| {
        entry
            .replacing
            .as_ref()
            .map(|replacing| replacing.deleting.clone())
    };
    let tried = entry.clone().attempted(Step::Delete(2), at);
    assert_eq!(deleting(&tried), Some(vec![0, 2, 1]));
    assert_eq!(tried.sent_at(&Step::Delete(2)), Some(at));
    let partly = entry.landed(Step::Delete(2), Facts::default());
    assert_eq!(deleting(&partly), Some(vec![0, 1]));
    let kept: Vec<Step> = partly
        .reposting()
        .records
        .iter()
        .map(|record| record.step.clone())
        .collect();
    assert_eq!(kept, [Step::Pin]);
}
