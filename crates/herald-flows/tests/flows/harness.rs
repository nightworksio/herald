use std::collections::VecDeque;
use std::sync::Mutex;

use herald_config::{Config, Uses, load};
use herald_core::{
    Cursor, DestinationName, Draft, Entry, Event, Identity, Key, Kind, RouteId, Schedule, Scheme,
    SourceName, State, Step, Subscription,
};
use herald_flows::{
    Announced, Connected, Housekept, Keys, Outcome, Ports, Stop, announce, housekeep, prepare,
};
use herald_ports::{
    Announcements, Cursors, Deliveries, Held, Http, HttpError, Inboxes, Keeper, Ledger, Notes,
    Pins, Request, Response, Schedules, Store, StoreError, Subscriptions, Version, Versioned,
    Written,
};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeStore};
use jiff::Timestamp;

pub(crate) const NOON: Timestamp = Timestamp::constant(1_793_448_000, 0);

pub(crate) fn config(extra: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "release"
crosspost = true
thread = {{ name = "{{{{ event.attributes.version }}}}", archive_after = "14d" }}
pin = "markers"
{extra}
[templates.release]
title = "📦 {{{{ event.project }}}} {{{{ event.attributes.version }}}}"
body = "{{% if event.attributes.version == 'boom' %}}{{{{ nothing.here }}}}{{% endif %}}{{{{ event.body }}}}"
"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

pub(crate) fn event(version: &str) -> Event {
    Event::normalise(Draft {
        identity: format!("github.com/1/{version}"),
        kind: Kind::RELEASE,
        project: "php-mutation-gate".to_owned(),
        attributes: herald_core::versioned(version.to_owned(), false),
        title: "php-mutation-gate".to_owned(),
        body: "## Added".to_owned(),
        links: [("release".to_owned(), "https://example.org/r".to_owned())].into(),
        occurred: NOON,
    })
    .unwrap()
}

pub(crate) fn secrets() -> FakeSecrets {
    FakeSecrets::default().with("HERALD_DISCORD_TOKEN", "hunter2")
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Script {
    pub(crate) answers: std::sync::Arc<Mutex<VecDeque<(String, u16, String)>>>,
    seen: std::sync::Arc<Mutex<Vec<String>>>,
    bodies: std::sync::Arc<Mutex<Vec<Vec<u8>>>>,
    headers: Vec<(String, String)>,
}

impl Script {
    pub(crate) fn answering(answers: &[(&str, u16, &str)]) -> Self {
        let script = Self::default();
        script.answers.lock().unwrap().extend(
            answers
                .iter()
                .map(|(line, status, body)| ((*line).to_owned(), *status, (*body).to_owned())),
        );
        script
    }

    pub(crate) fn headed(mut self, headers: &[(&str, &str)]) -> Self {
        self.headers = headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        self
    }

    pub(crate) fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    pub(crate) fn bodies(&self) -> Vec<Vec<u8>> {
        self.bodies.lock().unwrap().clone()
    }
}

impl Http for Script {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let line = format!("{} {}", request.method.as_str(), request.url.redacted());
        self.seen.lock().unwrap().push(line.clone());
        self.bodies.lock().unwrap().push(request.body.clone());
        let Some((expected, status, body)) = self.answers.lock().unwrap().pop_front() else {
            return Err(HttpError::Unreachable {
                reason: format!("nothing scripted for {line}"),
            });
        };
        assert!(line.ends_with(&expected), "{line} is not {expected}");
        if body.len() > request.most {
            return Err(HttpError::Unreachable {
                reason: format!("the response is larger than {} bytes", request.most),
            });
        }
        Ok(Response {
            status,
            headers: self.headers.clone(),
            body: body.into_bytes(),
        })
    }
}

pub(crate) const POSTED: (&str, u16, &str) = (
    "/channels/1566200000000000100/messages",
    200,
    r#"{"id":"1"}"#,
);
pub(crate) const CROSSPOSTED: (&str, u16, &str) = (
    "/channels/1566200000000000100/messages/1/crosspost",
    200,
    r#"{"id":"1"}"#,
);
pub(crate) const THREADED: (&str, u16, &str) = (
    "/channels/1566200000000000100/messages/1/threads",
    200,
    r#"{"id":"7"}"#,
);
pub(crate) const PINNED: (&str, u16, &str) =
    ("/channels/1566200000000000100/messages/pins/1", 204, "");
pub(crate) const WHO: (&str, u16, &str) = ("/users/@me", 200, r#"{"id":"99"}"#);
pub(crate) const NO_PINS: (&str, u16, &str) = (
    "/channels/1566200000000000100/messages/pins?limit=50",
    200,
    r#"{"items":[]}"#,
);

pub(crate) fn all() -> Vec<Step> {
    vec![
        Step::Post,
        Step::Crosspost,
        Step::OpenThread,
        Step::Pin,
        Step::Unpin,
    ]
}

pub(crate) async fn run<S: Store>(
    config: &Config,
    store: &S,
    clock: &FakeClock,
    script: &Script,
    version: &str,
    run: &str,
) -> Vec<Announced> {
    let prepared = prepare(config, &secrets(), &FakeSigner, Uses::Polls)
        .await
        .unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store,
        clock,
        signer: &FakeSigner,
    };
    announce(
        config,
        &connected,
        &ports,
        &SourceName::new("gate"),
        &event(version),
        run,
    )
    .await
}

pub(crate) fn outcome(announced: &[Announced]) -> &Outcome {
    &announced[0].outcome
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    Nothing,
    Skipped,
    ExpiresDeliveries,
    ExpiresEntries,
    ForgetsDeliveries,
    ExpiresNotes,
    KeepsNoDeliveries,
    KeepsNoInboxes,
    Cursors,
    WritesCursors,
    CursorConflicts,
    ListsSchedules,
    Reads,
    Claims,
    ClaimConflicts,
    Writes,
    Schedules,
    Pins,
    RemembersPins,
    Releases,
    ReleasesDeliveries,
    Subscribers,
    Subscribes,
    Unsubscribes,
    KeepsNotes,
    ReadsNotes,
    ReadsLatest,
    ListsLatest,
    KeepsLatest,
    LatestConflicts,
    KeepsNoLatest,
    Admits,
    ReadsDeliveries,
    AttemptsDeliveries,
    LandsDeliveries,
    LandsOneDelivery,
}

pub(crate) struct Failing {
    pub(crate) fault: Fault,
    pub(crate) inner: FakeStore,
    pub(crate) writes: Mutex<u32>,
    pub(crate) landings: Mutex<u32>,
}

pub(crate) fn healthy() -> Failing {
    Failing {
        fault: Fault::Nothing,
        inner: FakeStore::default(),
        writes: Mutex::new(0),
        landings: Mutex::new(0),
    }
}

pub(crate) fn broken() -> StoreError {
    StoreError::Unavailable {
        reason: "broken".to_owned(),
    }
}

impl Inboxes for Failing {
    async fn delivery(&self, key: &Key, inbox: &str) -> Result<Option<State>, StoreError> {
        if self.fault == Fault::ReadsDeliveries {
            return Err(broken());
        }
        self.inner.delivery(key, inbox).await
    }

    async fn put_delivery(&self, key: &Key, inbox: &str, state: &State) -> Result<(), StoreError> {
        let landings = {
            let mut landings = self.landings.lock().unwrap();
            if matches!(state, State::Landed { .. }) {
                *landings += 1;
            }
            *landings
        };
        match (self.fault, state) {
            (Fault::AttemptsDeliveries, State::Attempted { .. })
            | (Fault::LandsDeliveries, State::Landed { .. }) => Err(broken()),
            (Fault::LandsOneDelivery, State::Landed { .. }) if landings > 1 => Err(broken()),
            _ => self.inner.put_delivery(key, inbox, state).await,
        }
    }

    async fn forget_deliveries(&self, key: &Key) -> Result<(), StoreError> {
        if self.fault == Fault::ForgetsDeliveries {
            return Err(broken());
        }
        self.inner.forget_deliveries(key).await
    }
}

impl Ledger for Failing {
    async fn entry(&self, key: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        if self.fault == Fault::Reads {
            return Err(broken());
        }
        self.inner.entry(key).await
    }

    async fn put_entry(
        &self,
        entry: &Entry,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let writes = {
            let mut writes = self.writes.lock().unwrap();
            *writes += 1;
            *writes
        };
        match (self.fault, writes) {
            (Fault::Claims, 1) => Err(broken()),
            (Fault::ClaimConflicts, 1) | (Fault::Writes, 2) => Ok(Written::Conflict),
            (Fault::Releases, _) if entry.lease.is_none() => Err(broken()),
            _ => self.inner.put_entry(entry, expected).await,
        }
    }

    async fn expire_entries(&self, before: jiff::Timestamp) -> Result<Vec<Key>, StoreError> {
        if self.fault == Fault::ExpiresEntries {
            return Err(broken());
        }
        self.inner.expire_entries(before).await
    }
}

impl Cursors for Failing {
    async fn cursor(&self, source: &SourceName) -> Result<Option<Versioned<Cursor>>, StoreError> {
        if self.fault == Fault::Cursors {
            return Err(broken());
        }
        self.inner.cursor(source).await
    }

    async fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self.fault {
            Fault::WritesCursors => Err(broken()),
            Fault::CursorConflicts => Ok(Written::Conflict),
            _ => self.inner.put_cursor(source, cursor, expected).await,
        }
    }
}

