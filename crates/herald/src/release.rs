//! The release a forge's CI run announces: read from the run's event file,
//! or asked of the forge by its tag.

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::GITHUB_TOKEN;
use herald_core::{Event, PrereleaseRule, Signal};
use herald_environment::EnvSecrets;
use herald_kinds::Unpolled;
use herald_ports::{Http, Secrets};
use herald_sources_forge::{
    Forge, Origin, Repository, Signals, SourceOptions, Token, tag_event_of,
};

use super::output::say;
use super::{Connect, JOB_TOKEN};

/// The variables a forge's CI names a run's repository, API and token in.
pub(super) struct Ci {
    /// The repository's path.
    pub(super) repository: &'static str,
    /// The base URL of the forge's API.
    pub(super) api: &'static str,
    /// The run's own token.
    token: &'static str,
}

impl Ci {
    pub(super) const fn of(forge: Forge) -> Self {
        match forge {
            Forge::GitHub | Forge::Forgejo => Self {
                repository: "GITHUB_REPOSITORY",
                api: "GITHUB_API_URL",
                token: GITHUB_TOKEN,
            },
            Forge::GitLab => Self {
                repository: "CI_PROJECT_PATH",
                api: "CI_API_V4_URL",
                token: JOB_TOKEN,
            },
        }
    }
}

pub(super) fn variable<'a>(
    vars: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, String> {
    vars.get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is not set; herald reads it from the CI run"))
}

/// The tag a GitHub Actions run pushing one is for, where it is.
pub(super) fn pushed(vars: &BTreeMap<String, String>) -> Option<&str> {
    let pushed = vars
        .get("GITHUB_EVENT_NAME")
        .is_some_and(|name| name == "push");
    vars.get("GITHUB_REF")
        .and_then(|reference| reference.strip_prefix("refs/tags/"))
        .filter(|tag| pushed && !tag.is_empty())
}

/// The release the run's own event publishes, if it does.
pub(super) fn evented(
    (forge, rule): (Forge, PrereleaseRule),
    vars: &BTreeMap<String, String>,
    out: &mut dyn Write,
) -> Result<Option<Event>, String> {
    let event_name = variable(vars, "GITHUB_EVENT_NAME")?;
    if event_name != "release" {
        say(
            out,
            &format!("The run's event is {event_name}, not a release; nothing to announce."),
        );
        return Ok(None);
    }
    let path = variable(vars, "GITHUB_EVENT_PATH")?;
    let payload =
        std::fs::read(path).map_err(|error| format!("{path} could not be read: {error}"))?;
    let event = forge
        .event(&payload, rule)
        .map_err(|error| error.to_string())?;
    if event.is_none() {
        say(
            out,
            "The release is not published, or is a draft; nothing to announce.",
        );
    }
    Ok(event)
}

/// The GitLab CI pipeline's project, as its variables name it.
pub(super) fn project(vars: &BTreeMap<String, String>) -> Result<Repository, String> {
    let id = variable(vars, "CI_PROJECT_ID")?;
    Ok(Repository {
        id: id
            .parse()
            .map_err(|_| format!("CI_PROJECT_ID is {id}, not a project's id"))?,
        name: variable(vars, "CI_PROJECT_NAME")?.to_owned(),
        page: variable(vars, "CI_PROJECT_URL").ok().map(ToOwned::to_owned),
    })
}

/// The release tagged `tag` of the repository `options` names on `forge`,
/// asked for with the source's token, or else the run's own; the
/// repository asked for too where the run does not name it.
pub(super) async fn tagged(
    options: &SourceOptions,
    (forge, signals): (Forge, &Signals),
    tag: &str,
    known: Option<Repository>,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<Option<Event>, String> {
    let secrets = EnvSecrets::from_vars(vars.clone());
    let token = match &options.token {
        Some(named) => {
            Some(Token::Named(secrets.secret(&named.secret).await.map_err(
                |_| format!("{} is not set; the source names it", named.secret),
            )?))
        }
        None => secrets
            .secret(Ci::of(forge).token)
            .await
            .ok()
            .map(Token::Run),
    };
    let origin = Origin::new(
        forge,
        options,
        options.repository.as_deref().unwrap_or_default(),
    );
    let http = connect.reach(origin.api())?;
    let rule = signals.prerelease;
    let token = token.as_ref();
    let tag_event = || async {
        tag_event_of(&origin, &http, (tag, token), rule)
            .await
            .map(Some)
            .map_err(Unpolled::into_reason)
    };
    if !signals.takes(Signal::Release) {
        return tag_event().await;
    }
    let asked = |request| async { http.send(request).await.map_err(|error| error.to_string()) };
    let said = Unpolled::into_reason;
    let request = origin.release_request(tag, token).map_err(said)?;
    let answer = asked(request).await?;
    if answer.status == herald_core::status::NOT_FOUND && signals.takes(Signal::Tag) {
        return tag_event().await;
    }
    let repository = if let Some(repository) = known {
        repository
    } else {
        let described = asked(origin.repository_request(token).map_err(said)?).await?;
        forge
            .repository(&described)
            .map_err(|error| error.to_string())?
    };
    let event = forge
        .release(&repository, &answer, rule)
        .map_err(|error| error.to_string())?;
    if event.is_none() {
        say(
            out,
            &format!("The release {tag} is a draft; nothing to announce."),
        );
    }
    Ok(event)
}
