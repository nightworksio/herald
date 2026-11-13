//! What a config asks for that a layer cannot serve, and what it leaves to
//! chance (ADR-0002).

use herald_config::{Config, KeyCache, RouteConfig, Server};
use herald_core::{Features, Inbound, Pinning, Pins, RecordKind, Sandbox, Threads};
use herald_kinds::Carried;

mod layer;
mod online;

pub use layer::{Cadence, Egress, Here, Layer, Memory, Rounds, Tier};
pub use online::{keys, online, secrets};

/// A feature the config asks for that this herald cannot serve (ADR-0002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    /// What the config asks for.
    pub feature: String,
    /// The config key that asks for it.
    pub key: String,
    /// Why it cannot be served, and what would serve it.
    pub why: String,
    /// The first layer that serves it, where any does.
    pub first_layer: Option<Tier>,
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

impl Findings {
    /// These findings, followed by `more`.
    pub fn extend(&mut self, more: Self) {
        self.gaps.extend(more.gaps);
        self.notes.extend(more.notes);
        self.mistakes.extend(more.mistakes);
    }

    /// `gap`, where `here` holds the config to what is set here, and else
    /// a note that it is served only where its setting is made.
    fn unless_set(&mut self, here: &Here, gap: Gap) {
        if here.online {
            self.gaps.push(gap);
        } else {
            self.notes.push(format!(
                "{}: {} is served only where its setting is made: {}.",
                gap.key, gap.feature, gap.why
            ));
        }
    }
}

/// What a derived marker key costs (ADR-0011).
const DERIVED_KEY: &str = "Each post's markers are keyed by its destination's credential, as no [markers] key is set. Rotating that credential forgets every earlier marker, so an announcement made before the rotation can be posted again on a store that has only markers. Set [markers] key = { secret = \"HERALD_MARKER_SECRET\" } to keep them.";
/// What the markers store cannot hold apart (ADR-0005).
const UNGUARDED: &str = "The markers store has no compare-and-set, so two runs at once can both post an announcement; run one herald at a time, or choose a store that keeps records.";

/// What the markers store cannot run (ADR-0005).
const UNPOLLED: &str = "`herald poll` stops on the markers store, which keeps no cursors; announcing each release from its own event needs none.";

/// Why a layer with no endpoint serves nothing posted to herald.
const UNHOSTED: &str = "it is served at herald's endpoint, which this layer has none of";

/// The variable that says a host's own egress refuses private addresses
/// (ADR-0017).
pub const EGRESS_GUARDED: &str = "HERALD_EGRESS_GUARDED";

/// Whether `value`, of [`EGRESS_GUARDED`], says the host's own egress
/// refuses private addresses.
#[must_use]
pub fn egress_guarded(value: Option<&str>) -> bool {
    value == Some("true")
}

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
            "server.keys.document",
            inbound.document > Inbound::DEFAULT.document,
            "a key's or an actor's document herald reads may hold more",
        ),
        (
            "http.body",
            config.http.body > herald_ports::Outbound::BODY,
            "a response herald reads may hold more",
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
        first_layer: None,
    }
}