impl Schedules for Failing {
    async fn every_schedule(&self) -> Result<Vec<Schedule>, StoreError> {
        if self.fault == Fault::ListsSchedules {
            return Err(broken());
        }
        self.inner.every_schedule().await
    }

    async fn put_schedules(&self, add: &[Schedule], remove: &[Schedule]) -> Result<(), StoreError> {
        if self.fault == Fault::Schedules {
            return Err(broken());
        }
        self.inner.put_schedules(add, remove).await
    }
}

impl Subscriptions for Failing {
    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<Subscription>, StoreError> {
        if self.fault == Fault::Subscribers {
            return Err(broken());
        }
        self.inner.subscribers(destination, project).await
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        if self.fault == Fault::Subscribes {
            return Err(broken());
        }
        self.inner.subscribe(subscription).await
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        if self.fault == Fault::Unsubscribes {
            return Err(broken());
        }
        self.inner.unsubscribe(subscription).await
    }
}

impl Pins for Failing {
    async fn pinned(&self, route: &RouteId) -> Result<Option<String>, StoreError> {
        if self.fault == Fault::Pins {
            return Err(broken());
        }
        self.inner.pinned(route).await
    }

    async fn put_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        if self.fault == Fault::RemembersPins {
            return Err(broken());
        }
        self.inner.put_pinned(route, post).await
    }
}

