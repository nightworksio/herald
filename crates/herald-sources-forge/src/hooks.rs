//! A webhook registered on a GitLab owner's member, whose group cannot send
//! its own on GitLab Free (ADR-0041).

use herald_core::media;
use herald_kinds::{Registered, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _, Method, Request, Secret, SecretText};
use serde::Deserialize;
use serde_json::Value;

use crate::forge::authorized;
use crate::limited::checked;
use crate::wire::read;
use crate::{Forge, Token};

/// What a project's webhooks are read as.
const HOOKS: &str = "the project's webhooks";
/// What a webhook registered is read as.
const REGISTERED: &str = "the project's new webhook";
/// The most webhooks one listing of a project's asks for.
const PER_PAGE: usize = 100;

/// A project's webhook, by where it points.
#[derive(Deserialize)]
struct Hook {
    url: String,
}

/// What a token's scopes are read as.
const SCOPED: &str = "the token's scopes";
/// The scope a GitLab token registers a project webhook with.
const API_SCOPE: &str = "api";

/// A token, by its scopes.
#[derive(Deserialize)]
struct Scoped {
    scopes: Vec<String>,
}

/// Whether the project of id `id` at `api` has a webhook pointing at
/// `url`, asked through `http` with `token`.
pub(crate) async fn found(
    http: &dyn DynHttp,
    (api, token): (&str, Option<&Token>),
    id: &str,
    url: &str,
) -> Result<bool, Unpolled> {
    let hooks = format!("{api}/projects/{id}/hooks");
    let listing = authorized(
        Forge::GitLab,
        Request::new(
            Method::Get,
            SecretText::plain(format!("{hooks}?per_page={PER_PAGE}")),
        ),
        token,
    );
    let answer = Erased(http)
        .send(listing)
        .await
        .map_err(Unpolled::unreachable)?;
    checked(Forge::GitLab, HOOKS, &answer)?;
    let found = read::<Vec<Hook>>(HOOKS, &answer.body).map_err(Unpolled::unreadable)?;
    Ok(found.iter().any(|hook| hook.url == url))
}

/// Why `token` may not register a project webhook at `api`, asked through
/// `http`, where it may not: it names no `api` scope.
pub(crate) async fn may_register(
    http: &dyn DynHttp,
    (api, token): (&str, Option<&Token>),
) -> Result<(), String> {
    let asking = authorized(
        Forge::GitLab,
        Request::new(
            Method::Get,
            SecretText::plain(format!("{api}/personal_access_tokens/self")),
        ),
        token,
    );
    let answer = Erased(http)
        .send(asking)
        .await
        .map_err(|error| error.to_string())?;
    checked(Forge::GitLab, SCOPED, &answer).map_err(Unpolled::into_reason)?;
    let scoped = read::<Scoped>(SCOPED, &answer.body).map_err(|error| error.to_string())?;
    if scoped.scopes.iter().any(|scope| scope == API_SCOPE) {
        Ok(())
    } else {
        Err(format!(
            "its scopes are {}, and registering a project webhook needs the {API_SCOPE} scope",
            scoped.scopes.join(", ")
        ))
    }
}

/// What a project's webhook may send beside its releases and tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sent {
    /// Its pipelines.
    Pipelines,
    /// Its vulnerabilities.
    Vulnerabilities,
    /// Its merge requests.
    MergeRequests,
    /// Its issues.
    Issues,
    /// Its confidential issues.
    ConfidentialIssues,
}

impl Sent {
    /// Everything it may send.
    pub(crate) const ALL: [Self; 5] = [
        Self::Pipelines,
        Self::Vulnerabilities,
        Self::MergeRequests,
        Self::Issues,
        Self::ConfidentialIssues,
    ];

    /// The key GitLab's hook API turns it on by.
    const fn key(self) -> &'static str {
        match self {
            Self::Pipelines => "pipeline_events",
            Self::Vulnerabilities => "vulnerability_events",
            Self::MergeRequests => "merge_requests_events",
            Self::Issues => "issues_events",
            Self::ConfidentialIssues => "confidential_issues_events",
        }
    }
}

/// A webhook on the project of id `id` at `api`, pointing at `url` and
/// signed with `secret`, sending what `sent` says beside its releases and
/// tags, registered through `http` with `token` where it has none pointing
/// at `url`.
pub(crate) async fn registered(
    http: &dyn DynHttp,
    (api, token): (&str, Option<&Token>),
    (id, sent): (&str, &[Sent]),
    (url, secret): (&str, &Secret),
) -> Result<Registered, Unpolled> {
    if found(http, (api, token), id, url).await? {
        return Ok(Registered::Found);
    }
    let hooks = format!("{api}/projects/{id}/hooks");
    let mut body = serde_json::Map::from_iter([
        ("url".to_owned(), Value::from(url)),
        ("token".to_owned(), Value::from(secret.expose())),
        ("releases_events".to_owned(), Value::Bool(true)),
        ("tag_push_events".to_owned(), Value::Bool(true)),
        ("push_events".to_owned(), Value::Bool(false)),
    ]);
    body.extend(Sent::ALL.map(|each| (each.key().to_owned(), Value::Bool(sent.contains(&each)))));
    let body = Value::Object(body);
    let registering = authorized(
        Forge::GitLab,
        Request::new(Method::Post, SecretText::plain(hooks))
            .header("Content-Type", SecretText::plain(media::JSON))
            .sealed_body(body.to_string()),
        token,
    );
    let answer = Erased(http)
        .send(registering)
        .await
        .map_err(Unpolled::unreachable)?;
    checked(Forge::GitLab, REGISTERED, &answer)?;
    Ok(Registered::Made)
}

#[cfg(test)]
mod tests;
