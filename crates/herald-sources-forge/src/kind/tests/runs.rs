use herald_core::Visibility;
use serde_json::{Value, json};

use super::*;
use crate::{RunFrom, Runs};

const FINISHED: &str = "completed";

fn ran(signals: &[Signal]) -> SourceOptions {
    SourceOptions {
        signals: Some(signals.to_vec()),
        ..options("o/r")
    }
}

fn owned(owner: &str, forge: Forge) -> ForgeSource {
    ForgeSource {
        forge,
        options: SourceOptions {
            repository: None,
            owner: Some(owner.to_owned()),
            exclude: vec!["hidden".to_owned()],
            visibility: Some(vec![Visibility::Public, Visibility::Private]),
            ..ran(&[Signal::Run])
        },
        names: None,
    }
}

fn workflow(path: &str, status: &str) -> Value {
    let (owner, name) = path.rsplit_once('/').unwrap();
    json!({
        "action": status,
        "workflow_run": {
            "id": 9, "name": "CI", "run_attempt": 1, "status": status, "conclusion": "success",
            "html_url": format!("https://github.com/{path}/actions/runs/9"),
            "updated_at": "2026-11-17T10:00:00Z"
        },
        "repository": {
            "id": 7, "name": name, "full_name": path, "visibility": "private",
            "html_url": format!("https://github.com/{path}"), "owner": {"login": owner}
        }
    })
}

fn hooked(source: &ForgeSource, payload: &Value) -> Result<Hooked, String> {
    source.hooked(
        &[("X-GitHub-Event", "workflow_run")],
        payload.to_string().as_bytes(),
    )
}

fn source(options: SourceOptions) -> ForgeSource {
    ForgeSource {
        forge: Forge::GitHub,
        options,
        names: None,
    }
}

#[test]
fn a_run_delivered_to_a_source_announcing_runs_of_its_repository_is_its_event_from_there() {
    let announced = hooked(&source(ran(&[Signal::Run])), &workflow("o/r", FINISHED)).unwrap();
    let Hooked::Event(event) = announced else {
        panic!("{announced:?}");
    };
    assert_eq!(event.identity().as_str(), "github.com/7/run/9/1");
    let origin = event.origin().unwrap();
    assert_eq!(
        (origin.repository.as_str(), origin.visibility),
        ("o/r", Visibility::Private)
    );
    assert_eq!(
        hooked(&source(ran(&[Signal::Release])), &workflow("o/r", FINISHED)),
        Ok(Hooked::Nothing(UNRAN))
    );
    assert_eq!(
        hooked(
            &source(ran(&[Signal::Run])),
            &workflow("o/r", "in_progress")
        ),
        Ok(Hooked::Nothing(UNCHOSEN))
    );
    assert_eq!(
        hooked(&source(ran(&[Signal::Run])), &workflow("o/other", FINISHED)),
        Err(format!(
            "{} could not be read: it is not of o/r, the repository its source names",
            Forge::GitHub.runs_what()
        ))
    );
    assert!(hooked(&source(ran(&[Signal::Run])), &json!({"workflow_run": {}})).is_err());
}

