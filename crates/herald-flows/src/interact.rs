//! A command a member gave, answered: subscribing by role or by the store,
//! and what a project last announced, shown with a route's template
//! (ADR-0016).

use herald_config::{Config, RouteConfig};
use herald_core::{
    Answering, Asked, CHANGELOG_LINK, Command, Context, DestinationName, Draft, Event, Latest,
    Phrase, Place, RELEASE_LINK, RouteId, Said, Subscription, Wording, announced, body_template,
    chosen, chunk, completions, escaped, safe_url, title_template,
};
use herald_kinds::{Interacted, Replying};
use herald_ports::{Announcements, Http, Store, Subscriptions};

use crate::announce::Ports;
use crate::interacting::sent;
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

/// What herald replies before it is worded.
enum Reply {
    /// Something herald says, to the asker alone.
    Told(Said),
    /// What herald could not answer, for its log.
    Unavailable(String),
    /// What a project announced, shown as `public` says.
    Shown { said: String, public: bool },
}

impl Reply {
    fn unavailable(trouble: impl Into<String>) -> Self {
        Self::Unavailable(trouble.into())
    }

    /// The reply as `wording` words it.
    fn worded(self, wording: &Wording) -> Answer {
        match self {
            Self::Told(said) => Answer {
                said: wording.say(&said),
                public: false,
                trouble: None,
            },
            Self::Unavailable(trouble) => Answer {
                said: wording.say(&Said::Unavailable),
                public: false,
                trouble: Some(trouble),
            },
            Self::Shown { said, public } => Answer {
                said,
                public,
                trouble: None,
            },
        }
    }
}

/// The routes of `config` that announce on `destination` in `place`.
fn placed<'a>(
    config: &'a Config,
    destination: &DestinationName,
    place: &Place,
) -> Vec<&'a RouteConfig> {
    config
        .routes
        .iter()
        .filter(|route| {
            &route.route.destination == destination
                && route
                    .options
                    .place()
                    .is_some_and(|channel| place.is(channel))
        })
        .collect()
}

fn id<'r>(route: &'r &RouteConfig) -> &'r RouteId {
    &route.route.id
}

/// What herald answers `asked`, given on a destination of `connected`, with
/// what the store of `ports` keeps.
pub async fn interact<S: Store, C, G, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    asked: &Asked,
) -> Answer {
    let wording = connected
        .interacting(&asked.destination)
        .map_or_else(Wording::default, |(interacting, _)| {
            interacting.answering().wording
        });
    reply(config, connected, ports, asked)
        .await
        .worded(&wording)
}

async fn reply<S: Store, C, G, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    asked: &Asked,
) -> Reply {
    let disabled = || Reply::Told(Said::Disabled(asked.command));
    let Some((interacting, http)) = connected.interacting(&asked.destination) else {
        return disabled();
    };
    let answering = interacting.answering();
    if !answering.takes(asked.command) {
        return disabled();
    }
    let placed = placed(config, &asked.destination, &asked.place);
    let every_latest = match ports.store.announcements().every_latest().await {
        Ok(every_latest) => every_latest,
        Err(error) => return Reply::unavailable(error.to_string()),
    };
    let here = match chosen(
        asked.project.as_deref(),
        &announced(&every_latest, &placed, id),
    ) {
        Ok(here) => here,
        Err(unchosen) => return Reply::Told(Said::Unchosen(unchosen)),
    };
    match asked.command {
        Command::Subscribe | Command::Unsubscribe => {
            let routes: Vec<&RouteConfig> = placed
                .iter()
                .copied()
                .filter(|route| here.latest.routes.contains(&route.route.id))
                .collect();
            subscribing(interacting, http, ports.store, asked, here.latest, &routes).await
        }
        Command::Latest | Command::Changelog => {
            shown(config, &answering, asked, here.latest, here.route)
        }
    }
}

/// The projects announced on `destination` in `place` whose names hold
/// `typed`, as completions of it; none where the store cannot list them.
pub async fn completed<S: Store>(
    config: &Config,
    store: &S,
    destination: &DestinationName,
    place: &Place,
    typed: &str,
) -> Vec<String> {
    let every_latest = store
        .announcements()
        .every_latest()
        .await
        .unwrap_or_default();
    let placed = placed(config, destination, place);
    completions(typed, &announced(&every_latest, &placed, id))
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
        sent(interacting, http, request).await?;
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
    match interacting.deferred(replying, asked) {
        Some(request) => sent(interacting, http, request).await,
        None => Ok(()),
    }
}

