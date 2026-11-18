//! The keys that choose a source's pull requests.

use super::*;
use crate::{PULL_ACTIONS, PullAction, Pulls};

fn pulled(pulls: Pulls) -> SourceOptions {
    SourceOptions {
        repository: Some("a/b".to_owned()),
        signals: Some(vec![Signal::PullRequest]),
        pulls: Some(pulls),
        ..SourceOptions::default()
    }
}

#[test]
fn pulls_is_given_only_where_pull_requests_are_announced_and_each_key_is_sound() {
    for forge in Forge::ALL {
        assert_eq!(pulled(Pulls::default()).check(forge), Ok(()));
    }
    let refused = [
        (
            SourceOptions {
                signals: None,
                ..pulled(Pulls::default())
            },
            "pulls chooses pull requests, which no repository the source takes announces; add \"pull-request\" to signals, or remove pulls",
        ),
        (
            pulled(Pulls {
                actions: Some(Vec::new()),
                ..Pulls::default()
            }),
            "pulls.actions names nothing, so no pull request would be announced; name one, or remove pulls.actions",
        ),
    ];
    for (options, said) in refused {
        assert_eq!(options.check(Forge::GitHub), Err(said.to_owned()));
    }
    for key in ["labels", "authors"] {
        let bad = Pulls {
            labels: if key == "labels" {
                vec!["[".to_owned()]
            } else {
                Vec::new()
            },
            authors: if key == "authors" {
                vec!["[".to_owned()]
            } else {
                Vec::new()
            },
            ..Pulls::default()
        };
        let refused = pulled(bad).check(Forge::GitHub).unwrap_err();
        assert!(refused.starts_with(&format!("pulls.{key}: ")), "{refused}");
    }
}

#[test]
fn a_source_announces_pull_requests_opened_readied_and_merged_where_it_chooses_none() {
    let none = SourceOptions::default().pull_choice();
    assert_eq!(none.actions, PULL_ACTIONS);
    assert_eq!(
        PULL_ACTIONS,
        [
            PullAction::Opened,
            PullAction::ReadyForReview,
            PullAction::Merged
        ]
    );
    assert!(!none.drafts);
    let names: Vec<&str> = [
        PullAction::Opened,
        PullAction::ReadyForReview,
        PullAction::Reopened,
        PullAction::Merged,
        PullAction::Closed,
    ]
    .into_iter()
    .map(PullAction::name)
    .collect();
    assert_eq!(
        names,
        ["opened", "ready_for_review", "reopened", "merged", "closed"]
    );
    let drafts = pulled(Pulls {
        drafts: Some(true),
        ..Pulls::default()
    })
    .pull_choice();
    assert!(drafts.takes((PullAction::Opened, true), (Some("a"), &[])));
    assert!(!none.takes((PullAction::Opened, true), (Some("a"), &[])));
    assert!(none.takes((PullAction::Merged, true), (Some("a"), &[])));
    assert!(none.takes((PullAction::Opened, false), (None, &[])));
}