/// `names`, listed in a sentence, the last joined by `last`.
fn listed(names: &[&str], last: &str) -> String {
    match names.split_last() {
        Some((final_name, rest)) if !rest.is_empty() => {
            format!("{} {last} {final_name}", rest.join(", "))
        }
        _ => names.join(""),
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
    let key = |field: &str| format!("routes[{index}].{field}");
    if !route.profile.features.reads_back {
        let cannot = config
            .destinations
            .get(&route.route.destination)
            .map_or(herald_kinds::UNREAD, |destination| destination.unread_why());
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
    format!(
        "this herald keeps records in {}; set [store] kind to one of them",
        listed(stores, "or")
    )
}

/// The commands `config`'s destinations take that `layer` cannot carry to
/// herald, each named with the `layers` that can (ADR-0016).
fn unanswered(config: &Config, layer: &Layer, layers: &[Layer], gaps: &mut Vec<Gap>) {
    let holders: Vec<&str> = layers
        .iter()
        .filter(|layer| layer.sessions)
        .map(|layer| layer.name)
        .collect();
    let carrying = |session: &str| {
        if holders.is_empty() {
            String::new()
        } else {
            format!(
                ", or carry them over {session} on the {} layer",
                listed(&holders, "or")
            )
        }
    };
    let holding = if holders.is_empty() {
        "no layer holds a session open; have".to_owned()
    } else {
        format!(
            "a session is held open only on the {} layer; run herald there, or have",
            listed(&holders, "or")
        )
    };
    for (name, destination) in &config.destinations {
        let Some(Carried {
            posted,
            what: carried,
            session,
        }) = destination.carried()
        else {
            continue;
        };
        let key = format!("destinations.{}.interactions.via", name.as_str());
        if posted && !layer.endpoint {
            gaps.push(gap(
                &format!("the {carried}"),
                key,
                format!(
                    "they are posted to herald's endpoint, which this layer has none of; run herald where it serves one{}",
                    carrying(session)
                ),
            ));
        } else if !posted && !layer.sessions {
            gaps.push(gap(
                "a session carrying commands",
                key,
                format!("{holding} the commands posted to herald's endpoint"),
            ));
        }
    }
}

/// What `config` asks of herald's endpoint, on a layer with none.
fn unhosted(config: &Config, gaps: &mut Vec<Gap>) {
    for (name, destination) in &config.destinations {
        if destination.acts() {
            gaps.push(gap(
                "herald's own ActivityPub actor",
                format!("destinations.{}", name.as_str()),
                UNHOSTED.to_owned(),
            ));
        }
    }
    for (name, source) in &config.sources {
        if source.webhook().is_some() {
            gaps.push(gap(
                "verified webhooks",
                format!("sources.{}.webhook", name.as_str()),
                format!("{UNHOSTED}; announce the run's own event, or poll the source"),
            ));
        }
    }
    for (name, app) in &config.apps {
        if !app.tokens.is_empty() {
            gaps.push(gap(
                "the HTTP API",
                format!("apps.{name}.tokens"),
                format!("{UNHOSTED}; announce the app's events with herald announce --event"),
            ));
        }
    }
}

/// What `config` asks of herald's endpoint that `layer`, which serves one,
/// leaves to its host or its store.
fn hosted(config: &Config, layer: &Layer, here: &Here, findings: &mut Findings) {
    if !config.store.keeps_deliveries() {
        for (name, source) in &config.sources {
            if source.webhook().is_some() {
                findings.gaps.push(gap(
                    "refusing a replayed webhook",
                    format!("sources.{}.webhook", name.as_str()),
                    format!("the {} store keeps no deliveries, so a replay reaches the parser and only the ledger skips it; choose a store that keeps records", config.store.kind()),
                ));
            }
        }
    }
    for (name, source) in &config.sources {
        if let Some(note) = source.note() {
            findings
                .notes
                .push(format!("The source {}: {note}", name.as_str()));
        }
    }
    let actors: Vec<&str> = config
        .destinations
        .iter()
        .filter(|(_, destination)| destination.acts())
        .map(|(name, _)| name.as_str())
        .collect();
    if layer.egress == Egress::Unchecked && !here.guarded {
        for actor in &actors {
            findings.unless_set(
                here,
                gap(
                    "herald's own ActivityPub actor",
                    format!("destinations.{actor}"),
                    format!("the {} layer resolves names out of herald's sight, so the actor reads no key and delivers to no inbox until {EGRESS_GUARDED} is true; set it once the host's egress refuses private, loopback and link-local addresses", layer.name),
                ),
            );
        }
    }
    if !actors.is_empty()
        && layer.keyring == Memory::PerRequest
        && config.server.keys == KeyCache::Memory
    {
        findings.notes.push(format!(
            "[server.keys] cache is \"memory\", which the {} layer drops after each request, so every inbound activity has its signer's key read again; set cache = \"store\" to keep keys across requests.",
            layer.name
        ));
    }
}

/// Who asks `layer` for rounds, and where `config`'s `[schedule]` says
/// otherwise.
fn scheduled(config: &Config, layer: &Layer, here: &Here, findings: &mut Findings) {
    let schedule = config.schedule;
    let set = schedule.poll.is_some() || schedule.housekeep.is_some();
    let name = layer.name;
    match layer.rounds {
        Rounds::Scheduled => {}
        Rounds::Workflow => {
            if set {
                findings.notes.push(format!("[schedule] sets nothing on the {name} layer: a scheduled workflow runs herald poll and herald housekeep as often as its cron says."));
            }
        }
        Rounds::Platform(cadence) => {
            let by = cadence.by;
            findings.notes.push(format!(
                "On the {name} layer, {by} asks for rounds: a poll every {:#} and housekeeping every {:#}, as shipped.",
                cadence.poll, cadence.housekeep
            ));
            for (key, asked, shipped, what) in [
                ("schedule.poll", schedule.poll, cadence.poll, "a poll"),
                (
                    "schedule.housekeep",
                    schedule.housekeep,
                    cadence.housekeep,
                    "housekeeping",
                ),
            ] {
                if let Some(asked) = asked.filter(|asked| *asked != shipped) {
                    findings.notes.push(format!("{key} is {asked:#}, while on the {name} layer {by} asks for {what} every {shipped:#} as shipped; set the two alike."));
                }
            }
        }
        Rounds::Asked => {
            let often = if set {
                " as often as [schedule] says, which asks for nothing there itself"
            } else {
                ""
            };
            findings.notes.push(format!("On the {name} layer, a round runs only when a request holding the round secret asks for it at POST /herald/round/poll or /herald/round/housekeep; have a scheduler send them{often}."));
        }
    }
    let secrets = layer.round_secrets;
    if let Some(first) = secrets.first()
        && !secrets
            .iter()
            .any(|secret| here.round_secrets.contains(*secret))
    {
        findings.unless_set(
            here,
            gap(
                "rounds of polling and housekeeping",
                (*first).to_owned(),
                format!(
                    "on the {name} layer a round runs only when a request holding {} asks for it, and none is set",
                    listed(secrets, "or")
                ),
            ),
        );
    }
}

/// What each route could do through its destination reached as a bot,
/// where it is reached otherwise.
fn webhooked(config: &Config, notes: &mut Vec<String>) {
    for route in &config.routes {
        let profiles = config
            .destinations
            .get(&route.route.destination)
            .and_then(|destination| destination.profile(&route.options));
        let Some((reached, Some(bot))) = profiles else {
            continue;
        };
        let lacking = lacked(reached.features, bot.features);
        if !lacking.is_empty() {
            notes.push(format!(
                "The route {} cannot {} through \"{}\" as it is reached; a bot token could.",
                route.route.id.as_str(),
                listed(&lacking, "or"),
                route.route.destination.as_str()
            ));
        }
    }
}

/// What `reached` cannot do that `bot` can (ADR-0004).
fn lacked(reached: Features, bot: Features) -> Vec<&'static str> {
    [
        (bot.crosspost && !reached.crosspost, "crosspost"),
        (
            bot.threads == Threads::FromMessage && reached.threads != Threads::FromMessage,
            "open a thread from a message",
        ),
        (bot.pins != Pins::None && reached.pins == Pins::None, "pin"),
        (bot.edits && !reached.edits, "edit a post"),
        (
            bot.reads_back && !reached.reads_back,
            "read back its posts for markers",
        ),
        (bot.interactions && !reached.interactions, "take commands"),
    ]
    .into_iter()
    .filter_map(|(lacks, what)| lacks.then_some(what))
    .collect()
}

/// What `config`'s store asks that no run serves, and what serves the rest.
fn stored(config: &Config, layer: &Layer, findings: &mut Findings) {
    let kind = config.store.kind();
    if !layer.stores.contains(&kind) {
        findings.gaps.push(gap(
            &format!("keeping records in {kind}"),
            "store.kind".to_owned(),
            unserved(&layer.stores),
        ));
    }
    if let Some((key, why)) = config.store.withheld() {
        findings.gaps.push(gap(
            &format!("keeping records in {kind}"),
            format!("store.{key}"),
            why.to_owned(),
        ));
    }
    if let Some(note) = config.store.note() {
        findings.notes.push(note.to_owned());
    }
}

/// What the markers store, reading the routes' own posts, cannot serve.
fn marked(config: &Config, findings: &mut Findings) {
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

/// The routes that take a tag that moves with no versions pattern.
fn moving(config: &Config, notes: &mut Vec<String>) {
    for route in &config.routes {
        let moving = route.route.filter.sources.iter().find(|name| {
            config
                .sources
                .get(*name)
                .is_some_and(|source| source.moving_tags())
        });
        if let (Some(source), None) = (moving, &route.route.filter.versions) {
            notes.push(format!(
                "The route {} takes {}'s tags with no versions pattern, so a tag that moves, such as latest, is announced the first time it is pushed. Set versions = \"*.*.*\" on the route to leave such tags out.",
                route.route.id.as_str(),
                source.as_str()
            ));
        }
    }
}

/// What `config` asks for that `layer` cannot serve, and what it leaves to
/// chance, with `here`'s settings.
fn served(config: &Config, layer: &Layer, layers: &[Layer], here: &Here) -> Findings {
    let mut findings = Findings::default();
    unanswered(config, layer, layers, &mut findings.gaps);
    if layer.endpoint {
        hosted(config, layer, here, &mut findings);
    } else {
        unhosted(config, &mut findings.gaps);
    }
    if config.marker_key.is_none() {
        findings.notes.push(DERIVED_KEY.to_owned());
    }
    loosened(config, &mut findings.notes);
    stored(config, layer, &mut findings);
    moving(config, &mut findings.notes);
    webhooked(config, &mut findings.notes);
    scheduled(config, layer, here, &mut findings);
    for (index, route) in config.routes.iter().enumerate() {
        unpinned(config, index, route, &mut findings.gaps);
    }
    if config.store.reads_posts() {
        marked(config, &mut findings);
    }
    findings
}

/// What `config` asks for that `layer` cannot serve, each gap with the
/// first of `layers`, in ADR-0002's order, that serves it, and what it
/// leaves to chance, with `here`'s settings (ADR-0002).
#[must_use]
pub fn doctor(config: &Config, layer: &Layer, layers: &[Layer], here: &Here) -> Findings {
    let mut findings = served(config, layer, layers, here);
    for found in &mut findings.gaps {
        found.first_layer = layers
            .iter()
            .find(|candidate| {
                !served(config, candidate, layers, here)
                    .gaps
                    .iter()
                    .any(|other| other.key == found.key)
            })
            .map(|candidate| candidate.tier);
    }
    findings
}

#[cfg(test)]
mod tests;
