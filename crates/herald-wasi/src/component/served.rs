use herald_compose::{Artefact, Composed, HostRequest, Hosting, Left, Log, Told, told};
use herald_config::{Config, Uses, load};
use herald_core::status;
use herald_environment::SystemClock;
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
    load(path, &text, &read, &herald_compose::kinds()).map_err(|errors| errors.to_string())
}

/// herald composed from the host's environment and files, or the reply to
/// `method` on `target` once its config loads where that needs no composed
/// herald.
async fn composed(
    vars: Vec<(String, String)>,
    method: &str,
    target: &str,
) -> Result<Result<Composed<'static, WasiHost>, Reply>, String> {
    let path = named(&vars, "HERALD_CONFIG").unwrap_or_else(|| CONFIG.to_owned());
    let hosting = named(&vars, "HERALD_HOST").unwrap_or_else(|| HOST.to_owned());
    let hosting = Hosting::running(&hosting, Artefact::Component)?;
    let config = configured(&path)?;
    if let Some(reply) = uncomposed(method, target, &config.server.base) {
        return Ok(Err(reply));
    }
    Composed::new(
        WasiHost::new(vars),
        config,
        hosting,
        Uses::Serves,
        hosting.name(),
    )
    .await
    .map(Ok)
}

#[expect(
    clippy::print_stderr,
    reason = "the host keeps what a component writes to standard error as its log"
)]
pub(super) fn log_lines(log: &Log, told: impl IntoIterator<Item = Told>) {
    for line in log.lines(told) {
        eprintln!("{line}");
    }
}

/// herald composed for one request.
pub(super) struct Started {
    composed: Composed<'static, WasiHost>,
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
        let unconfigured = Log::unconfigured(
            |name| {
                vars.iter()
                    .find(|(key, value)| key == name && !value.is_empty())
                    .map(|(_, value)| value.as_str())
            },
            SystemClock,
        );
        match composed(vars.clone(), method, target).await {
            Ok(Ok(composed)) => Ok(Self { composed }),
            Ok(Err(reply)) => Err(reply),
            Err(error) => {
                let refused = refusal(
                    status::SERVER_ERROR,
                    format!("herald could not start: {error}"),
                );
                log_lines(&unconfigured, refused.refused.as_deref().map(told::refused));
                Err(refused.reply)
            }
        }
    }

    /// What its log writes.
    pub(super) fn log(&self) -> &Log {
        self.composed.log()
    }

    /// The most of a body that is read.
    pub(super) fn body(&self) -> usize {
        self.composed.config().server.body
    }

    /// The answer to `asked`, and the work it leaves.
    pub(super) async fn answered(&self, asked: &HostRequest) -> (Reply, Option<Left>) {
        let answer = self.composed.reached(asked).await;
        let log = self.composed.log();
        log_lines(log, answer.refused.as_deref().map(told::refused));
        log_lines(log, answer.said);
        (answer.reply, answer.left)
    }

    /// The work a request left, done.
    pub(super) async fn worked(&self, left: Left) {
        log_lines(
            self.composed.log(),
            told::worked(&self.composed.work(left).await),
        );
    }
}
