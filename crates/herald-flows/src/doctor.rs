use herald_config::{Config, Destination, RouteConfig, Server, Source, Store};
use herald_core::{Inbound, Pinning, RecordKind, Sandbox};
use herald_ports::{Secrets, Signer};

use crate::prepare::keyed;

/// A feature the config asks for that this herald cannot serve (ADR-0002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    /// What the config asks for.
    pub feature: String,
    /// The config key that asks for it.
    pub key: String,
    /// Why it cannot be served, and what would serve it.
    pub why: String,
}

/// What doctor finds in a config: the gaps, and what the config leaves to
/// chance without being a gap.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Findings {
    /// The features that cannot be served.
    pub gaps: Vec<Gap>,
    /// What holds, at a cost worth knowing.
    pub notes: Vec<String>,
    /// What no run could get past.
    pub mistakes: Vec<String>,
}

/// What a derived marker key costs (ADR-0011).
const DERIVED_KEY: &str = "Each post's markers are keyed by its destination's credential, as no [markers] key is set. Rotating that credential forgets every earlier marker, so an announcement made before the rotation can be posted again on a store that has only markers. Set [markers] key = { secret = \"HERALD_MARKER_SECRET\" } to keep them.";
/// What the markers store cannot hold apart (ADR-0005).
const UNGUARDED: &str = "The markers store has no compare-and-set, so two runs at once can both post an announcement; run one herald at a time, or choose a store that keeps records.";

/// What the markers store cannot run (ADR-0005).
const UNPOLLED: &str = "`herald poll` stops on the markers store, which keeps no cursors; announcing each release from its own event needs none.";
/// What the Actions cache keeps apart and drops (ADR-0005).
const SCOPED: &str = "The Actions cache keeps records apart for each ref a run is on, and drops what no run reads for a week: a release run on its tag does not see what a run on the default branch wrote. Run housekeeping and polling on one ref, or choose a store that keeps records across refs.";
/// Where the sqlite store must run (ADR-0005).
const FILED: &str = "Every record is kept in one SQLite file, so run herald where that file outlives the run: a CI runner's disk is gone when its job ends.";
/// What repository variables hold at most (ADR-0005).
const CROWDED: &str = "Each record is a repository variable: a repository holds at most 500, and a run is handed at most 256 KB of them, so many records can crowd out the repository's own variables.";

/// Each soundness bound the config sets above its default, as a note: what
/// herald holds a request or a template to is looser than it ships with.
fn loosened(config: &Config, notes: &mut Vec<String>) {
    let inbound = &config.server.inbound;
    let render = &config.render;
    let bounds = [
        (
            "server.body",
            config.server.body > Server::BODY,
            "an inbound request's body may hold more",
        ),
        (
            "server.inbound.replay_window",
            inbound.replay_window > Inbound::DEFAULT.replay_window,
            "a copy of a signed request is let through for longer",
        ),
        (
            "server.inbound.signatures",
            inbound.signatures > Inbound::DEFAULT.signatures,
            "a request may make herald check more signatures",
        ),
        (
            "server.inbound.covered",
            inbound.covered > Inbound::DEFAULT.covered,
            "a signature may make herald read more headers",
        ),
        (
            "render.fuel",
            render.fuel > Sandbox::DEFAULT.fuel,
            "a template may run longer",
        ),
        (
            "render.recursion",
            render.recursion > Sandbox::DEFAULT.recursion,
            "a template may nest deeper",
        ),
        (
            "render.output",
            render.output > Sandbox::DEFAULT.output,
            "a template may write more",
        ),
    ];
    for (key, above, cost) in bounds {
        if above {
            notes.push(format!(
                "{key} is set above its default, so {cost}; set it back where nothing needs it."
            ));
        }
    }
}

fn gap(feature: &str, key: String, why: String) -> Gap {
    Gap {
        feature: feature.to_owned(),
        key,
        why,
    }
}

fn unpinned(config: &Config, index: usize, route: &RouteConfig, gaps: &mut Vec<Gap>) {
    if route.route.delivery.pin != Some(Pinning::Store) || config.store.keeps(RecordKind::Pins) {
        return;
    }
    let or = config
        .store
        .kept_by(RecordKind::Pins)
        .map_or_else(String::new, |hint| format!("{hint}, or "));
    gaps.push(gap(
        "a pin the store remembers",
        format!("routes[{index}].pin"),
        format!(
            "the {} store keeps no pins; {or}set pin = \"markers\"",
            config.store.kind()
        ),
    ));
}

fn unkept(config: &Config, index: usize, route: &RouteConfig, gaps: &mut Vec<Gap>) {
    let bot = matches!(
        config.destinations.get(&route.route.destination),
        Some(Destination::Discord(options)) if options.token.is_some()
    );
    let key = |field: &str| format!("routes[{index}].{field}");
    if !route.profile.features.reads_back {
        let cannot = if bot {
            "a forum's posts are not read back"
        } else {
            "a webhook cannot read back its posts"
        };
        gaps.push(gap(
            "sending each announcement once",
            key("destination"),
            format!("the markers store reads announcements back from their posts, and {cannot}; post through a bot to a text or announcement channel, or choose a store that keeps records"),
        ));
    }
}

/// Why a store kind outside `stores` cannot be served.
#[must_use]
pub fn unserved(stores: &[&str]) -> String {
    let listed = match stores.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => stores.join(""),
    };
    format!("this herald keeps records in {listed}; set [store] kind to one of them")
}

