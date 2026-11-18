//! A webhook registered on a GitLab owner's member, whose group cannot send
//! its own on GitLab Free (ADR-0041).

use herald_core::media;
use herald_kinds::{Registered, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _, Method, Request, Secret, SecretText};
use herald_wire::answered;
use serde::Deserialize;

use crate::forge::authorized;
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
    answered(HOOKS, &answer).map_err(Unpolled::unreadable)?;
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
    answered(SCOPED, &answer).map_err(|error| error.to_string())?;
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

/// A webhook on the project of id `id` at `api`, pointing at `url` and
/// signed with `secret`, sending its pipelines too where `ran` says so and
/// its vulnerabilities where `alerted` does, registered through `http`
/// with `token` where it has none pointing at `url`.
pub(crate) async fn registered(
    http: &dyn DynHttp,
    (api, token): (&str, Option<&Token>),
    (id, (ran, alerted)): (&str, (bool, bool)),
    (url, secret): (&str, &Secret),
) -> Result<Registered, Unpolled> {
    if found(http, (api, token), id, url).await? {
        return Ok(Registered::Found);
    }
    let hooks = format!("{api}/projects/{id}/hooks");
    let body = serde_json::json!({
        "url": url,
        "token": secret.expose(),
        "releases_events": true,
        "tag_push_events": true,
        "push_events": false,
        "pipeline_events": ran,
        "vulnerability_events": alerted,
    });
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
    answered(REGISTERED, &answer).map_err(Unpolled::unreadable)?;
    Ok(Registered::Made)
}

#[cfg(test)]
mod tests;
