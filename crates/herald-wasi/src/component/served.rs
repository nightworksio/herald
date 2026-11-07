use herald_compose::{Composed, Hosting, ROUND_SECRET, Round, asked_query, told};
use herald_config::load;
use herald_flows::{Admission, Inbound, Job, Reply};
use herald_ports::Secret;

use super::host::WasiHost;

/// The file the config is read from where `HERALD_CONFIG` is unset.
const CONFIG: &str = "herald.toml";
/// The host named where `HERALD_HOST` is unset.
const HOST: &str = "wasi";

const PLAIN: &str = "text/plain; charset=utf-8";

fn plain(status: u16, text: &str) -> Reply {
    Reply {
        status,
        kind: PLAIN,
        body: text.as_bytes().to_vec(),
    }
}

/// What a request asked, read off the wire.
pub(super) struct Asked {
    pub(super) method: String,
    pub(super) target: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
}

/// herald composed from the host's environment and files.
async fn composed(vars: Vec<(String, String)>) -> Result<Composed<WasiHost>, String> {
    let named = |name: &str| {
        vars.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let path = named("HERALD_CONFIG").unwrap_or_else(|| CONFIG.to_owned());
    let hosting = named("HERALD_HOST").unwrap_or_else(|| HOST.to_owned());
    let hosting =
        Hosting::of(&hosting).ok_or_else(|| format!("{hosting} is no host herald knows"))?;
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
    let read =
        |name: &str| std::fs::read_to_string(name).map_err(|error| format!("{name}: {error}"));
    let config = load(&path, &text, &read).map_err(|errors| errors.to_string())?;
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
    let composed = match composed(vars.clone()).await {
        Ok(composed) => composed,
        Err(error) => {
            eprintln!("herald could not start: {error}");
            return (plain(500, "herald could not start"), None);
        }
    };
    let (path, query) = match asked.target.split_once('?') {
        Some((path, query)) => (path, asked_query(Some(query.to_owned()))),
        None => (asked.target.as_str(), None),
    };
    let header = |name: &str| {
        asked
            .headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };
    if let Some(round) = Round::named(&asked.method, path) {
        let secret = vars
            .iter()
            .find(|(key, value)| key == ROUND_SECRET && !value.is_empty())
            .map(|(_, value)| Secret::new(ROUND_SECRET, value.clone()));
        if !Round::allowed(header("authorization"), secret.as_ref()) {
            return (plain(401, "a round needs its secret"), None);
        }
        match round {
            Round::Poll => said(composed.poll().await.iter().flat_map(told::polled)),
            Round::Housekeep => said(told::kept(&composed.housekeep().await)),
        }
        return (plain(200, "the round ran"), None);
    }
    let headers: Vec<(&str, &str)> = asked
        .headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let inbound = Inbound {
        method: &asked.method,
        path,
        query: query.as_deref(),
        headers: &headers,
        body: &asked.body,
    };
    let answered = composed.answer(&inbound, Admission::Once).await;
    let reply = answered.reply;
    (reply, answered.job.map(|job| (composed, job)))
}

/// The work a request left, done.
pub(super) async fn worked(composed: &Composed<WasiHost>, job: Job) {
    said(told::worked(&composed.work(job).await));
}
