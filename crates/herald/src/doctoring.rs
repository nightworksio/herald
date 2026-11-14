//! `herald doctor`: what a config asks for that a layer cannot serve, read
//! offline or, with `--online`, against every secret and each destination
//! (ADR-0002), printed as lines or as JSON.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use clap::{Args, ValueEnum};
use herald_compose::{Composed, Host, Hosting, ServingOn, Through, told};
use herald_config::{Config, Uses};
use herald_crypto::RingSigner;
use herald_environment::EnvSecrets;
use herald_flows::{
    Announced, EGRESS_GUARDED, Findings, Gap, Here, Outcome, Tier, doctor, egress_guarded, keys,
    online, trial,
};
use herald_ports::{Keyring, Secrets as _, Store};
use schemars::JsonSchema;
use serde::Serialize;

use super::cli::{Layer, Running};
use super::output::say;
use super::serving::hosting;
use super::stores::Native;
use super::{Connect, DONE, MISTAKEN, STOPPED, place};

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
    }
}

/// The layer the command `run` runs on, where it warns of the gaps it
/// leaves (ADR-0002).
pub(crate) fn warning(run: &Running, vars: &BTreeMap<String, String>) -> Option<Hosting> {
    match run {
        Running::Server { .. } => Some(hosting(vars)),
        Running::Lambda => Some(Hosting::Lambda),
        Running::Followers(_) => None,
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
    if asked.online {
        let native = Native::new(vars, connect);
        match Composed::dry(native, config.clone(), Uses::Announces, place(vars)).await {
            Ok(composed) => {
                findings.extend(online(composed.connected()).await);
                if asked.post {
                    let tried = composed.run(Posting).await;
                    findings.extend(posted(config, &tried));
                }
            }
            Err(why) => findings
                .notes
                .push(format!("No destination is asked, as {why}.")),
        }
    }
    findings
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
fn printed(path: &str, asked: DoctorArgs, findings: &Findings, out: &mut dyn Write) -> u8 {
    match asked.format {
        Format::Json => {
            let report = Report {
                layer: asked.layer.0.name(),
                online: asked.online,
                gaps: findings
                    .gaps
                    .iter()
                    .map(|gap| Gapped {
                        feature: &gap.feature,
                        key: &gap.key,
                        why: &gap.why,
                        first_layer: gap.first_layer.map(FirstLayer::from),
                    })
                    .collect(),
                notes: &findings.notes,
                mistakes: &findings.mistakes,
            };
            say(out, &serde_json::to_string(&report).unwrap_or_default());
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
    printed(path, asked, &findings, out)
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
        printed("", asked, &findings, out)
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
