use serde_json::{Value, json};

use super::*;

fn pulling(forge: Forge, repository: &str, webhook: bool) -> ForgeSource {
    ForgeSource {
        forge,
        options: SourceOptions {
            signals: Some(vec![Signal::PullRequest]),
            webhook: webhook.then(|| SecretRef {
                secret: "HERALD_WEBHOOK".to_owned(),
            }),
            ..options(repository)
        },
        names: None,
    }
}

fn opened(path: &str) -> Value {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    json!({
        "action": "opened",
        "pull_request": {
            "number": 4, "title": "Bump", "state": "open",
            "html_url": format!("https://github.com/{path}/pull/4"),
            "user": { "login": "ann" }, "created_at": "2026-11-18T08:00:00Z",
        },
        "repository": {
            "id": 7, "name": name, "full_name": path, "visibility": "public",
            "html_url": format!("https://github.com/{path}"),
        }
    })
}

fn hooked(source: &ForgeSource, header: (&str, &str), payload: &Value) -> Result<Hooked, String> {
    source.hooked(&[header], payload.to_string().as_bytes())
}

const PULL: (&str, &str) = ("X-GitHub-Event", "pull_request");

#[test]
fn a_pull_request_delivered_to_a_source_announcing_them_is_its_event() {
    let source = pulling(Forge::GitHub, "o/r", true);
    let Ok(Hooked::Event(event)) = hooked(&source, PULL, &opened("o/r")) else {
        panic!("not an event");
    };
    assert_eq!(event.identity().as_str(), "github.com/7/pull/4/opened");
    let mut synced = opened("o/r");
    synced["action"] = json!("synchronize");
    assert_eq!(
        hooked(&source, PULL, &synced),
        Ok(Hooked::Nothing(UNCHOSEN_PULL))
    );
    let released = ForgeSource {
        options: options("o/r"),
        ..source.clone()
    };
    assert_eq!(
        hooked(&released, PULL, &opened("o/r")),
        Ok(Hooked::Nothing(UNPULLED))
    );
    let gitlab = pulling(Forge::GitLab, "group/web", true);
    let merge = json!({
        "user": { "username": "ann" },
        "project": { "id": 50, "path_with_namespace": "group/web", "web_url": "https://gitlab.com/group/web" },
        "object_attributes": {
            "iid": 3, "title": "t", "url": "https://gitlab.com/group/web/-/merge_requests/3",
            "action": "open", "created_at": "2026-11-18T08:00:00Z",
        },
    });
    let Ok(Hooked::Event(event)) =
        hooked(&gitlab, ("X-Gitlab-Event", "Merge Request Hook"), &merge)
    else {
        panic!("not an event");
    };
    assert_eq!(event.identity().as_str(), "gitlab.com/50/pull/3/opened");
    assert!(
        hooked(
            &gitlab,
            ("X-Gitlab-Event", "Merge Request Hook"),
            &json!({})
        )
        .is_err()
    );
}

#[test]
fn doctor_says_what_a_source_announcing_pull_requests_cannot_hear() {
    let noted = |source: &ForgeSource| -> Vec<&'static str> {
        source
            .notes()
            .into_iter()
            .filter(|note| [PULLS_UNFOLLOWED, PULLS_POLLED, GITLAB_PULL_AUTHORS].contains(note))
            .collect()
    };
    assert_eq!(
        noted(&pulling(Forge::GitHub, "o/r", true)),
        Vec::<&str>::new()
    );
    assert_eq!(noted(&pulling(Forge::GitHub, "o/r", false)), [PULLS_POLLED]);
    assert_eq!(
        noted(&pulling(Forge::GitLab, "g/r", true)),
        [GITLAB_PULL_AUTHORS]
    );
    assert_eq!(noted(&pulling(Forge::GitLab, "g/r", false)), [PULLS_POLLED]);
    let followed = ForgeSource {
        options: SourceOptions {
            repository: None,
            owner: Some("o".to_owned()),
            transport: Some(Transport::Events),
            ..pulling(Forge::GitHub, "o/r", true).options
        },
        ..pulling(Forge::GitHub, "o/r", true)
    };
    assert_eq!(noted(&followed), [PULLS_UNFOLLOWED]);
}