impl Deliveries for Failing {
    async fn admit(
        &self,
        scheme: herald_core::Scheme,
        id: &str,
        at: jiff::Timestamp,
    ) -> Result<herald_ports::Admitted, StoreError> {
        if self.fault == Fault::Admits {
            return Err(broken());
        }
        self.inner.admit(scheme, id, at).await
    }

    async fn release(&self, scheme: herald_core::Scheme, id: &str) -> Result<(), StoreError> {
        if self.fault == Fault::ReleasesDeliveries {
            return Err(broken());
        }
        self.inner.release(scheme, id).await
    }

    async fn expire_deliveries(&self, before: jiff::Timestamp) -> Result<(), StoreError> {
        if self.fault == Fault::ExpiresDeliveries {
            return Err(broken());
        }
        self.inner.expire_deliveries(before).await
    }
}

impl Notes for Failing {
    async fn note(&self, key: &herald_core::Key) -> Result<Option<String>, StoreError> {
        if self.fault == Fault::ReadsNotes {
            return Err(broken());
        }
        self.inner.note(key).await
    }

    async fn put_note(
        &self,
        actor: &str,
        key: &herald_core::Key,
        note: &str,
        at: jiff::Timestamp,
    ) -> Result<(), StoreError> {
        if self.fault == Fault::KeepsNotes {
            return Err(broken());
        }
        self.inner.put_note(actor, key, note, at).await
    }

    async fn outbox(
        &self,
        actor: &str,
        after: Option<&str>,
        most: usize,
    ) -> Result<herald_ports::Outboxed, StoreError> {
        if self.fault == Fault::ReadsNotes {
            return Err(broken());
        }
        self.inner.outbox(actor, after, most).await
    }

