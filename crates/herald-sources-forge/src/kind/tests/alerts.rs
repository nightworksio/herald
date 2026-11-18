use herald_core::Visibility;
use serde_json::{Value, json};

use super::*;

fn alerting(forge: Forge, repository: &str, webhook: bool) -> ForgeSource {
    ForgeSource {
        forge,
        options: SourceOptions {
            signals: Some(vec![Signal::Alert]),
            webhook: webhook.then(|| SecretRef {
                secret: "HERALD_WEBHOOK".to_owned(),
            }),
            ..options(repository)
        },
        names: None,
    }
}

fn dependabot(path: &str, action: &str) -> Value {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    json!({
        "action": action,
        "alert": {
            "number": 4, "state": "open",
            "html_url": format!("https://github.com/{path}/security/dependabot/4"),
            "created_at": "2026-11-18T08:00:00Z",
            "security_advisory": { "severity": "low" },
        },
        "repository": {
            "id": 7, "name": name, "full_name": path, "visibility": "public",
            "html_url": format!("https://github.com/{path}"),
        }
    })
}

fn vulnerability(path: &str) -> Value {
    json!({
        "object_kind": "vulnerability",
        "object_attributes": {
            "url": format!("https://gitlab.com/{path}/-/security/vulnerabilities/2"),
            "state": "detected", "project_id": 50, "severity": "critical",
            "report_type": "sast", "created_at": "2026-11-18T08:00:00Z",
        }
    })
}

fn hooked(source: &ForgeSource, header: (&str, &str), payload: &Value) -> Result<Hooked, String> {
    source.hooked(&[header], payload.to_string().as_bytes())
}

const DEPENDABOT: (&str, &str) = ("X-GitHub-Event", "dependabot_alert");
const VULNERABILITY: (&str, &str) = ("X-Gitlab-Event", "Vulnerability Hook");

#[test]
fn an_alert_delivered_to_a_source_announcing_alerts_of_its_repository_is_its_event() {
    let source = alerting(Forge::GitHub, "o/r", true);
    let Ok(Hooked::Event(event)) = hooked(&source, DEPENDABOT, &dependabot("o/r", "created"))
    else {
        panic!("not an event");
    };
    assert_eq!(
        event.identity().as_str(),
        "github.com/7/alert/dependency-scanning/4/created"
    );
    let origin = event.origin().unwrap();
    assert_eq!(
        (
            origin.repository.as_str(),
            origin.visibility,
            origin.id.as_str()
        ),
        ("o/r", Visibility::Public, "7")
    );
    assert_eq!(
        hooked(&source, DEPENDABOT, &dependabot("o/r", "fixed")),
        Ok(Hooked::Nothing(UNCHOSEN_ALERT))
    );
    let released = ForgeSource {
        options: SourceOptions {
            signals: None,
            ..source.options.clone()
        },
        ..source.clone()
    };
    assert_eq!(
        hooked(&released, DEPENDABOT, &dependabot("o/r", "created")),
        Ok(Hooked::Nothing(UNALERTED))
    );
    assert_eq!(
        hooked(&source, DEPENDABOT, &dependabot("o/other", "created")),
        Err("GitHub's Dependabot alert event could not be read: it is not of o/r, the repository its source names".to_owned())
    );
    let gitlab = alerting(Forge::GitLab, "group/web", true);
    let Ok(Hooked::Event(event)) = hooked(&gitlab, VULNERABILITY, &vulnerability("group/web"))
    else {
        panic!("not an event");
    };
    assert_eq!(
        event.identity().as_str(),
        "gitlab.com/50/alert/code-scanning/2/created"
    );
    let origin = event.origin().unwrap();
    assert_eq!(
        (
            origin.repository.as_str(),
            origin.visibility,
            origin.owner.as_str()
        ),
        ("group/web", Visibility::Private, "group")
    );
    assert!(hooked(&gitlab, VULNERABILITY, &json!({})).is_err());
}

#[test]
fn an_alert_delivered_to_an_owner_source_is_its_member_s_where_it_selects_it() {
    let owned = |forge: Forge, owner: &str| ForgeSource {
        options: SourceOptions {
            repository: None,
            owner: Some(owner.to_owned()),
            visibility: Some(vec![Visibility::Public, Visibility::Private]),
            ..alerting(forge, "unused/unused", true).options
        },
        ..alerting(forge, "unused/unused", true)
    };
    let member = hooked(
        &owned(Forge::GitHub, "o"),
        DEPENDABOT,
        &dependabot("o/r", "created"),
    );
    let Ok(Hooked::Member { id, visibility, .. }) = member else {
        panic!("{member:?}");
    };
    assert_eq!((id.as_str(), visibility), ("7", Some(Visibility::Public)));
    let listing = ForgeSource {
        names: Some(vec!["other".to_owned()]),
        ..owned(Forge::GitHub, "o")
    };
    assert_eq!(
        hooked(&listing, DEPENDABOT, &dependabot("o/r", "created")),
        Ok(Hooked::Nothing(crate::forge::UNSELECTED))
    );
    let group = hooked(
        &owned(Forge::GitLab, "group"),
        VULNERABILITY,
        &vulnerability("group/web"),
    );
    let Ok(Hooked::Member { id, visibility, .. }) = group else {
        panic!("{group:?}");
    };
    assert_eq!((id.as_str(), visibility), ("50", None));
    assert!(
        hooked(
            &owned(Forge::GitLab, "else"),
            VULNERABILITY,
            &vulnerability("group/web")
        )
        .is_err()
    );
    let excluding = ForgeSource {
        options: SourceOptions {
            exclude: vec!["web".to_owned()],
            ..owned(Forge::GitLab, "group").options
        },
        ..owned(Forge::GitLab, "group")
    };
    assert_eq!(
        hooked(&excluding, VULNERABILITY, &vulnerability("group/web")),
        Ok(Hooked::Nothing(crate::forge::UNSELECTED))
    );
}

#[test]
fn doctor_says_what_a_source_announcing_alerts_cannot_hear() {
    let noted = |source: &ForgeSource| -> Vec<&'static str> {
        source
            .notes()
            .into_iter()
            .filter(|note| {
                [
                    ALERTS_UNFOLLOWED,
                    ALERTS_POLLED,
                    GITHUB_ALERTS,
                    GITLAB_ALERTS,
                ]
                .contains(note)
            })
            .collect()
    };
    assert_eq!(
        noted(&alerting(Forge::GitHub, "o/r", true)),
        [GITHUB_ALERTS]
    );
    assert_eq!(
        noted(&alerting(Forge::GitHub, "o/r", false)),
        [ALERTS_POLLED, GITHUB_ALERTS]
    );
    assert_eq!(
        noted(&alerting(Forge::GitLab, "g/r", true)),
        [GITLAB_ALERTS]
    );
    let followed = ForgeSource {
        options: SourceOptions {
            repository: None,
            owner: Some("o".to_owned()),
            transport: Some(Transport::Events),
            ..alerting(Forge::GitHub, "o/r", true).options
        },
        ..alerting(Forge::GitHub, "o/r", true)
    };
    assert_eq!(noted(&followed), [ALERTS_UNFOLLOWED, GITHUB_ALERTS]);
    let released = ForgeSource {
        options: options("o/r"),
        ..followed
    };
    assert_eq!(noted(&released), Vec::<&str>::new());
}
