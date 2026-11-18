//! `herald announce --from`: the release a forge's CI run publishes,
//! announced from the sources that name the run's repository to every route
//! that takes it.

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::told::{report, why};
use herald_compose::{Composed, Host, Hosting, ServingOn, Through};
use herald_config::{Config, Uses};
use herald_core::{Event, RouteId, Signal, SourceName};
use herald_flows::{Fresh, Outcome, Stop, Within, announce_within, held_or_announced};
use herald_ports::{Base, Keyring, Outbound, Store};
use herald_sources_forge::{Forge, ForgeSource, Signals, SourceOptions, ci_run, of_a_run};

use super::dry_run::Dry;
use super::output::say;
use super::release::{Ci, evented, project, pushed, tagged, variable};
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
                        && named
                            .options
                            .repository
                            .as_deref()
                            .is_some_and(|named| named.eq_ignore_ascii_case(repository))
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
    let repository = options.repository.as_deref().unwrap_or_default();
    let signals = options.signals_of(repository.rsplit('/').next().unwrap_or(repository));
    let name = vars.get("GITHUB_EVENT_NAME").map_or("", String::as_str);
    if tag.is_none() && forge == Forge::GitHub && of_a_run(forge, name) {
        return ran(options, (forge, &signals), (name, vars), out);
    }
    let tag = tag.or_else(|| {
        (forge != Forge::GitLab && signals.takes(Signal::Tag))
            .then(|| pushed(vars))
            .flatten()
    });
    match (forge, tag) {
        (Forge::GitLab, None) => {
            say(out, "The pipeline is not for a tag; nothing to announce.");
            Ok(None)
        }
        (Forge::GitLab, Some(tag)) => {
            let project = project(vars)?;
            tagged(
                options,
                (forge, &signals),
                tag,
                Some(project),
                vars,
                connect,
                out,
            )
            .await
        }
        (_, Some(tag)) => tagged(options, (forge, &signals), tag, None, vars, connect, out).await,
        (_, None) if !signals.takes(Signal::Release) => {
            say(
                out,
                "The source announces no releases of the repository; nothing to announce.",
            );
            Ok(None)
        }
        (_, None) => evented((forge, signals.prerelease), vars, out),
    }
}

/// The run a GitHub Actions run's own event `name` is of: its event where
/// `signals` take runs and the source's `runs` choose it.
fn ran(
    options: &SourceOptions,
    (forge, signals): (Forge, &Signals),
    (name, vars): (&str, &BTreeMap<String, String>),
    out: &mut dyn Write,
) -> Result<Option<Event>, String> {
    if !signals.takes(Signal::Run) {
        say(
            out,
            &format!(
                "The run's event is {name}, and the source announces no runs of the repository; nothing to announce."
            ),
        );
        return Ok(None);
    }
    let path = variable(vars, "GITHUB_EVENT_PATH")?;
    let payload =
        std::fs::read(path).map_err(|error| format!("{path} could not be read: {error}"))?;
    let ran = ci_run((forge, options), (name, &payload)).map_err(|error| error.to_string())?;
    if ran.is_none() {
        say(
            out,
            "The run is not one the source's runs announce; nothing to announce.",
        );
    }
    Ok(ran)
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
    let reaching = |_: Base<'_>, _: &Outbound| Ok(Connection::new(dry.clone()));
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
fn reported(told: Vec<Result<Fresh, Stop>>, out: &mut dyn Write) -> u8 {
    let mut code = DONE;
    for fresh in told {
        let fresh = match fresh {
            Ok(fresh) => fresh,
            Err(stop) => {
                say(out, &format!("stopped: {}", why(&stop)));
                code = STOPPED;
                continue;
            }
        };
        if let Some(due) = fresh.held {
            let version = fresh.event.version().unwrap_or(fresh.event.title());
            say(
                out,
                &format!("The tag {version} is held for its Release until {due}."),
            );
        }
        for announced in fresh.announced {
            if matches!(announced.outcome, Outcome::Stopped { .. }) {
                code = STOPPED;
            }
            say(out, &report(&announced));
        }
    }
    code
}

/// An event, announced through the routes and from the sources it names.
struct Announcing<'a>(Routed<'a>);

impl<H: Host> Through<H> for Announcing<'_> {
    type Done = Vec<Result<Fresh, Stop>>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        run: &str,
    ) -> Vec<Result<Fresh, Stop>> {
        let routed = self.0;
        let (config, connected, ports) = (serving.config, serving.connected, serving.ports);
        let mut all = Vec::new();
        for source in routed.sources {
            let told = match routed.within {
                Some(within) => Ok(Fresh {
                    event: routed.event.clone(),
                    announced: announce_within(
                        config,
                        connected,
                        ports,
                        source,
                        routed.event,
                        Within::routes(within),
                        run,
                    )
                    .await,
                    held: None,
                }),
                None => {
                    held_or_announced(config, connected, ports, (source, routed.event), run).await
                }
            };
            all.push(told);
        }
        all
    }
}
