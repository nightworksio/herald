//! The keys that choose a source's issues.

use super::*;
use crate::{ISSUE_ACTIONS, IssueAction, Issues};

fn issued(issues: Issues) -> SourceOptions {
    SourceOptions {
        repository: Some("a/b".to_owned()),
        signals: Some(vec![Signal::Issue]),
        issues: Some(issues),
        ..SourceOptions::default()
    }
}

#[test]
fn issues_is_given_only_where_issues_are_announced_and_each_key_is_sound() {
    for forge in Forge::ALL {
        assert_eq!(issued(Issues::default()).check(forge), Ok(()));
    }
    let refused = [
        (
            SourceOptions {
                signals: None,
                ..issued(Issues::default())
            },
            "issues chooses issues, which no repository the source takes announces; add \"issue\" to signals, or remove issues",
        ),
        (
            issued(Issues {
                actions: Some(Vec::new()),
                ..Issues::default()
            }),
            "issues.actions names nothing, so no issue would be announced; name one, or remove issues.actions",
        ),
    ];
    for (options, said) in refused {
        assert_eq!(options.check(Forge::GitHub), Err(said.to_owned()));
    }
    for key in ["labels", "authors"] {
        let bad = Issues {
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
            ..Issues::default()
        };
        let refused = issued(bad).check(Forge::GitHub).unwrap_err();
        assert!(refused.starts_with(&format!("issues.{key}: ")), "{refused}");
    }
    assert_eq!(
        SourceOptions::default().issue_choice().actions,
        ISSUE_ACTIONS
    );
    assert_eq!(
        ISSUE_ACTIONS,
        [
            IssueAction::Opened,
            IssueAction::Closed,
            IssueAction::Reopened
        ]
    );
}

#[test]
fn confidential_issues_are_gitlab_s_alone_and_taken_only_where_asked() {
    let asked = issued(Issues {
        confidential: Some(true),
        ..Issues::default()
    });
    assert_eq!(asked.check(Forge::GitLab), Ok(()));
    for forge in [Forge::GitHub, Forge::Forgejo] {
        assert_eq!(
            asked.check(forge),
            Err(format!(
                "issues.confidential chooses GitLab's confidential issues, which {} has none of; remove it",
                forge.kind()
            ))
        );
    }
    let choice = asked.issue_choice();
    assert!(choice.takes((IssueAction::Opened, true), (None, &[])));
    let none = SourceOptions::default().issue_choice();
    assert!(!none.takes((IssueAction::Opened, true), (None, &[])));
    assert!(none.takes((IssueAction::Opened, false), (None, &[])));
}