/// Each ActivityPub actor's key in `config`, read through `secrets` and
/// loaded through `signer`: a mistake where it cannot sign as the actor,
/// and a note where it is not set here to check.
pub async fn keys<S: Secrets, G: Signer>(config: &Config, secrets: &S, signer: &G) -> Findings {
    let mut findings = Findings::default();
    for (name, destination) in &config.destinations {
        let Destination::ActivityPub(options) = destination else {
            continue;
        };
        let Ok(key) = secrets.secret(&options.key.secret).await else {
            findings.notes.push(format!(
                "The key of the destination \"{}\" is not checked, as {} is not set here.",
                name.as_str(),
                options.key.secret
            ));
            continue;
        };
        if let Err(unkeyed) = keyed(signer, name, &key).await {
            findings.mistakes.push(unkeyed.to_string());
        }
    }
    findings
}

/// What a layer of support serves (ADR-0002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer<'a> {
    /// The store kinds it keeps records in.
    pub stores: &'a [&'a str],
    /// Whether it serves herald's endpoint, where commands are posted.
    pub endpoint: bool,
    /// Whether it holds sessions open, which carry commands to it.
    pub sessions: bool,
}

/// The commands `config`'s destinations take that `layer` cannot carry to
/// herald (ADR-0016).
fn unanswered(config: &Config, layer: Layer<'_>, gaps: &mut Vec<Gap>) {
    for (name, destination) in &config.destinations {
        let (posted, how) = match destination {
            Destination::Discord(options) => match &options.interactions {
                Some(interactions) => (
                    interactions.via == herald_dest_discord::Via::Http,
                    (
                        "interactions Discord posts",
                        "the gateway with via = \"gateway\"",
                    ),
                ),
                None => continue,
            },
            Destination::Slack(options) => match &options.interactions {
                Some(interactions) => (
                    interactions.via == herald_dest_slack::Via::Http,
                    ("commands Slack posts", "Socket Mode with via = \"socket\""),
                ),
                None => continue,
            },
            Destination::Matrix(options) => match &options.interactions {
                Some(interactions) => (
                    interactions.via == herald_dest_matrix::Via::Appservice,
                    (
                        "transactions the homeserver pushes",
                        "a sync with via = \"sync\"",
                    ),
                ),
                None => continue,
            },
            Destination::Mastodon(_) | Destination::ActivityPub(_) => continue,
        };
        let key = format!("destinations.{}.interactions.via", name.as_str());
        let (carried, session) = how;
        if posted && !layer.endpoint {
            gaps.push(gap(
                &format!("the {carried}"),
                key,
                format!("they are posted to herald's endpoint, which herald server and the serverless drivers serve; run one of them, or carry them over {session} in herald server"),
            ));
        } else if !posted && !layer.sessions {
            gaps.push(gap(
                "a session carrying commands",
                key,
                "only herald server holds a session open; run it, or have the commands posted to herald's endpoint".to_owned(),
            ));
        }
    }
}

/// What `config` asks for that `layer` cannot serve, and what it leaves to
/// chance (ADR-0002).
#[must_use]
pub fn doctor(config: &Config, layer: Layer<'_>) -> Findings {
    let stores = layer.stores;
    let mut findings = Findings::default();
    unanswered(config, layer, &mut findings.gaps);
    if config.marker_key.is_none() {
        findings.notes.push(DERIVED_KEY.to_owned());
    }
    loosened(config, &mut findings.notes);
    let kind = config.store.kind();
    if !stores.contains(&kind) {
        findings.gaps.push(gap(
            &format!("keeping records in {kind}"),
            "store.kind".to_owned(),
            unserved(stores),
        ));
    }
    for route in &config.routes {
        let moving = route.route.filter.sources.iter().find(|name| {
            matches!(config.sources.get(*name), Some(Source::Registry(registry, _)) if registry.moving_tags())
        });
        if let (Some(source), None) = (moving, &route.route.filter.versions) {
            findings.notes.push(format!(
                "The route {} takes {}'s tags with no versions pattern, so a tag that moves, such as latest, is announced the first time it is pushed. Set versions = \"*.*.*\" on the route to leave such tags out.",
                route.route.id.as_str(),
                source.as_str()
            ));
        }
    }
    match config.store {
        Store::ActionsCache(_) => findings.notes.push(SCOPED.to_owned()),
        Store::RepoVariables(_) => findings.notes.push(CROWDED.to_owned()),
        Store::Sqlite(_) => findings.notes.push(FILED.to_owned()),
        _ => {}
    }
    for (index, route) in config.routes.iter().enumerate() {
        unpinned(config, index, route, &mut findings.gaps);
    }
    if matches!(config.store, Store::Markers) {
        for (index, route) in config.routes.iter().enumerate() {
            unkept(config, index, route, &mut findings.gaps);
        }
        if config.schedule.poll.is_some() {
            findings.gaps.push(gap(
                "polling every source",
                "schedule.poll".to_owned(),
                "the markers store keeps no cursors; choose a store that does".to_owned(),
            ));
        } else if !config.sources.is_empty() {
            findings.notes.push(UNPOLLED.to_owned());
        }
        findings.notes.push(UNGUARDED.to_owned());
    }
    findings
}
