//! `herald server`: the config's store and keys, served over HTTP until
//! the run is stopped (ADR-0014).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::io::Write;

use herald_config::Config;
use herald_flows::{
    Connected, FollowError, Followed, Interacted, Keys, Ports, Serving, Webhooks, Worked, webhooks,
};
use herald_native::{EnvSecrets, NativeSockets, RingSigner, RingVerifier, SystemClock};
use herald_ports::{Keyring, PUBLIC, Store as Keeps};
use herald_records::MemoryKeyring;
use herald_server::{Capped, Heard, Limits, READ, serve};

use super::output::{chored, polled, report, say, why};
use super::stores::{Commands, reached};
use super::{Connect, Connection, DONE, LISTEN, Served, worded};

/// A server's work, over whichever store the config names, and the keys
/// it checks activities with.
struct Listening<'a, F> {
    config: &'a Config,
    connected: &'a Connected<Connection>,
    webhooks: &'a Webhooks,
    public: &'a Capped<Connection>,
    sockets: NativeSockets,
    listener: tokio::net::TcpListener,
    out: RefCell<&'a mut dyn Write>,
    stop: F,
}

impl<F: Future<Output = ()>> Served for Listening<'_, F> {
    type Done = u8;

    async fn over<S: Keeps + Keyring>(self, store: &S) -> u8 {
        let server = &self.config.server;
        if server.keys_in_store() {
            self.keyed(store, store).await
        } else {
            let memory = MemoryKeyring::new(server.keys_most);
            self.keyed(store, &memory).await
        }
    }
}

impl<F: Future<Output = ()>> Listening<'_, F> {
    /// The server, over `store`, checking activities with the keys in
    /// `keyring`.
    async fn keyed<S: Keeps, K: Keyring>(self, store: &S, keyring: &K) -> u8 {
        let server = &self.config.server;
        let ports = Ports {
            store,
            clock: &SystemClock,
            signer: &RingSigner,
        };
        let keys = Keys {
            keyring,
            refreshed: server.keys_refreshed,
        };
        let serving = Serving {
            config: self.config,
            connected: self.connected,
            ports: &ports,
            verifier: &RingVerifier,
            public: self.public,
            keys: &keys,
            webhooks: self.webhooks,
        };
        let period = |every: Option<jiff::SignedDuration>| {
            every.and_then(|every| std::time::Duration::try_from(every).ok())
        };
        let limits = Limits {
            jobs: server.jobs,
            connections: server.connections,
            body: server.body,
            poll: period(self.config.schedule.poll),
            housekeep: period(self.config.schedule.housekeep),
            keys: period(Some(server.keys_refreshed)).unwrap_or_default(),
            read: READ,
        };
        let out = &self.out;
        let heard = |heard: Heard| {
            let mut out = out.borrow_mut();
            for line in said(heard) {
                say(&mut **out, &line);
            }
        };
        serve(
            &serving,
            self.listener,
            &self.sockets,
            limits,
            &heard,
            self.stop,
        )
        .await;
        DONE
    }
}

/// The domains the sessions herald holds open connections under: Discord's
/// gateway and Slack's Socket Mode (ADR-0016).
const SESSIONS: [&str; 2] = ["discord.gg", "slack.com"];

/// What a server says of what it heard.
pub(crate) fn said(heard: Heard) -> Vec<String> {
    match heard {
        Heard::Listening(address) => vec![format!("herald serves on {address}")],
        Heard::Worked(Worked::Announced(announced)) => announced.iter().map(report).collect(),
        Heard::Worked(Worked::Followed(followed)) => vec![follow(followed)],
        Heard::Worked(Worked::Interacted(interacted)) => interacted.iter().map(answered).collect(),
        Heard::Polled(swept) => swept.iter().flat_map(polled).collect(),
        Heard::Kept(Ok(kept)) => kept.iter().map(chored).collect(),
        Heard::Kept(Err(stop)) => vec![format!(
            "housekeeping stopped: {}. The next round tries again.",
            why(&stop)
        )],
        Heard::Unaccepted(reason) => vec![format!("a connection could not be taken: {reason}")],
        Heard::Session { destination, said } => vec![format!("{}: {said}", destination.as_str())],
    }
}

fn answered(interacted: &Interacted) -> String {
    let asked = &interacted.asked;
    let command = asked.command.name();
    let destination = asked.destination.as_str();
    match (&interacted.answer.trouble, &interacted.sent) {
        (Some(trouble), _) => format!("{destination}: {command} could not be answered: {trouble}"),
        (None, Err(error)) => {
            format!("{destination}: the answer to {command} could not be sent: {error}")
        }
        (None, Ok(())) => format!(
            "{destination}: {command} was answered in {}",
            asked.place.id
        ),
    }
}

fn follow(followed: Result<Followed, FollowError>) -> String {
    match followed {
        Ok(Followed::Accepted {
            follower, actor, ..
        }) => format!("{actor}: {follower} follows it now"),
        Ok(Followed::Undone { follower, actor }) => {
            format!("{actor}: {follower} no longer follows it")
        }
        Ok(Followed::Ignored(why)) => format!("an activity was ignored: {why}"),
        Err(error) => format!("an activity was not taken, and may be delivered again: {error}"),
    }
}

pub(crate) async fn serving(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    listen: Option<&str>,
    out: &mut dyn Write,
    stop: impl Future<Output = ()>,
) -> Result<u8, String> {
    let address = listen
        .map(ToOwned::to_owned)
        .or_else(|| vars.get(LISTEN).filter(|value| !value.is_empty()).cloned())
        .unwrap_or_else(|| config.server.listen.clone());
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|error| format!("herald could not listen on {address}: {error}"))?;
    let (records, connected) = reached(config, vars, connect, Commands::Answered).await?;
    let secrets = EnvSecrets::from_vars(vars.clone());
    let webhooks = webhooks(config, &secrets)
        .await
        .map_err(|error| error.to_string())?;
    let public = Capped::new(connect(PUBLIC)?, config.server.fetches);
    let silence = std::time::Duration::try_from(config.server.silence).unwrap_or_default();
    let sockets = worded(NativeSockets::under(SESSIONS))?.silent_for(silence);
    let listening = Listening {
        config,
        connected: &connected,
        webhooks: &webhooks,
        public: &public,
        sockets,
        listener,
        out: RefCell::new(out),
        stop,
    };
    Ok(records.serve(&connected, listening).await)
}
