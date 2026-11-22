//! `herald doctor`: what a config asks for that a layer cannot serve, read
//! offline or, with `--online`, against every secret and each destination
//! (ADR-0002), printed as lines or as JSON.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use clap::{Args, ValueEnum};
use herald_compose::{Composed, Host, Hosting, ServingOn, Through, told};
use herald_config::{Config, Rotation, Uses};
use herald_crypto::RingSigner;
use herald_environment::{EnvSecrets, SystemClock};
use herald_flows::{
    Announced, EGRESS_GUARDED, Federation, Findings, Gap, Here, Outcome, Tier, doctor,
    egress_guarded, handed_over, kept_tokens, keys, online, recalls, shared, trial,
};
use herald_ports::{Clock as _, Keyring, Secrets as _, Store};
use schemars::JsonSchema;
use serde::Serialize;

use super::cli::{Layer, Running};
use super::output::{say, say_json};
use super::serving::hosting;
use super::stores::{Native, composed};
use super::{Connect, DONE, MISTAKEN, STOPPED, place};

mod federation;

/// How doctor prints its findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    /// One line per finding.
    Text,
    /// One JSON document, as `herald-doctor.schema.json` describes it.
    Json,
}

/// How a doctor run is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Args)]
pub(crate) struct DoctorArgs {
    /// The layer to check the config for.
    #[arg(long, value_enum, default_value_t = Layer(Hosting::Actions))]
    pub(crate) layer: Layer,
    /// How to print what it finds.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub(crate) format: Format,
    /// Also read every secret, and ask each destination whether it takes
    /// its credential and grants what each route needs, posting nothing.
    #[arg(long)]
    pub(crate) online: bool,
    /// Also post one test message to each route's destination, kept apart
    /// from every record a real announcement reads.
    #[arg(long, requires = "online")]
    pub(crate) post: bool,
}

/// The test messages `herald doctor --post` posts, kept in a store of their
/// own.
struct Posting;

impl<H: Host> Through<H> for Posting {
    type Done = Vec<Announced>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        run: &str,
    ) -> Vec<Announced> {
        trial(serving.config, serving.connected, serving.ports, run).await
    }
}

/// Whether the store keeps the installation tokens of each GitHub App that
/// keeps them, minting one for each.
struct Keeping;

impl<H: Host> Through<H> for Keeping {
    type Done = Findings;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Findings {
        let ports = serving.ports;
        kept_tokens(
            serving.connected,
            (ports.signer, ports.store.tokens(), ports.clock.now()),
            serving.config.store.kind(),
        )
        .await
    }
}

/// Whether each peer whose rotation is signed handed over to keys
/// herald.toml does not pin, and which instances' tombstones fill a herald
/// source's share, as the store keeps them (ADR-0049).
struct Rotating;

impl<H: Host> Through<H> for Rotating {
    type Done = Findings;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Findings {
        let mut findings = handed_over(serving.config, &serving.ports.store.published()).await;
        findings.extend(shared(serving.config, &serving.ports.store.cursors()).await);
        findings.extend(
            recalls(
                serving.config,
                (
                    &serving.ports.store.cursors(),
                    &serving.ports.store.recalls(),
                    &serving.ports.store.ledger(),
                ),
            )
            .await,
        );
        findings
    }
}

/// What each route's test message did, as findings: a note where it was
/// posted, and a gap where it was not.
fn posted(config: &Config, tried: &[Announced]) -> Findings {
    let mut findings = Findings::default();
    for announced in tried {
        let id = announced.route.as_str();
        let index = config
            .routes
            .iter()
            .position(|route| route.route.id == announced.route)
            .unwrap_or_default();
        if let Outcome::Sent { steps } = &announced.outcome {
            findings.notes.push(format!(
                "A test message was posted on the route {id} ({}).",
                told::steps(steps)
            ));
        } else {
            findings.gaps.push(Gap {
                feature: format!("posting a test message on the route {id}"),
                key: format!("routes[{index}]"),
                why: told::report(announced),
                first_layer: None,
            });
        }
    }
    findings
}

/// The first layer that serves a gap, as the JSON names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum FirstLayer {
    /// The CLI in a workflow or any CI.
    Actions,
    /// A serverless driver's endpoint.
    Serverless,
    /// A process of its own that holds its endpoint open.
    Container,
}

impl From<Tier> for FirstLayer {
    fn from(tier: Tier) -> Self {
        match tier {
            Tier::Actions => Self::Actions,
            Tier::Serverless => Self::Serverless,
            Tier::Container => Self::Container,
        }
    }
}