#[test]
fn a_run_delivered_to_an_owner_source_is_its_member_s_where_the_owner_holds_and_selects_it() {
    let member = hooked(&owned("O", Forge::GitHub), &workflow("o/r", FINISHED)).unwrap();
    let Hooked::Member {
        event,
        id,
        visibility,
    } = member
    else {
        panic!("{member:?}");
    };
    assert_eq!(
        (event.project(), id.as_str(), visibility),
        ("r", "7", Some(Visibility::Private))
    );
    assert_eq!(
        hooked(&owned("o", Forge::GitHub), &workflow("o/hidden", FINISHED)),
        Ok(Hooked::Nothing(crate::forge::UNSELECTED))
    );
    let listing = |names: &[&str]| ForgeSource {
        names: Some(names.iter().map(|name| (*name).to_owned()).collect()),
        ..owned("o", Forge::GitHub)
    };
    assert_eq!(
        hooked(&listing(&["other"]), &workflow("o/r", FINISHED)),
        Ok(Hooked::Nothing(crate::forge::UNSELECTED))
    );
    assert!(matches!(
        hooked(&listing(&["r"]), &workflow("o/r", FINISHED)),
        Ok(Hooked::Member { .. })
    ));
    assert_eq!(
        hooked(&owned("someone", Forge::GitHub), &workflow("o/r", FINISHED)),
        Err(format!(
            "{} could not be read: it is not of a repository someone, the owner its source names, holds",
            Forge::GitHub.runs_what()
        ))
    );
    let pipeline = |path: &str| {
        json!({
            "object_attributes": {"id": 3, "status": "success", "finished_at": "2026-11-17 10:00:00 UTC"},
            "project": {"id": 8, "name": "r", "path_with_namespace": path, "web_url": format!("https://gitlab.com/{path}")}
        })
        .to_string()
    };
    let group = |subgroups: bool| ForgeSource {
        options: SourceOptions {
            subgroups: Some(subgroups),
            ..owned("group", Forge::GitLab).options
        },
        ..owned("group", Forge::GitLab)
    };
    let headers = [("X-Gitlab-Event", "Pipeline Hook")];
    assert!(matches!(
        group(true).hooked(&headers, pipeline("group/sub/r").as_bytes()),
        Ok(Hooked::Member { .. })
    ));
    assert!(
        group(false)
            .hooked(&headers, pipeline("group/sub/r").as_bytes())
            .is_err()
    );
}
#[test]
fn a_source_s_kinds_follow_every_signal_it_may_announce() {
    let cases: [(&[Signal], &[Kind]); 8] = [
        (&[Signal::Release], &[Kind::RELEASE]),
        (&[Signal::Tag, Signal::Run], &[Kind::RELEASE, Kind::RUN]),
        (&[Signal::Package], &[Kind::PACKAGE_VERSION]),
        (&[Signal::Run], &[Kind::RUN]),
        (
            &[Signal::Package, Signal::Run],
            &[Kind::PACKAGE_VERSION, Kind::RUN],
        ),
        (
            &[Signal::Release, Signal::Package, Signal::Run],
            &[Kind::RELEASE, Kind::PACKAGE_VERSION, Kind::RUN],
        ),
        (
            &[Signal::Release, Signal::Package],
            &[Kind::RELEASE, Kind::PACKAGE_VERSION],
        ),
        (&[], &[Kind::RELEASE]),
    ];
    for (signals, kinds) in cases {
        assert_eq!(source(ran(signals)).kinds(), kinds, "{signals:?}");
    }
}

#[test]
fn doctor_says_what_runs_cost_where_the_owner_s_events_or_a_poll_of_checks_leave_them_out() {
    let followed = ForgeSource {
        options: SourceOptions {
            transport: Some(crate::Transport::Events),
            webhook: None,
            ..owned("o", Forge::GitHub).options
        },
        ..owned("o", Forge::GitHub)
    };
    assert_eq!(followed.notes(), [RUNS_UNFOLLOWED]);
    let released = ForgeSource {
        options: SourceOptions {
            signals: Some(vec![Signal::Release]),
            ..followed.options.clone()
        },
        ..followed.clone()
    };
    assert_eq!(released.notes(), Vec::<&str>::new());
    let checked = source(SourceOptions {
        webhook: None,
        runs: Some(Runs {
            from: Some(vec![RunFrom::Checks]),
            ..Runs::default()
        }),
        ..ran(&[Signal::Run])
    });
    assert_eq!(checked.notes(), [CHECKS_POLLED]);
    let hooked = source(SourceOptions {
        runs: checked.options.runs.clone(),
        ..ran(&[Signal::Run])
    });
    assert_eq!(hooked.notes(), Vec::<&str>::new());
    let gitlab = ForgeSource {
        options: SourceOptions {
            transport: Some(crate::Transport::Graphql),
            webhook: None,
            ..owned("o", Forge::GitLab).options
        },
        ..owned("o", Forge::GitLab)
    };
    assert_eq!(gitlab.notes(), Vec::<&str>::new());
}
