//! `herald server`: the config's store and keys, served over HTTP until
//! the run is stopped (ADR-0014).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::io::Write;

use herald_compose::{Capped, Served};
use herald_config::Config;
use herald_crypto::{RingSigner, RingVerifier};
use herald_environment::{EnvSecrets, SystemClock};
use herald_flows::{Connected, Keys, Ports, ROUND_SECRET, Serving, Webhooks, webhooks};
use herald_native::NativeSockets;
use herald_ports::{Keyring, PUBLIC, Secret, Store as Keeps};
use herald_records::MemoryKeyring;
use herald_server::{Asked, Heard, Limits, READ, serve};

use herald_compose::told;

use super::output::say;
use super::stores::{Commands, SERVED, reached};
use super::{Connect, Connection, DONE, LISTEN, worded};

/// A server's work, over whichever store the config names, and the keys
/// it checks activities with.
struct Listening<'a, F> {
    config: &'a Config,
    connected: &'a Connected<Connection>,
    webhooks: &'a Webhooks,
    public: &'a Capped<Connection>,
    sockets: NativeSockets,
    listener: tokio::net::TcpListener,
    asked: Option<Asked<'a>>,
    secret: Option<Secret>,
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
            reread: server.keys_reread,
        };
        let serving = Serving {
            config: self.config,
            connected: self.connected,
            ports: &ports,
            verifier: &RingVerifier,
            public: self.public,
            keys: &keys,
            webhooks: self.webhooks,
            admission: herald_flows::Admission::Once,
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
            asked: self.asked,
            secret: self.secret.as_ref(),
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

/// Where Azure Functions tells a custom handler to listen.
const AZURE_PORT: &str = "FUNCTIONS_CUSTOMHANDLER_PORT";
/// The app setting that overrides `host.json`'s HTTP route prefix.
const AZURE_PREFIX: &str = "AzureFunctionsJobHost__extensions__http__routePrefix";
/// The file Azure Functions reads its HTTP route prefix from.
const AZURE_HOST: &str = "host.json";
/// The route prefix Azure Functions serves HTTP under where nothing sets
/// one.
const AZURE_DEFAULT_PREFIX: &str = "api";

/// The prefix Azure Functions serves outside HTTP under: the app setting,
/// else `host`'s `extensions.http.routePrefix`, else Azure's own default.
pub(crate) fn azure_prefix(vars: &BTreeMap<String, String>, host: Option<&str>) -> String {
    if let Some(prefix) = vars.get(AZURE_PREFIX) {
        return prefix.trim_matches('/').to_owned();
    }
    host.and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|host| {
            host.pointer("/extensions/http/routePrefix")
                .and_then(serde_json::Value::as_str)
                .map(|prefix| prefix.trim_matches('/').to_owned())
        })
        .unwrap_or_else(|| AZURE_DEFAULT_PREFIX.to_owned())
}

/// Where Azure Functions' timer triggers ask for rounds, by the functions
/// `deploy/azure` names, with outside HTTP under `prefix`; none where the
/// prefix is empty, as a request from outside could then reach them.
pub(crate) fn timers(prefix: &str) -> Option<Asked<'_>> {
    (!prefix.is_empty()).then_some(Asked {
        poll: "/poll",
        housekeep: "/housekeep",
        reply: r#"{"Outputs":{},"Logs":[],"ReturnValue":null}"#,
        prefix,
    })
}

/// What a server says of what it heard.
pub(crate) fn said(heard: Heard) -> Vec<String> {
    match heard {
        Heard::Listening(address) => vec![format!("herald serves on {address}")],
        Heard::Worked(worked) => told::worked(&worked),
        Heard::Polled(swept) => swept.iter().flat_map(told::polled).collect(),
        Heard::Kept(kept) => told::kept(&kept),
        Heard::Unaccepted(reason) => vec![format!("a connection could not be taken: {reason}")],
        Heard::Refused(reason) => vec![told::refused(&reason)],
        Heard::Session { destination, said } => vec![format!("{}: {said}", destination.as_str())],
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
    let azure = vars
        .get(AZURE_PORT)
        .filter(|port| !port.is_empty())
        .map(|port| format!("127.0.0.1:{port}"));
    let prefix = azure
        .is_some()
        .then(|| azure_prefix(vars, std::fs::read_to_string(AZURE_HOST).ok().as_deref()));
    let asked = prefix.as_deref().and_then(timers);
    let secret = vars
        .get(ROUND_SECRET)
        .filter(|value| !value.is_empty())
        .map(|value| Secret::new(ROUND_SECRET, value.clone()));
    let address = listen
        .map(ToOwned::to_owned)
        .or_else(|| vars.get(LISTEN).filter(|value| !value.is_empty()).cloned())
        .or(azure)
        .unwrap_or_else(|| config.server.listen.clone());
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|error| format!("herald could not listen on {address}: {error}"))?;
    let (records, connected) = reached(config, vars, connect, Commands::Answered, &SERVED).await?;
    let secrets = EnvSecrets::from_vars(vars.clone());
    let webhooks = webhooks(config, &secrets)
        .await
        .map_err(|error| error.to_string())?;
    let public = Capped::new(connect(PUBLIC)?, config.server.fetches);
    let silence = std::time::Duration::try_from(config.server.silence).unwrap_or_default();
    let sockets = worded(NativeSockets::under(config.session_domains()))?.silent_for(silence);
    let listening = Listening {
        config,
        connected: &connected,
        webhooks: &webhooks,
        public: &public,
        sockets,
        listener,
        asked,
        secret,
        out: RefCell::new(out),
        stop,
    };
    Ok(records.serve(&connected, listening).await)
}