/// Whether `asked` subscribes, or unsubscribes, a member who `holds` the
/// subscription already, or else what herald says instead.
fn decided(asked: &Asked, holds: bool) -> Result<bool, fn(String) -> Said> {
    match (asked.command, asked.toggles, holds) {
        (Command::Subscribe, _, false) => Ok(true),
        (Command::Subscribe, false, true) => Err(Said::AlreadySubscribed),
        (_, _, true) => Ok(false),
        (_, _, false) => Err(Said::NotSubscribed),
    }
}

async fn subscribing<S: Store, H: Http>(
    interacting: &Interacted,
    http: &H,
    store: &S,
    asked: &Asked,
    latest: &Latest,
    routes: &[&RouteConfig],
) -> Reply {
    let project = latest.project.clone();
    let unoffered = || Reply::Told(Said::Unoffered(project.clone()));
    let done = |give: bool| {
        Reply::Told(if give {
            Said::Subscribed(project.clone())
        } else {
            Said::Unsubscribed(project.clone())
        })
    };
    if interacting.gives_roles() {
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
            Err(said) => return Reply::Told(said(project)),
        };
        for role in &roles {
            let Some(request) = interacting.role(&member.guild, &asked.user, role, give) else {
                return Reply::unavailable(
                    "a role could not be asked of Discord: an id is not written as Discord writes one",
                );
            };
            if let Err(trouble) = sent(interacting, http, request).await {
                return Reply::unavailable(trouble);
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
    let subscribers = match store
        .subscriptions()
        .subscribers(&asked.destination, &project)
        .await
    {
        Ok(subscribers) => subscribers,
        Err(error) => return Reply::unavailable(error.to_string()),
    };
    let holds = subscribers
        .iter()
        .any(|subscription| subscription.subscriber == asked.user);
    let give = match decided(asked, holds) {
        Ok(give) => give,
        Err(said) => return Reply::Told(said(project)),
    };
    let subscription = Subscription {
        destination: asked.destination.clone(),
        project: project.clone(),
        subscriber: asked.user.clone(),
        inbox: None,
        pending: None,
    };
    let written = if give {
        store.subscriptions().subscribe(&subscription).await
    } else {
        store.subscriptions().unsubscribe(&subscription).await
    };
    match written {
        Ok(()) => done(give),
        Err(error) => Reply::unavailable(error.to_string()),
    }
}

fn shown(
    config: &Config,
    answering: &Answering,
    asked: &Asked,
    latest: &Latest,
    route: &RouteConfig,
) -> Reply {
    let shown = latest.shown(answering.prereleases);
    let event = match Event::normalise(Draft {
        identity: format!("latest/{}", latest.project),
        kind: shown.kind.clone(),
        project: latest.project.clone(),
        title: shown.title.clone(),
        body: shown.body.clone(),
        links: shown.links.clone(),
        attributes: shown.attributes.clone(),
        occurred: shown.occurred,
    }) {
        Ok(event) => event,
        Err(problem) => return Reply::unavailable(problem.to_string()),
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
            .map(|title| latest_text(&title, &event, &answering.wording))
    } else {
        config
            .templates
            .render(&body_template(&template), &context)
            .map(|body| changelog_text(&body, &event, &answering.wording))
    };
    match said {
        Ok(said) => Reply::Shown {
            said,
            public: answering.replies.public(asked.command, asked.share),
        },
        Err(error) => Reply::unavailable(error.to_string()),
    }
}

/// What `latest` answers: the title, linked to the release, and the day it
/// happened.
fn latest_text(title: &str, event: &Event, wording: &Wording) -> String {
    let title = escaped(title);
    let headline = match event.link(RELEASE_LINK).or_else(|| event.first_link()) {
        Some(link) => format!("**[{title}]({})**", safe_url(link)),
        None => format!("**{title}**"),
    };
    format!("{headline}\n{}", wording.date(event.occurred()))
}

/// What `changelog` answers: the notes, and a link to the changelog or the
/// release.
fn changelog_text(body: &str, event: &Event, wording: &Wording) -> String {
    let notes = body.trim();
    let notes = if notes.is_empty() {
        wording.text(Phrase::NoNotes)
    } else {
        notes
    };
    let link = event
        .link(CHANGELOG_LINK)
        .map(|link| (wording.text(Phrase::ChangelogLink), link))
        .or_else(|| {
            event
                .link(RELEASE_LINK)
                .map(|link| (wording.text(Phrase::ReleaseLink), link))
        });
    match link {
        Some((name, link)) => format!("{notes}\n\n[{name}]({})", safe_url(link)),
        None => notes.to_owned(),
    }
}
