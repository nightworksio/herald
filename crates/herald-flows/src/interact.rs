//! A command a member gave, answered: subscribing by role or by the store,
//! and what a project last announced, shown with a route's template
//! (ADR-0016).

use herald_config::{Config, RouteConfig};
use herald_core::{
    Answering, Asked, Command, Context, DestinationName, Draft, Event, Latest, RouteId, Said,
    Subscription, announced, body_template, chosen, chunk, completions, escaped, safe_url,
    title_template,
};
use herald_ports::{Http, Store};

use crate::announce::Ports;
use crate::interacting::{Interacting, Replying};
use crate::prepare::Connected;

/// What herald answers a command: Markdown, who sees it, and what kept herald
/// from answering, for its log, where something did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The answer, in Markdown.
    pub said: String,
    /// Whether the channel sees it, or the asker alone.
    pub public: bool,
    /// What kept herald from answering, where something did.
    pub trouble: Option<String>,
}

fn told(said: &Said) -> Answer {
    Answer {
        said: said.to_string(),
        public: false,
        trouble: None,
    }
}

fn unavailable(trouble: impl Into<String>) -> Answer {
    Answer {
        trouble: Some(trouble.into()),
        ..told(&Said::Unavailable)
    }
}

/// The routes of `config` that announce on `destination` in `place`.
fn placed<'a>(
    config: &'a Config,
    destination: &DestinationName,
    place: &str,
) -> Vec<&'a RouteConfig> {
    config
        .routes
        .iter()
        .filter(|route| {
            &route.route.destination == destination && route.options.place() == Some(place)
        })
        .collect()
}

fn ids(routes: &[&RouteConfig]) -> Vec<RouteId> {
    routes.iter().map(|route| route.route.id.clone()).collect()
}

/// What herald answers `asked`, given on a destination of `connected`, with
/// what the store of `ports` keeps.
pub async fn interact<S: Store, C, G, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    asked: &Asked,
) -> Answer {
    let disabled = || told(&Said::Disabled(asked.command));
    let Some((interacting, http)) = connected.interacting(&asked.destination) else {
        return disabled();
    };
    let answering = interacting.answering();
    if !answering.takes(asked.command) {
        return disabled();
    }
    let placed = placed(config, &asked.destination, &asked.place);
    let latests = match ports.store.latests().await {
        Ok(latests) => latests,
        Err(error) => return unavailable(error.to_string()),
    };
    let announced = announced(&latests, &ids(&placed));
    let latest = match chosen(asked.project.as_deref(), &announced) {
        Ok(latest) => latest,
        Err(unchosen) => return told(&Said::Unchosen(unchosen)),
    };
    let routes: Vec<&RouteConfig> = placed
        .into_iter()
        .filter(|route| latest.routes.contains(&route.route.id))
        .collect();
    match asked.command {
        Command::Subscribe | Command::Unsubscribe => {
            subscribing(interacting, http, ports.store, asked, latest, &routes).await
        }
        Command::Latest | Command::Changelog => shown(config, &answering, asked, latest, &routes),
    }
}

/// The projects announced on `destination` in `place` whose names hold
/// `typed`, as completions of it; none where the store cannot list them.
pub async fn completed<S: Store>(
    config: &Config,
    store: &S,
    destination: &DestinationName,
    place: &str,
    typed: &str,
) -> Vec<String> {
    let latests = store.latests().await.unwrap_or_default();
    completions(
        typed,
        &announced(&latests, &ids(&placed(config, destination, place))),
    )
}

/// Sends `answer` where `replying` names, on `destination` of `connected`, in
/// as many parts as its destination needs.
///
/// # Errors
///
/// When the destination takes no commands, or refuses a part.
pub async fn replied<H: Http>(
    connected: &Connected<H>,
    destination: &DestinationName,
    replying: &Replying,
    answer: &Answer,
) -> Result<(), String> {
    let (interacting, http) = connected
        .interacting(destination)
        .ok_or_else(|| format!("{} takes no commands", destination.as_str()))?;
    let (dialect, room) = interacting.room();
    let parts = chunk(&answer.said, dialect, room);
    for request in interacting.answered(replying, &parts, answer.public) {
        interacting.sent(http, request).await?;
    }
    Ok(())
}

/// Says that the command `asked`, carried where no endpoint answered it,
/// is being worked, where its destination waits for that: a Discord
/// interaction the gateway carried.
///
/// # Errors
///
/// When the destination takes no commands, or refuses it.
pub async fn deferral<H: Http>(
    connected: &Connected<H>,
    destination: &DestinationName,
    replying: &Replying,
    asked: &Asked,
) -> Result<(), String> {
    let (interacting, http) = connected
        .interacting(destination)
        .ok_or_else(|| format!("{} takes no commands", destination.as_str()))?;
    match (interacting, replying) {
        (Interacting::Discord(discord), Replying::Discord(replying)) => {
            let public = discord
                .answering()
                .replies
                .public(asked.command, asked.share);
            interacting
                .sent(http, discord.defer(replying, public))
                .await
        }
        _ => Ok(()),
    }
}

