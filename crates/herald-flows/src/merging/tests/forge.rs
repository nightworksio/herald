//! What keeps a queue the forge runs from handing a pull request over: a
//! forge queue herald cannot hand to, or one merging by another method.

use super::*;

#[test]
fn a_forge_s_queue_herald_cannot_hand_to_or_merging_by_another_method_takes_no_step() {
    let started = started();
    let store = FakeStore::default();
    let personal = Branched {
        queued: herald_kinds::Queued {
            unhanded: Some("GitHub has none here".to_owned()),
            ..herald_kinds::Queued::default()
        },
        ..base()
    };
    assert_eq!(
        idle(
            &started,
            (&personal, None, AppliedBy::Command),
            Some(&store)
        ),
        [format!("GitHub has none here{}", branch::UNSTARTED)]
    );
    let squashing = |method| Branched {
        queue: Some(true),
        queued: herald_kinds::Queued {
            method: Some(method),
            ..herald_kinds::Queued::default()
        },
        ..base()
    };
    assert_eq!(
        idle(
            &started,
            (&squashing(Method::Squash), None, AppliedBy::Command),
            Some(&store)
        ),
        Vec::<String>::new()
    );
    let merged = idle(
        &started,
        (&squashing(Method::Merge), None, AppliedBy::Command),
        Some(&store),
    );
    assert_eq!(merged.len(), 1);
    assert!(merged[0].contains("merges by \"merge\""), "{merged:?}");
    assert!(merged[0].ends_with("give method = \"merge\""), "{merged:?}");
    let mut repository = started.clone();
    repository.method = Some(Method::Repository);
    let one = |allowed: Vec<Method>| herald_kinds::Methods {
        allowed,
        whole: true,
    };
    let allows = |methods| {
        idle(
            &repository,
            (
                &squashing(Method::Squash),
                Some(&methods),
                AppliedBy::Command,
            ),
            Some(&store),
        )
    };
    assert_eq!(allows(one(vec![Method::Squash])), Vec::<String>::new());
    assert_eq!(allows(one(vec![Method::Rebase])).len(), 1);
    assert_eq!(
        allows(one(vec![Method::Rebase, Method::Squash])),
        Vec::<String>::new()
    );
    assert_eq!(
        idle(
            &repository,
            (&squashing(Method::Rebase), None, AppliedBy::Command),
            Some(&store)
        ),
        Vec::<String>::new()
    );
}

#[test]
fn a_repository_method_is_held_to_the_forge_queue_s_own_where_the_methods_are_read() {
    let mut repository = started();
    repository.method = Some(Method::Repository);
    let squashing = Branched {
        queue: Some(true),
        queued: herald_kinds::Queued {
            method: Some(Method::Squash),
            ..herald_kinds::Queued::default()
        },
        ..base()
    };
    let idled = |allowed: Option<Vec<Method>>| {
        let reader = Fixed {
            methods: allowed.map(|allowed| herald_kinds::Methods {
                allowed,
                whole: true,
            }),
            ..Fixed::listing(Some(squashing.clone()), Some(Vec::new()))
        };
        let http = ScriptedHttp::new(Vec::new());
        let store = FakeStore::default();
        let asked = (
            ("app-main", &repository),
            Some(&store),
            (false, AppliedBy::Command),
        );
        match pollster::block_on(found((&reader, &http), asked, Printed::Configured, now())) {
            Reading::Due(due) => due.idle,
            Reading::Done(found) => panic!("{found:?}"),
        }
    };
    assert_eq!(idled(Some(vec![Method::Squash])), Vec::<String>::new());
    let rebasing = idled(Some(vec![Method::Rebase]));
    assert_eq!(rebasing.len(), 1);
    assert!(
        rebasing[0].ends_with("give method = \"squash\""),
        "{rebasing:?}"
    );
    assert_eq!(idled(None), Vec::<String>::new());
}
