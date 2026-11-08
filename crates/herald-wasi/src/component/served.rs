use herald_compose::{Asked, Composed, Hosting, Left, told};
use herald_config::{Config, load};
use herald_flows::{Reply, refusal, uncomposed};

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

/// herald composed from the host's environment and files, or the reply to
/// `method` on `target` once its config loads where that needs no composed
/// herald.
async fn composed(
    vars: Vec<(String, String)>,
    method: &str,
    target: &str,
) -> Result<Result<Composed<WasiHost>, Reply>, String> {
    let path = named(&vars, "HERALD_CONFIG").unwrap_or_else(|| CONFIG.to_owned());
    let hosting = named(&vars, "HERALD_HOST").unwrap_or_else(|| HOST.to_owned());
    let hosting =
        Hosting::of(&hosting).ok_or_else(|| format!("{hosting} is no host herald knows"))?;
    let config = configured(&path)?;
    if let Some(reply) = uncomposed(method, target, &config.server.base) {
        return Ok(Err(reply));
    }
    Composed::new(
        WasiHost::new(vars),
        config,
        hosting.stores(),
        hosting.name(),
    )
    .await
    .map(Ok)
}

pub(super) fn said(lines: impl IntoIterator<Item = String>) {
    for line in lines {
        eprintln!("{line}");
    }
}

/// herald composed for one request.
pub(super) struct Started {
    composed: Composed<WasiHost>,
}

impl Started {
    /// herald composed over `vars`, the environment, for `method` on
    /// `target`, or the reply given without composing it: whether it is up,
    /// or the refusal that says it could not start, the reason logged.
    pub(super) async fn new(
        vars: Vec<(String, String)>,
        method: &str,
        target: &str,
    ) -> Result<Self, Reply> {
        match composed(vars, method, target).await {
            Ok(Ok(composed)) => Ok(Self { composed }),
            Ok(Err(reply)) => Err(reply),
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
        let answer = self.composed.reached(asked).await;
        said(answer.refused.as_deref().map(told::refused));
        said(answer.said);
        (answer.reply, answer.left)
    }

    /// The work a request left, done.
    pub(super) async fn worked(&self, left: Left) {
        said(told::worked(&self.composed.work(left).await));
    }
}