/// Whether `asked` subscribes, or unsubscribes, a member who `holds` the
/// subscription already, or else what herald says instead.
fn decided(asked: &Asked, holds: bool) -> Result<bool, fn(String) -> Said> {
    match (asked.command, asked.toggles, holds) {
        (Command::Subscribe, _, false) => Ok(true),
        (Command::Subscribe, true, true) => Ok(false),
        (Command::Subscribe, false, true) => Err(Said::AlreadySubscribed),
        (_, _, true) => Ok(false),
        (_, _, false) => Err(Said::NotSubscribed),
    }
}

async fn subscribing<S: Store, H: Http>(
    interacting: &Interacting,
    http: &H,
    store: &S,
    asked: &Asked,
    latest: &Latest,
    routes: &[&RouteConfig],
) -> Answer {
    let project = latest.project.clone();
    let unoffered = || told(&Said::Unoffered(project.clone()));
    let done = |give: bool| {
        told(&if give {
            Said::Subscribed(project.clone())
        } else {
            Said::Unsubscribed(project.clone())
        })
    };
    if let Interacting::Discord(discord) = interacting {
        let mut roles: Vec<String> = Vec::new();
        for role in routes
            .iter()
            .filter_map(|route| route.options.opt_in_role())
        {
            if !roles.contains(&role) {
                roles.push(role);
            }
        }
        let (Some(member), false) = (&asked.member, roles.is_empty()) else {
            return unoffered();
        };
        let holds = roles.iter().any(|role| member.roles.contains(role));
        let give = match decided(asked, holds) {
            Ok(give) => give,
            Err(said) => return told(&said(project)),
        };
        for role in &roles {
            let Some(request) = discord.role(&member.guild, &asked.user, role, give) else {
                return unavailable(
                    "a role could not be asked of Discord: an id is not written as Discord writes one",
                );
            };
            if let Err(trouble) = interacting.sent(http, request).await {
                return unavailable(trouble);
            }
        }
        return done(give);
    }
    if !routes
        .iter()
        .any(|route| route.route.delivery.mentions.subscribers)
    {
        return unoffered();
    }
    let subscribers = match store.subscribers(&asked.destination, &project).await {
        Ok(subscribers) => subscribers,
        Err(error) => return unavailable(error.to_string()),
    };
    let holds = subscribers
        .iter()
        .any(|subscription| subscription.subscriber == asked.user);
    let give = match decided(asked, holds) {
        Ok(give) => give,
        Err(said) => return told(&said(project)),
    };
    let subscription = Subscription {
        destination: asked.destination.clone(),
        project: project.clone(),
        subscriber: asked.user.clone(),
        inbox: None,
    };
    let written = if give {
        store.subscribe(&subscription).await
    } else {
        store.unsubscribe(&subscription).await
    };
    match written {
        Ok(()) => done(give),
        Err(error) => unavailable(error.to_string()),
    }
}

fn shown(
    config: &Config,
    answering: &Answering,
    asked: &Asked,
    latest: &Latest,
    routes: &[&RouteConfig],
) -> Answer {
    let Some(route) = routes.first() else {
        return unavailable("no route announced the project here");
    };
    let shown = latest.shown(answering.prereleases);
    let event = match Event::normalise(Draft {
        identity: format!("latest/{}", latest.project),
        kind: shown.kind,
        project: latest.project.clone(),
        version: shown.version.clone(),
        title: shown.title.clone(),
        body: shown.body.clone(),
        links: shown.links.clone(),
        prerelease: shown.prerelease,
        occurred: shown.occurred,
    }) {
        Ok(event) => event,
        Err(problem) => return unavailable(problem.to_string()),
    };
    let template = answering
        .template
        .clone()
        .unwrap_or_else(|| route.route.template.clone());
    let context = Context {
        event: &event,
        route: &route.route.id,
        herald: env!("CARGO_PKG_VERSION"),
    };
    let said = if asked.command == Command::Latest {
        config
            .templates
            .render_line(&title_template(&template), &context)
            .map(|title| latest_text(&title, &event))
    } else {
        config
            .templates
            .render(&body_template(&template), &context)
            .map(|body| changelog_text(&body, &event))
    };
    match said {
        Ok(said) => Answer {
            said,
            public: answering.replies.public(asked.command, asked.share),
            trouble: None,
        },
        Err(error) => unavailable(error.to_string()),
    }
}

/// What `latest` answers: the title, linked to the release, and the day it
/// happened.
fn latest_text(title: &str, event: &Event) -> String {
    let title = escaped(title);
    let headline = match event.link("release").or_else(|| event.first_link()) {
        Some(link) => format!("**[{title}]({})**", safe_url(link)),
        None => format!("**{title}**"),
    };
    format!("{headline}\n{}", event.occurred().strftime("%Y-%m-%d"))
}

/// What `changelog` answers: the notes, and a link to the changelog or the
/// release.
fn changelog_text(body: &str, event: &Event) -> String {
    let notes = body.trim();
    let notes = if notes.is_empty() {
        "No notes were announced."
    } else {
        notes
    };
    let link = event
        .link("changelog")
        .map(|link| ("Changelog", link))
        .or_else(|| event.link("release").map(|link| ("Release", link)));
    match link {
        Some((name, link)) => format!("{notes}\n\n[{name}]({})", safe_url(link)),
        None => notes.to_owned(),
    }
}
