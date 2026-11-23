use super::*;

fn source(milestones: Milestones) -> SourceOptions {
    SourceOptions {
        repository: Some("o/r".to_owned()),
        signals: Some(vec![Signal::Milestone]),
        milestones: Some(milestones),
        ..SourceOptions::default()
    }
}

#[test]
fn a_source_announces_closed_milestones_with_their_counts_where_it_says_nothing() {
    let choice = SourceOptions::default().milestone_choice();
    assert_eq!(choice.actions, [MilestoneAction::Closed]);
    assert_eq!(choice.titles, Vec::<herald_core::Glob>::new());
    assert!(!choice.group);
    assert!(choice.counts);
    assert_eq!(choice.most, 100);
    assert!(choice.takes(MilestoneAction::Closed, "Sprint 3"));
    assert!(!choice.takes(MilestoneAction::Opened, "Sprint 3"));
    let names = [
        MilestoneAction::Opened,
        MilestoneAction::Closed,
        MilestoneAction::Reopened,
        MilestoneAction::Deleted,
    ]
    .map(MilestoneAction::name);
    assert_eq!(names, ["opened", "closed", "reopened", "deleted"]);
}

#[test]
fn a_choice_takes_the_actions_and_titles_it_names() {
    let choice = source(Milestones {
        actions: Some(vec![MilestoneAction::Opened, MilestoneAction::Deleted]),
        titles: vec!["v*".to_owned()],
        group: Some(true),
        counts: Some(false),
        most: NonZeroUsize::new(7),
    })
    .milestone_choice();
    assert!(choice.takes(MilestoneAction::Opened, "v1.4"));
    assert!(choice.takes(MilestoneAction::Deleted, "v1.4"));
    assert!(!choice.takes(MilestoneAction::Opened, "Sprint 3"));
    assert!(!choice.takes(MilestoneAction::Closed, "v1.4"));
    assert!(choice.group);
    assert!(!choice.counts);
    assert_eq!(choice.most, 7);
}

#[test]
fn a_milestones_table_is_refused_where_it_cannot_be_honoured() {
    assert_eq!(source(Milestones::default()).check(Forge::GitHub), Ok(()));
    let grouped = source(Milestones {
        group: Some(true),
        ..Milestones::default()
    });
    assert_eq!(grouped.check(Forge::GitLab), Ok(()));
    for forge in [Forge::GitHub, Forge::Forgejo] {
        assert_eq!(
            grouped.check(forge),
            Err(format!(
                "milestones.group reads a GitLab group's milestones, which {} has none of; remove it",
                forge.kind()
            ))
        );
    }
    let unsignalled = SourceOptions {
        signals: Some(vec![Signal::Release]),
        ..source(Milestones::default())
    };
    assert_eq!(
        unsignalled.check(Forge::GitHub),
        Err("milestones chooses milestones, which no repository the source takes announces; add \"milestone\" to signals, or remove milestones".to_owned())
    );
    let silent = source(Milestones {
        actions: Some(Vec::new()),
        ..Milestones::default()
    });
    assert_eq!(
        silent.check(Forge::GitHub),
        Err("milestones.actions names nothing, so no milestone would be announced; name one, or remove milestones.actions".to_owned())
    );
    for (most, sound) in [
        (MOST_MILESTONES_ALLOWED, true),
        (MOST_MILESTONES_ALLOWED + 1, false),
    ] {
        let bounded = source(Milestones {
            most: NonZeroUsize::new(most),
            ..Milestones::default()
        });
        assert_eq!(bounded.check(Forge::GitHub).is_ok(), sound, "{most}");
    }
    assert_eq!(
        source(Milestones {
            most: NonZeroUsize::new(MOST_MILESTONES_ALLOWED + 1),
            ..Milestones::default()
        })
        .check(Forge::GitHub),
        Err("milestones.most is at most 1000; lower it".to_owned())
    );
    let globbed = source(Milestones {
        titles: vec!["[".to_owned()],
        ..Milestones::default()
    });
    assert!(
        globbed
            .check(Forge::GitHub)
            .unwrap_err()
            .starts_with("milestones.titles: ")
    );
}
