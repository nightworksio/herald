use herald_config::{Config, Destination, RouteConfig, RouteOptions, Store};
use herald_core::Pinning;
use herald_dest_discord::profile;

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
}

/// What a derived marker key costs (ADR-0011).
const DERIVED_KEY: &str = "Each post's markers are keyed by its destination's credential, as no [markers] key is set. Rotating that credential forgets every earlier marker, so an announcement made before the rotation can be posted again on a store that has only markers. Set [markers] key = { secret = \"HERALD_MARKER_SECRET\" } to keep them.";
/// What the markers store cannot hold apart (ADR-0005).
const UNGUARDED: &str = "The markers store has no compare-and-set, so two runs at once can both post an announcement; run one herald at a time, or choose a store that keeps records.";

/// What the Actions cache keeps apart and drops (ADR-0005).
const SCOPED: &str = "The Actions cache keeps records apart for each ref a run is on, and drops what no run reads for a week: a release run on its tag does not see what a run on the default branch wrote. Run housekeeping and polling on one ref, or choose a store that keeps records across refs.";
/// What repository variables hold at most (ADR-0005).
const CROWDED: &str = "Each record is a repository variable: a repository holds at most 500, and a run is handed at most 256 KB of them, so many records can crowd out the repository's own variables.";

fn gap(feature: &str, key: String, why: String) -> Gap {
    Gap {
        feature: feature.to_owned(),
        key,
        why,
    }
}

fn unkept(config: &Config, index: usize, route: &RouteConfig, gaps: &mut Vec<Gap>) {
    let bot = config
        .destinations
        .get(&route.route.destination)
        .is_some_and(|Destination::Discord(options)| options.token.is_some());
    let RouteOptions::Discord(placed) = &route.options;
    let key = |field: &str| format!("routes[{index}].{field}");
    if route.route.delivery.pin == Some(Pinning::Store) {
        gaps.push(gap(
            "a pin the store remembers",
            key("pin"),
            "the markers store keeps no pins; set pin = \"markers\"".to_owned(),
        ));
    }
    if !profile(bot, placed.forum).features.reads_back {
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

/// What `config` asks for that a herald keeping records in the store kinds
/// `stores` cannot serve, and what it leaves to chance (ADR-0002).
#[must_use]
pub fn doctor(config: &Config, stores: &[&str]) -> Findings {
    let mut findings = Findings::default();
    if config.marker_key.is_none() {
        findings.notes.push(DERIVED_KEY.to_owned());
    }
    let kind = config.store.kind();
    if !stores.contains(&kind) {
        findings.gaps.push(gap(
            &format!("keeping records in {kind}"),
            "store.kind".to_owned(),
            unserved(stores),
        ));
    }
    match config.store {
        Store::ActionsCache => findings.notes.push(SCOPED.to_owned()),
        Store::RepoVariables(_) => findings.notes.push(CROWDED.to_owned()),
        _ => {}
    }
    if matches!(config.store, Store::Markers) {
        for (index, route) in config.routes.iter().enumerate() {
            unkept(config, index, route, &mut findings.gaps);
        }
        for name in config.sources.keys() {
            findings.gaps.push(gap(
                &format!("polling {}", name.as_str()),
                format!("sources.{}", name.as_str()),
                "the markers store keeps no cursors; `herald poll` needs a store that does"
                    .to_owned(),
            ));
        }
        findings.notes.push(UNGUARDED.to_owned());
    }
    findings
}