/// A feature the config asks for that the layer cannot serve.
#[derive(Debug, Serialize, JsonSchema)]
struct Gapped<'a> {
    /// What the config asks for.
    feature: &'a str,
    /// The config key that asks for it.
    key: &'a str,
    /// Why it cannot be served, and what would serve it.
    why: &'a str,
    /// The first layer that serves it; null where none does until the
    /// config or the platform changes.
    #[schemars(required)]
    first_layer: Option<FirstLayer>,
}

/// What `herald doctor --format json` prints.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(title = "herald doctor")]
struct Report<'a> {
    /// The layer the config is checked for, as `--layer` names it.
    layer: &'a str,
    /// Whether every secret was read and each destination asked.
    online: bool,
    /// The features that cannot be served.
    gaps: Vec<Gapped<'a>>,
    /// What holds, at a cost worth knowing.
    notes: &'a [String],
    /// What no run could get past.
    mistakes: &'a [String],
    /// The instance among other heralds; null where the config takes no
    /// part among them.
    #[schemars(required)]
    federation: Option<federation::FederationJson<'a>>,
}

/// `gap` as the JSON names it.
fn gapped_json(gap: &Gap) -> Gapped<'_> {
    Gapped {
        feature: &gap.feature,
        key: &gap.key,
        why: &gap.why,
        first_layer: gap.first_layer.map(FirstLayer::from),
    }
}

