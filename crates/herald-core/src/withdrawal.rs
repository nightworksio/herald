//! What withdrawing an announcement removes, and the identity a release
//! re-created after a withdrawal is announced under (ADR-0048).

use crate::key::Identity;
use crate::ledger::{Entry, Step};

/// What the identity of a re-created release is suffixed with, before the
/// count of withdrawals before it.
const REISSUED: &str = "#";

/// The steps that withdraw the post `entry` keeps: each chunk that landed
/// deleted, the last first, then the post, then the Note's deletion
/// delivered to every follower its deliveries reached.
#[must_use]
pub fn withdrawal(entry: &Entry) -> Vec<Step> {
    let mut steps: Vec<Step> = entry.chunks().into_iter().rev().map(Step::Delete).collect();
    if entry.message(&Step::Post).is_some() {
        steps.push(Step::Delete(0));
    }
    if entry.facts(&Step::Deliveries).is_some() {
        steps.push(Step::Deletes);
    }
    steps
}

/// The identity a release of `identity` is announced under once it was
/// withdrawn `withdrawals` times: its own where never, else its own
/// suffixed with the count.
#[must_use]
pub fn reissued(identity: &Identity, withdrawals: u32) -> Identity {
    match withdrawals {
        0 => identity.clone(),
        count => Identity::new(format!("{}{REISSUED}{count}", identity.as_str())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{Key, RouteId};
    use crate::ledger::Facts;

    fn sent(message: &str) -> Facts {
        Facts {
            message: Some(message.to_owned()),
            ..Facts::default()
        }
    }

    fn key() -> Key {
        Key::derive(&RouteId::new("route"), &Identity::new("github.com/1/v1"))
    }

    #[test]
    fn a_withdrawal_deletes_each_chunk_last_first_then_the_post_then_tells_the_followers() {
        let posted = Entry::new(key())
            .landed(Step::Post, sent("post"))
            .landed(Step::Chunk(1), sent("one"))
            .landed(Step::Chunk(2), sent("two"));
        assert_eq!(
            withdrawal(&posted),
            [Step::Delete(2), Step::Delete(1), Step::Delete(0)]
        );
        let delivered = Entry::new(key())
            .landed(Step::Post, sent("note"))
            .landed(Step::Deliveries, Facts::default());
        assert_eq!(withdrawal(&delivered), [Step::Delete(0), Step::Deletes]);
        assert_eq!(withdrawal(&Entry::new(key())), Vec::<Step>::new());
    }

    #[test]
    fn a_post_s_deletion_leaves_no_post_and_is_planned_while_the_post_stands() {
        let posted = Entry::new(key()).landed(Step::Post, sent("post"));
        let deleted = posted.clone().landed(Step::Delete(0), Facts::default());
        assert_eq!(deleted.message(&Step::Post), None);
        assert_eq!(withdrawal(&deleted), Vec::<Step>::new());
        let now: jiff::Timestamp = "2026-11-17T12:00:00Z".parse().unwrap();
        assert!(matches!(
            crate::ledger::plan(Some(&posted), &[Step::Delete(0)], "r1", now),
            crate::ledger::Plan::Send(_)
        ));
        assert_eq!(
            crate::ledger::plan(Some(&deleted), &[Step::Delete(0)], "r1", now),
            crate::ledger::Plan::Complete
        );
    }

    #[test]
    fn a_deleted_release_is_marked_withdrawn_and_a_re_created_one_takes_its_reissued_identity() {
        let event = crate::Event::normalise(crate::event::Draft {
            identity: "github.com/1/v1".to_owned(),
            kind: crate::Kind::RELEASE,
            project: "herald".to_owned(),
            attributes: crate::versioned("1.0.0", false),
            title: "herald 1.0.0".to_owned(),
            body: String::new(),
            links: std::collections::BTreeMap::from([(
                crate::RELEASE_LINK.to_owned(),
                "https://example.org/v1".to_owned(),
            )]),
            occurred: "2026-11-17T12:00:00Z".parse().unwrap(),
        })
        .unwrap();
        assert!(!event.withdrawn());
        let withdrawn = event.clone().marked_withdrawn();
        assert!(withdrawn.withdrawn());
        assert_eq!(withdrawn.identity(), event.identity());
        let again = event.clone().reissued(reissued(event.identity(), 2));
        assert_eq!(again.identity().as_str(), "github.com/1/v1#2");
        assert_eq!(again.title(), event.title());
        assert!(!again.withdrawn());
    }

    #[test]
    fn an_entry_keeps_when_it_was_withdrawn_only_once_it_was() {
        let fresh = serde_json::to_value(Entry::new(key())).unwrap();
        assert!(fresh.get("withdrawn").is_none());
        let mut withdrawn = Entry::new(key());
        withdrawn.withdrawn = Some("2026-11-17T12:00:00Z".parse().unwrap());
        let kept = serde_json::to_value(&withdrawn).unwrap();
        assert_eq!(kept["withdrawn"], "2026-11-17T12:00:00Z");
        assert_eq!(serde_json::from_value::<Entry>(kept).unwrap(), withdrawn);
    }

    #[test]
    fn a_re_created_release_is_announced_under_its_identity_and_its_count_of_withdrawals() {
        let identity = Identity::new("github.com/1/v1");
        assert_eq!(reissued(&identity, 0), identity);
        assert_eq!(reissued(&identity, 1).as_str(), "github.com/1/v1#1");
        assert_eq!(reissued(&identity, 12).as_str(), "github.com/1/v1#12");
    }
}