    async fn expire_notes(&self, before: jiff::Timestamp) -> Result<(), StoreError> {
        if self.fault == Fault::ExpiresNotes {
            return Err(broken());
        }
        self.inner.expire_notes(before).await
    }
}

impl Announcements for Failing {
    async fn latest(
        &self,
        project: &str,
    ) -> Result<Option<Versioned<herald_core::Latest>>, StoreError> {
        if self.fault == Fault::ReadsLatest {
            return Err(broken());
        }
        self.inner.latest(project).await
    }

    async fn every_latest(&self) -> Result<Vec<herald_core::Latest>, StoreError> {
        if self.fault == Fault::ListsLatest {
            return Err(broken());
        }
        self.inner.every_latest().await
    }

    async fn put_latest(
        &self,
        latest: &herald_core::Latest,
        expected: Option<herald_ports::Version>,
    ) -> Result<Written, StoreError> {
        match self.fault {
            Fault::KeepsLatest => Err(broken()),
            Fault::LatestConflicts => Ok(Written::Conflict),
            _ => self.inner.put_latest(latest, expected).await,
        }
    }
}

impl Keeper for Failing {
    fn holds(&self, kind: Held) -> bool {
        match kind {
            Held::Deliveries => self.fault != Fault::KeepsNoDeliveries,
            Held::Inboxes => self.fault != Fault::KeepsNoInboxes,
            Held::Latest => self.fault != Fault::KeepsNoLatest,
            _ => true,
        }
    }

    fn skipped(&self) -> Vec<String> {
        if self.fault == Fault::Skipped {
            vec!["latest/bad.json".to_owned()]
        } else {
            Vec::new()
        }
    }
}

pub(crate) fn marks() -> String {
    let route = herald_core::Marker::of(&fake_mac(&herald_core::Marker::route_message(
        &RouteId::new("releases"),
    )));
    let key = Key::derive(&RouteId::new("releases"), &Identity::new("github.com/1/v1"));
    let announcement = herald_core::Marker::of(&fake_mac(&herald_core::Marker::message(&key)));
    format!("{route}.{announcement}")
}

pub(crate) fn fake_mac(message: &[u8]) -> Vec<u8> {
    let derived =
        herald_core::hex(&[b"HmacSha256", &b"hunter2"[..], b"herald/marker-key/v1"].join(&b'|'));
    [b"HmacSha256", derived.as_bytes(), message].join(&b'|')
}

/// A store that answers every call with the same error.
pub(crate) struct Refusing(pub(crate) StoreError);

