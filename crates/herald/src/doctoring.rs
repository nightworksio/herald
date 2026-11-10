//! `herald doctor`: what a config asks for that a layer cannot serve, read
//! offline or, with `--online`, against every secret and each destination
//! (ADR-0002), printed as lines or as JSON.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use clap::{Args, ValueEnum};
use herald_config::Config;
use herald_crypto::RingSigner;
use herald_environment::EnvSecrets;
use herald_flows::{
    EGRESS_GUARDED, Findings, Gap, Here, Tier, doctor, egress_guarded, keys, online, prepare,
};
use herald_ports::Secrets as _;
use schemars::JsonSchema;
use serde::Serialize;

use super::cli::{Layer, Serve};
use super::output::say;
use super::serving::AZURE_PORT;
use super::{Connect, DONE, MISTAKEN, STOPPED};

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
pub(crate) struct Asked {
    /// The layer to check the config for.
    #[arg(long, value_enum, default_value_t = Layer::Actions)]
    pub(crate) layer: Layer,
    /// How to print what it finds.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub(crate) format: Format,
    /// Also read every secret, and ask each destination whether it takes
    /// its credential and grants what each route needs, posting nothing.
    #[arg(long)]
    pub(crate) online: bool,
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
    let named: BTreeSet<&str> = Layer::every()
        .iter()
        .flat_map(|layer| layer.round_secrets.iter().copied())
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

/// The layer a command serving as `serve` runs on, where it warns of the
/// gaps it leaves (ADR-0002).
pub(crate) fn warning(serve: &Serve, vars: &BTreeMap<String, String>) -> Option<Layer> {
    match serve {
        Serve::Server { .. } if vars.contains_key(AZURE_PORT) => Some(Layer::Azure),
        Serve::Server { .. } => Some(Layer::Server),
        Serve::Lambda => Some(Layer::Lambda),
        Serve::Followers(_) => None,
        Serve::Announce { .. } | Serve::Poll | Serve::Housekeep => Some(Layer::Actions),
    }
}

/// The gaps `config` leaves on `layer`, as every command warns of them.
pub(crate) async fn gaps(
    config: &Config,
    layer: Layer,
    vars: &BTreeMap<String, String>,
) -> Vec<Gap> {
    let secrets = EnvSecrets::from_vars(vars.clone());
    let here = here(vars, &secrets, false).await;
    doctor(config, layer.served(), &Layer::every(), &here).gaps
}

/// What doctor finds in `config` for `asked`.
async fn findings(
    config: &Config,
    asked: Asked,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
) -> Findings {
    let secrets = EnvSecrets::from_vars(vars.clone());
    let here = here(vars, &secrets, asked.online).await;
    let mut findings = doctor(config, asked.layer.served(), &Layer::every(), &here);
    findings.extend(herald_flows::secrets(config, &secrets, &here).await);
    findings.extend(keys(config, &secrets, &RingSigner).await);
    if asked.online {
        let connected = match prepare(config, &secrets, &RingSigner).await {
            Ok(prepared) => prepared.connect(|base| connect.reach(base)),
            Err(error) => Err(error.to_string()),
        };
        match connected {
            Ok(connected) => findings.extend(online(&connected).await),
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
fn printed(path: &str, asked: Asked, findings: &Findings, out: &mut dyn Write) -> u8 {
    match asked.format {
        Format::Json => {
            let report = Report {
                layer: asked.layer.served().name,
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
    asked: Asked,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> u8 {
    let findings = findings(config, asked, vars, connect).await;
    printed(path, asked, &findings, out)
}

/// `herald doctor` on a config that could not be read, as `message` says,
/// printed as JSON where `asked` says so; `None` where it prints lines.
pub(crate) fn unread(asked: Asked, message: &str, out: &mut dyn Write) -> Option<u8> {
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
