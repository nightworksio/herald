//! The keys that choose a source's runs.

use super::*;
use crate::{Conclusion, RUN_ACTIONS, RUN_FROM, RunAction, RunFrom, Runs};

fn ran(runs: Runs) -> SourceOptions {
    SourceOptions {
        repository: Some("a/b".to_owned()),
        signals: Some(vec![Signal::Run]),
        runs: Some(runs),
        ..SourceOptions::default()
    }
}

#[test]
fn runs_is_given_only_where_runs_are_announced_and_each_of_its_keys_is_sound() {
    for forge in Forge::ALL {
        assert_eq!(ran(Runs::default()).check(forge), Ok(()));
    }
    let unran = SourceOptions {
        signals: None,
        ..ran(Runs::default())
    };
    let refused = [
        (
            unran,
            Forge::GitHub,
            "runs chooses runs, which no repository the source takes announces; add \"run\" to signals, or remove runs",
        ),
        (
            ran(Runs {
                from: Some(Vec::new()),
                ..Runs::default()
            }),
            Forge::GitHub,
            "runs.from names nothing, so no run would be announced; name one, or remove runs.from",
        ),
        (
            ran(Runs {
                actions: Some(Vec::new()),
                ..Runs::default()
            }),
            Forge::GitHub,
            "runs.actions names nothing, so no run would be announced; name one, or remove runs.actions",
        ),
        (
            ran(Runs {
                from: Some(vec![RunFrom::Actions, RunFrom::Checks]),
                ..Runs::default()
            }),
            Forge::Forgejo,
            "runs.from \"checks\" reads GitHub's check suites and commit statuses, which forgejo has none of; read its runs from \"actions\"",
        ),
        (
            ran(Runs {
                actions: Some(vec![RunAction::InProgress]),
                conclusions: vec![Conclusion::Failure],
                ..Runs::default()
            }),
            Forge::GitLab,
            "runs.conclusions chooses how finished runs ended, and runs.actions announces none finishing; add \"completed\" to runs.actions, or remove runs.conclusions",
        ),
        (
            ran(Runs {
                branches: vec!["[".to_owned()],
                ..Runs::default()
            }),
            Forge::GitHub,
            "runs.branches: \"[\" opens a [ it never closes with a character in it; close it, or write [[] for a [ itself",
        ),
        (
            ran(Runs {
                workflows: vec!["[".to_owned()],
                ..Runs::default()
            }),
            Forge::GitHub,
            "runs.workflows: \"[\" opens a [ it never closes with a character in it; close it, or write [[] for a [ itself",
        ),
        (
            ran(Runs {
                branches: vec!["main".to_owned(), "release/../../user".to_owned()],
                ..Runs::default()
            }),
            Forge::GitHub,
            "runs.branches: \"release/../../user\" holds an empty part, '.' or '..', which names no branch in a URL path; give the branch's own name",
        ),
    ];
    for (options, forge, refusal) in refused {
        assert_eq!(options.check(forge), Err(refusal.to_owned()));
    }
    let checked = ran(Runs {
        from: Some(vec![RunFrom::Checks]),
        actions: Some(vec![RunAction::Completed]),
        conclusions: vec![Conclusion::Failure],
        ..Runs::default()
    });
    assert_eq!(checked.check(Forge::GitHub), Ok(()));
    let failures = ran(Runs {
        conclusions: vec![Conclusion::Failure],
        ..Runs::default()
    });
    assert_eq!(failures.check(Forge::GitLab), Ok(()));
}

#[test]
fn a_source_announces_every_finished_run_of_the_forge_s_own_ci_where_runs_is_absent() {
    let choice = SourceOptions::default().run_choice();
    assert_eq!(choice.from, RUN_FROM);
    assert_eq!(choice.actions, RUN_ACTIONS);
    assert_eq!(RUN_FROM, [RunFrom::Actions]);
    assert_eq!(RUN_ACTIONS, [RunAction::Completed]);
    assert!(choice.reads(RunFrom::Actions));
    assert!(!choice.reads(RunFrom::Checks));
    assert_eq!(choice.only_action(), Some(RunAction::Completed));
    assert_eq!(
        (
            choice.only_conclusion(),
            choice.only_trigger(),
            choice.only_branch()
        ),
        (None, None, None)
    );
    let globbed = ran(Runs {
        branches: vec!["release/*".to_owned()],
        ..Runs::default()
    })
    .run_choice();
    assert_eq!(globbed.only_branch(), None);
    let named = ran(Runs {
        branches: vec!["Main".to_owned()],
        triggers: vec!["push".to_owned()],
        conclusions: vec![Conclusion::Failure],
        ..Runs::default()
    })
    .run_choice();
    assert_eq!(
        (
            named.only_branch(),
            named.only_trigger(),
            named.only_conclusion()
        ),
        (Some("Main"), Some("push"), Some(Conclusion::Failure))
    );
    assert_eq!(
        [RunFrom::Actions, RunFrom::Checks].map(RunFrom::name),
        ["actions", "checks"]
    );
    assert_eq!(
        [
            RunAction::Requested,
            RunAction::InProgress,
            RunAction::Completed
        ]
        .map(RunAction::name),
        ["requested", "in_progress", "completed"]
    );
}
