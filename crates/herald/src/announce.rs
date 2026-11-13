//! `herald announce --from`: the release a forge's CI run publishes,
//! announced from the sources that name the run's repository to every route
//! that takes it.

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::told::report;
use herald_compose::{Composed, Host, Hosting, ServingOn, Through};
use herald_config::{Config, Uses};
use herald_core::{Event, RouteId, SourceName};
use herald_flows::{Announced, Outcome, announce_within};
use herald_ports::{Keyring, Outbound, Store};
use herald_sources_forge::{Forge, ForgeSource, SourceOptions};

use super::dry_run::Dry;
use super::output::say;
use super::release::{Ci, evented, project, tagged, variable};
use super::stores::{Native, composed};
use super::{Connect, Connection, Connector, DONE, STOPPED, place, waiting};

/// How an announcement is asked for.
#[derive(Debug, Clone, Copy)]
pub(super) struct Announce<'a> {
    /// The forge whose CI the run is in.
    pub(super) forge: Forge,
    /// The release to announce by its tag, in place of the run's event.
    pub(super) release: Option<&'a str>,
    /// Whether to print the requests and send nothing.
    pub(super) dry_run: bool,
}

pub(super) async fn announcing_from(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    asked: Announce<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let forge = asked.forge;
    let ci = Ci::of(forge);
    let repository = variable(vars, ci.repository)?;
    let named = naming(config, forge, repository);
    let Some((_, named_options)) = named.first() else {
        return Err(format!(
            "no source in the config names {repository}; add [sources.<name>] with kind = \"{}\" and repository = \"{repository}\"",
            forge.kind()
        ));
    };
    let options = &SourceOptions {
        api: named_options
            .api
            .clone()
            .or_else(|| vars.get(ci.api).filter(|api| !api.is_empty()).cloned()),
        ..(*named_options).clone()
    };
    let tag = tag(forge, asked.release, vars);
    let Some(event) = released(options, forge, tag, vars, connect, out).await? else {
        return Ok(DONE);
    };
    let sources: Vec<&SourceName> = named.iter().map(|(name, _)| *name).collect();
    let routed = Routed {
        sources: &sources,
        within: None,
        event: &event,
    };
    announcing(config, vars, connect, routed, asked.dry_run, out).await
}

/// Each source of `config` on `forge` that names `repository`, with its
/// options.
fn naming<'c>(
    config: &'c Config,
    forge: Forge,
    repository: &str,
) -> Vec<(&'c SourceName, &'c SourceOptions)> {
    config
        .sources
        .iter()
        .filter_map(|(name, source)| {
            source
                .of::<ForgeSource>()
                .filter(|named| {
                    named.forge == forge
                        && named.options.repository.eq_ignore_ascii_case(repository)
                })
                .map(|named| (name, &named.options))
        })
        .collect()
}

/// The tag of the release to announce: `release`, or on GitLab the tag the
/// pipeline is for.
fn tag<'a>(
    forge: Forge,
    release: Option<&'a str>,
    vars: &'a BTreeMap<String, String>,
) -> Option<&'a str> {
    match forge {
        Forge::GitLab => release.or_else(|| {
            vars.get("CI_COMMIT_TAG")
                .map(String::as_str)
                .filter(|tag| !tag.is_empty())
        }),
        Forge::GitHub | Forge::Forgejo => release,
    }
}

/// The release tagged `tag`, or else the one the run's own event publishes,
/// where there is one to announce.
async fn released(
    options: &SourceOptions,
    forge: Forge,
    tag: Option<&str>,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<Option<Event>, String> {
    match (forge, tag) {
        (Forge::GitLab, None) => {
            say(out, "The pipeline is not for a tag; nothing to announce.");
            Ok(None)
        }
        (Forge::GitLab, Some(tag)) => {
            let project = project(vars)?;
            tagged(options, forge, tag, Some(project), vars, connect, out).await
        }
        (_, Some(tag)) => tagged(options, forge, tag, None, vars, connect, out).await,
        (_, None) => evented(forge, vars, out),
    }
}

/// An event, and the sources and routes it is announced from and to.
pub(super) struct Routed<'a> {
    /// The sources it comes from.
    pub(super) sources: &'a [&'a SourceName],
    /// The routes it may go to, where only some may take it.
    pub(super) within: Option<&'a [RouteId]>,
    /// The event.
    pub(super) event: &'a Event,
}

/// `routed` announced to every route that takes it, or, on a dry run, the
/// requests a first announcement sends printed and nothing sent.
pub(super) async fn announcing(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    routed: Routed<'_>,
    dry_run: bool,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let dry = Dry::default();
    let reaching = |_: &str, _: &Outbound| Ok(Connection::new(dry.clone()));
    let dried = Connector::new(&reaching, &waiting);
    let composed = if dry_run {
        let native = Native::new(vars, &dried);
        Composed::dry(native, config.clone(), Uses::Announces, place(vars)).await?
    } else {
        composed(
            config,
            vars,
            connect,
            Hosting::Actions,
            Uses::Announces,
            place(vars),
        )
        .await?
    };
    let announced = composed.run(Announcing(routed)).await;
    if dry_run {
        dry.print(out);
    }
    Ok(reported(announced, out))
}

/// The code `announced` ends a run with, each announcement reported.
fn reported(announced: Vec<Announced>, out: &mut dyn Write) -> u8 {
    let mut code = DONE;
    for announced in announced {
        if matches!(announced.outcome, Outcome::Stopped { .. }) {
            code = STOPPED;
        }
        say(out, &report(&announced));
    }
    code
}

/// An event, announced through the routes and from the sources it names.
struct Announcing<'a>(Routed<'a>);

impl<H: Host> Through<H> for Announcing<'_> {
    type Done = Vec<Announced>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        run: &str,
    ) -> Vec<Announced> {
        let routed = self.0;
        let mut all = Vec::new();
        for source in routed.sources {
            all.extend(
                announce_within(
                    serving.config,
                    serving.connected,
                    serving.ports,
                    source,
                    routed.event,
                    routed.within,
                    run,
                )
                .await,
            );
        }
        all
    }
}
