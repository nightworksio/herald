use herald_compose::{Asked, Composed, Hosting, ROUND_SECRET, plain, told};
use herald_config::{Config, load};
use herald_flows::{Job, Reply};
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

fn said(lines: impl IntoIterator<Item = String>) {
    for line in lines {
        eprintln!("{line}");
    }
}

/// The answer to `asked` over `vars`, the environment, and the work it leaves
/// with what it runs on.
pub(super) async fn answered(
    asked: &Asked,
    vars: Vec<(String, String)>,
) -> (Reply, Option<(Composed<WasiHost>, Job)>) {
    let secret = named(&vars, ROUND_SECRET).map(|value| Secret::new(ROUND_SECRET, value));
    let composed = match composed(vars).await {
        Ok(composed) => composed,
        Err(error) => {
            eprintln!("herald could not start: {error}");
            return (plain(500, "herald could not start"), None);
        }
    };
    let reached = composed.reached(asked, secret.as_ref()).await;
    said(reached.said);
    (reached.reply, reached.job.map(|job| (composed, job)))
}

/// The work a request left, done.
pub(super) async fn worked(composed: &Composed<WasiHost>, job: Job) {
    said(told::worked(&composed.work(job).await));
}