impl herald_ports::Inboxes for Refusing {
    async fn delivery(
        &self,
        _: &herald_core::Key,
        _: &str,
    ) -> Result<Option<herald_core::State>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_delivery(
        &self,
        _: &herald_core::Key,
        _: &str,
        _: &herald_core::State,
    ) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn forget_deliveries(&self, _: &herald_core::Key) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Ledger for Refusing {
    async fn entry(
        &self,
        _: &herald_core::Key,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Entry>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_entry(
        &self,
        _: &herald_core::Entry,
        _: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        Err(self.0.clone())
    }

    async fn expire_entries(&self, _: Timestamp) -> Result<Vec<herald_core::Key>, StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Cursors for Refusing {
    async fn cursor(
        &self,
        _: &herald_core::SourceName,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Cursor>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_cursor(
        &self,
        _: &herald_core::SourceName,
        _: &herald_core::Cursor,
        _: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Schedules for Refusing {
    async fn every_schedule(&self) -> Result<Vec<herald_core::Schedule>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_schedules(
        &self,
        _: &[herald_core::Schedule],
        _: &[herald_core::Schedule],
    ) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Subscriptions for Refusing {
    async fn subscribers(
        &self,
        _: &herald_core::DestinationName,
        _: &str,
    ) -> Result<Vec<herald_core::Subscription>, StoreError> {
        Err(self.0.clone())
    }

    async fn subscribe(&self, _: &herald_core::Subscription) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn unsubscribe(&self, _: &herald_core::Subscription) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Pins for Refusing {
    async fn pinned(&self, _: &herald_core::RouteId) -> Result<Option<String>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_pinned(&self, _: &herald_core::RouteId, _: &str) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Deliveries for Refusing {
    async fn admit(
        &self,
        _: Scheme,
        _: &str,
        _: Timestamp,
    ) -> Result<herald_ports::Admitted, StoreError> {
        Err(self.0.clone())
    }

    async fn release(&self, _: Scheme, _: &str) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn expire_deliveries(&self, _: Timestamp) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Notes for Refusing {
    async fn note(&self, _: &herald_core::Key) -> Result<Option<String>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_note(
        &self,
        _: &str,
        _: &herald_core::Key,
        _: &str,
        _: Timestamp,
    ) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn outbox(
        &self,
        _: &str,
        _: Option<&str>,
        _: usize,
    ) -> Result<herald_ports::Outboxed, StoreError> {
        Err(self.0.clone())
    }

    async fn expire_notes(&self, _: Timestamp) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Announcements for Refusing {
    async fn latest(
        &self,
        _: &str,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Latest>>, StoreError> {
        Err(self.0.clone())
    }

    async fn every_latest(&self) -> Result<Vec<herald_core::Latest>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_latest(
        &self,
        _: &herald_core::Latest,
        _: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        Err(self.0.clone())
    }
}

impl Keeper for Refusing {
    fn holds(&self, _: Held) -> bool {
        true
    }

    fn skipped(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Every destination, source and store kind herald ships, in the order herald lists
/// them.
pub fn kinds() -> herald_config::Kinds {
    herald_config::Kinds::new()
        .destination::<herald_dest_discord::DiscordKind>()
        .destination::<herald_dest_fediverse::MastodonKind>()
        .destination::<herald_dest_matrix::MatrixKind>()
        .destination::<herald_dest_slack::SlackKind>()
        .destination::<herald_dest_fediverse::ActorKind>()
        .source::<herald_sources_forge::GitHubKind>()
        .source::<herald_sources_forge::GitLabKind>()
        .source::<herald_sources_forge::ForgejoKind>()
        .source::<herald_sources_registry::PackagistKind>()
        .source::<herald_sources_registry::CratesIoKind>()
        .source::<herald_sources_registry::NpmKind>()
        .source::<herald_sources_registry::DockerHubKind>()
        .source::<herald_sources_feed::FeedKind>()
        .store::<herald_store_sqlite::SqliteKind>()
        .store::<herald_store_postgres::PostgresKind>()
        .store::<herald_store_d1::D1Kind>()
        .store::<herald_store_durable_object::DurableObjectKind>()
        .store::<herald_store_dynamodb::DynamodbKind>()
        .store::<herald_store_deno_kv::DenoKvKind>()
        .store::<herald_store_fastly_kv::FastlyKvKind>()
        .store::<herald_store_markers::MarkersKind>()
        .store::<herald_store_git_branch::GitBranchKind>()
        .store::<herald_store_actions_cache::ActionsCacheKind>()
        .store::<herald_store_repo_variables::RepoVariablesKind>()
}

/// `text`, loaded as `herald.toml` with every kind herald ships, where it
/// names no other file.
pub fn loaded(text: &str) -> herald_config::Config {
    herald_config::load("herald.toml", text, &|_| Err(String::new()), &kinds()).unwrap()
}

/// The keys in `keyring`, each used for an hour and read again a minute
/// after it fails.
pub(crate) fn keys_in<K>(keyring: &K) -> Keys<'_, K> {
    Keys {
        keyring,
        refreshed: jiff::SignedDuration::from_hours(1),
        reread: jiff::SignedDuration::from_mins(1),
    }
}

/// Housekeeping at noon over `store`, keeping what `retention` says, its
/// keys in a keyring of their own.
pub(crate) fn housekept<S: herald_ports::Store, H: herald_ports::Http>(
    connected: &Connected<H>,
    store: &S,
    retention: &herald_core::Retention,
) -> Result<Housekept, Stop> {
    let keyring = herald_records::MemoryKeyring::new(1);
    pollster::block_on(housekeep(
        connected,
        store,
        &FakeClock::at(NOON),
        &keys_in(&keyring),
        retention,
    ))
}