/// The JSON Schema of what `herald doctor --format json` prints, as
/// committed in `herald-doctor.schema.json`.
#[must_use]
pub fn schema() -> String {
    let schema = schemars::schema_for!(Report<'static>);
    format!(
        "{}\n",
        serde_json::to_string_pretty(&schema).unwrap_or_default()
    )
}

/// The settings doctor finds where it runs: the egress guard and every
/// layer's round secrets, read through `secrets`.
pub(crate) async fn here(
    vars: &BTreeMap<String, String>,
    secrets: &EnvSecrets,
    online: bool,
) -> Here {
    let named: BTreeSet<&str> = Hosting::ALL
        .into_iter()
        .flat_map(Hosting::round_secrets)
        .copied()
        .collect();
    let mut round_secrets = BTreeSet::new();
    for name in named {
        if secrets.secret(name).await.is_ok() {
            round_secrets.insert(name.to_owned());
        }
    }
    Here {
        online,
        guarded: egress_guarded(vars.get(EGRESS_GUARDED).map(String::as_str)),
        round_secrets,
        models: herald_compose::ASKS_MODELS,
    }
}

/// The layer the command `run` runs on, where it warns of the gaps it
/// leaves (ADR-0002).
pub(crate) fn warning(run: &Running, vars: &BTreeMap<String, String>) -> Option<Hosting> {
    match run {
        Running::Server { .. } => Some(hosting(vars)),
        Running::Lambda => Some(Hosting::Lambda),
        Running::Followers(_) | Running::Withdraw { .. } => None,
        Running::Announce { .. } | Running::Poll | Running::Housekeep => Some(Hosting::Actions),
    }
}

/// The gaps `config` leaves on `layer`, as every command warns of them.
pub(crate) async fn gaps(
    config: &Config,
    hosting: Hosting,
    vars: &BTreeMap<String, String>,
) -> Vec<Gap> {
    let secrets = EnvSecrets::from_vars(vars.clone());
    let here = here(vars, &secrets, false).await;
    doctor(config, &hosting.layer(), &Hosting::layers(), &here).gaps
}

/// What doctor finds in `config` for `asked`.
async fn findings(
    config: &Config,
    asked: DoctorArgs,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
) -> Findings {
    let secrets = EnvSecrets::from_vars(vars.clone());
    let here = here(vars, &secrets, asked.online).await;
    let mut findings = doctor(config, &asked.layer.0.layer(), &Hosting::layers(), &here);
    findings.extend(herald_flows::secrets(config, &secrets, &here).await);
    findings.extend(keys(config, &secrets, &RingSigner::default()).await);
    findings.extend(herald_flows::granted(config, SystemClock.now()));
    if asked.online {
        let native = Native::new(vars, connect);
        match Composed::dry(native, config.clone(), Uses::Announces, place(vars)).await {
            Ok(composed) => {
                findings.extend(
                    online(
                        composed.connected(),
                        (&RingSigner::default(), SystemClock.now()),
                    )
                    .await,
                );
                findings.extend(
                    herald_flows::codeowners(config, composed.connected(), SystemClock.now()).await,
                );
                if asked.post {
                    let tried = composed.run(Posting).await;
                    findings.extend(posted(config, &tried));
                }
            }
            Err(why) => findings
                .notes
                .push(format!("No destination is asked, as {why}.")),
        }
        if config.github_apps.values().any(|app| app.store_token) {
            findings.extend(keeping(config, asked.layer.0, vars, connect).await);
        }
        if config
            .peers
            .values()
            .any(|peer| peer.rotation == Rotation::Signed)
        {
            findings.extend(rotating(config, asked.layer.0, vars, connect).await);
        }
    }
    findings
}

/// Whether the store `config` keeps records in on `hosting` keeps the
/// installation tokens of each GitHub App that keeps them.
async fn keeping(
    config: &Config,
    hosting: Hosting,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
) -> Findings {
    match composed(config, vars, connect, hosting, Uses::Polls, place(vars)).await {
        Ok(composed) => composed.run(Keeping).await,
        Err(why) => Findings {
            notes: vec![format!("No installation token is kept, as {why}.")],
            ..Findings::default()
        },
    }
}

/// Whether each peer whose rotation is signed handed over to keys
/// herald.toml does not pin, in the store `config` keeps records in on
/// `hosting`.
async fn rotating(
    config: &Config,
    hosting: Hosting,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
) -> Findings {
    match composed(config, vars, connect, hosting, Uses::Polls, place(vars)).await {
        Ok(composed) => composed.run(Rotating).await,
        Err(why) => Findings {
            notes: vec![format!("No peer's key set is read, as {why}.")],
            ..Findings::default()
        },
    }
}

/// The exit code `findings` call for (ADR-0002).
fn code(findings: &Findings) -> u8 {
    if !findings.mistakes.is_empty() {
        MISTAKEN
    } else if findings.gaps.is_empty() {
        DONE
    } else {
        STOPPED
    }
}

/// `gap` as one line.
pub(crate) fn gapped(gap: &Gap) -> String {
    let first = gap.first_layer.map_or_else(String::new, |tier| {
        format!(" The {} layer is the first that serves it.", tier.name())
    });
    format!(
        "{}: {} cannot be served: {}.{first}",
        gap.key, gap.feature, gap.why
    )
}

/// `findings` for `asked`, printed as `asked` says to `out`.
fn printed(
    path: &str,
    asked: DoctorArgs,
    (findings, shown): (&Findings, Option<&Federation>),
    out: &mut dyn Write,
) -> u8 {
    match asked.format {
        Format::Json => {
            let report = Report {
                layer: asked.layer.0.name(),
                online: asked.online,
                gaps: findings.gaps.iter().map(gapped_json).collect(),
                notes: &findings.notes,
                mistakes: &findings.mistakes,
                federation: shown.map(federation::FederationJson::from),
            };
            say_json(out, &serde_json::to_string(&report).unwrap_or_default());
        }
        Format::Text => {
            say(
                out,
                &format!("{path} reads, its names resolve and its templates render."),
            );
            for line in findings
                .notes
                .iter()
                .cloned()
                .chain(findings.gaps.iter().map(gapped))
                .chain(findings.mistakes.iter().cloned())
                .chain(shown.into_iter().flat_map(federation::lines))
            {
                say(out, &line);
            }
        }
    }
    code(findings)
}

/// `herald doctor` on `config`, read from `path`, as `asked`.
pub(crate) async fn doctoring(
    config: &Config,
    path: &str,
    asked: DoctorArgs,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> u8 {
    let findings = findings(config, asked, vars, connect).await;
    let secrets = EnvSecrets::from_vars(vars.clone());
    let mut shown = herald_flows::federation(
        config,
        &asked.layer.0.layer(),
        (&secrets, &RingSigner::default()),
        &findings.gaps,
    )
    .await;
    if asked.online
        && let Some(shown) = &mut shown
    {
        federation::reached(config, connect, shown).await;
    }
    printed(path, asked, (&findings, shown.as_ref()), out)
}

/// `herald doctor` on a config that could not be read, as `message` says,
/// printed as JSON where `asked` says so; `None` where it prints lines.
pub(crate) fn unread(asked: DoctorArgs, message: &str, out: &mut dyn Write) -> Option<u8> {
    (asked.format == Format::Json).then(|| {
        let mistakes: Vec<String> = message.lines().map(ToOwned::to_owned).collect();
        let findings = Findings {
            mistakes,
            ..Findings::default()
        };
        printed("", asked, (&findings, None), out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_first_layer_is_named_in_json_as_its_tier_is() {
        for tier in [Tier::Actions, Tier::Serverless, Tier::Container] {
            assert_eq!(
                serde_json::to_value(FirstLayer::from(tier)).unwrap(),
                tier.name()
            );
        }
    }
}
