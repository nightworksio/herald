use herald_compose::{Asked, Composed, Hosting, Left, ROUND_SECRET, told};
use herald_config::{Config, load};
use herald_flows::{Reply, refusal};
use herald_ports::Secret;

use super::host::WasiHost;

/// The file the config is read from where `HERALD_CONFIG` is unset.
const CONFIG: &str = "herald.toml";
/// The host named where `HERALD_HOST` is unset.
const HOST: &str = "wasi";

fn named(vars: &[(String, String)], name: &str) -> Option<String> {
    vars.iter()
        .find(|(key, value)| key == name && !value.is_empty())
        .map(|(_, value)| value.clone())
}

/// The config at `path`, and the files it names.
fn configured(path: &str) -> Result<Config, String> {
    let read =
        |name: &str| std::fs::read_to_string(name).map_err(|error| format!("{name}: {error}"));
    let text = read(path)?;
    load(path, &text, &read).map_err(|errors| errors.to_string())
}

/// herald composed from the host's environment and files.
async fn composed(vars: Vec<(String, String)>) -> Result<Composed<WasiHost>, String> {
    let path = named(&vars, "HERALD_CONFIG").unwrap_or_else(|| CONFIG.to_owned());
    let hosting = named(&vars, "HERALD_HOST").unwrap_or_else(|| HOST.to_owned());
    let hosting =
        Hosting::of(&hosting).ok_or_else(|| format!("{hosting} is no host herald knows"))?;
    let config = configured(&path)?;
    Composed::new(
        WasiHost::new(vars),
        config,
        hosting.stores(),
        hosting.name(),
    )
    .await
}

pub(super) fn said(lines: impl IntoIterator<Item = String>) {
    for line in lines {
        eprintln!("{line}");
    }
}

/// herald composed for one request, with the secret its rounds are asked
/// with.
pub(super) struct Started {
    composed: Composed<WasiHost>,
    secret: Option<Secret>,
}

impl Started {
    /// herald composed over `vars`, the environment, or the reply that says
    /// it could not start, the reason logged.
    pub(super) async fn new(vars: Vec<(String, String)>) -> Result<Self, Reply> {
        let secret = named(&vars, ROUND_SECRET).map(|value| Secret::new(ROUND_SECRET, value));
        match composed(vars).await {
            Ok(composed) => Ok(Self { composed, secret }),
            Err(error) => {
                let refused = refusal(500, format!("herald could not start: {error}"));
                said(refused.refused.as_deref().map(told::refused));
                Err(refused.reply)
            }
        }
    }

    /// The most of a body that is read.
    pub(super) fn body(&self) -> usize {
        self.composed.config().server.body
    }

    /// The answer to `asked`, and the work it leaves.
    pub(super) async fn answered(&self, asked: &Asked) -> (Reply, Option<Left>) {
        let reached = self.composed.reached(asked, self.secret.as_ref()).await;
        said(reached.said);
        (reached.reply, reached.left)
    }

    /// The work a request left, done.
    pub(super) async fn worked(&self, left: Left) {
        said(told::worked(&self.composed.work(left).await));
    }
}
