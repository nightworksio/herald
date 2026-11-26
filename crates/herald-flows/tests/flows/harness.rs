use std::sync::Mutex;

use herald_config::{Config, Uses, load};
use herald_core::{
    Cursor, DestinationName, Draft, Entry, Event, Identity, Key, Kind, RouteId, Schedule,
    SourceName, State, Step, Subscription,
};
use herald_flows::{
    Announced, Connected, Housekept, Keys, Outcome, Ports, Stop, announce, housekeep, prepare,
};
use herald_ports::{
    Announcements, Cursors, Deliveries, Held, Http, Inboxes, Keeper, Ledger, Notes, Pins,
    Positions, Schedules, Store, StoreError, Subscriptions, Version, Versioned, Written,
};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeStore};
use jiff::Timestamp;

mod actions;
mod pending;
mod pulls;
mod recalls;
mod refusing;
mod script;

pub(crate) use script::{Script, UNCONNECTED};

pub(crate) const NOON: Timestamp = Timestamp::constant(1_793_448_000, 0);

fn written(extra: &str) -> String {
    format!(
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
    )
}

pub(crate) fn config(extra: &str) -> Config {
    parsed(&written(extra))
}

/// The config `config` gives, its route saying `private = true` and so
/// crossposting nothing, since a crosspost is posted where anyone may read
/// it (ADR-0067).
pub(crate) fn private_config(extra: &str) -> Config {
    parsed(&written(extra).replace("crosspost = true\n", "private = true\n"))
}

fn parsed(text: &str) -> Config {
    load(
        "herald.toml",
        text,
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
    FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "hunter2")
        .with("HERALD_APP_KEY", "the App's key")
        .with("BAD", "a key that signs nothing")
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
    run_within(
        config,
        store,
        (clock, herald_flows::Budget::UNBOUNDED),
        script,
        version,
        run,
    )
    .await
}

/// [`run`], what it may spend bounded by the budget beside its clock.
pub(crate) async fn run_within<S: Store, H: Http + Clone>(
    config: &Config,
    store: &S,
    (clock, budget): (&FakeClock, herald_flows::Budget<'_>),
    http: &H,
    version: &str,
    run: &str,
) -> Vec<Announced> {
    let prepared = prepare(config, &secrets(), &FakeSigner, Uses::Polls)
        .await
        .unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(http.clone())).unwrap();
    let ports = Ports {
        store,
        clock,
        signer: &FakeSigner,
        budget,
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
    ReadsPending,
    ReadsHeld,
    ReadsOnce,
    ReadsTwice,
    ReadsGroups,
    WritesGroups,
    WritesPending,
    PendingConflicts,
    ClosingConflicts,
    GroupedConflicts,
    DropsPending,
    Skipped,
    ExpiresDeliveries,
    ExpiresEntries,
    ForgetsDeliveries,
    ExpiresNotes,
    KeepsNoDeliveries,
    KeepsNoInboxes,
    KeepsNoPending,
    Cursors,
    WritesCursors,
    CursorConflicts,
    ReadsMembers,
    WritesMembers,
    MemberConflicts,
    ForgetsMembers,
    ListingConflicts,
    ReadsTokens,
    WritesTokens,
    ReadsPublished,
    WritesPublished,
    ConflictsPublished,
    /// Reading the static signed log's record fails.
    ReadsPublication,
    /// The first two writes recording a post a replacement posted as
    /// landed fail, its landing and the release after it, as where a run
    /// stops once the post landed and before herald recorded it.
    LandsReplacedPostOnce,
    /// The first two writes recording a post's first chunk as landed fail,
    /// as where a run stops once the chunk landed and before herald
    /// recorded it.
    LandsChunkOnce,
    /// Writing the static signed log's record fails.
    WritesPublication,
    /// Writing the static signed log's record conflicts.
    ConflictsPublication,
    ReadsFeeds,
    WritesFeeds,
    ConflictsFeeds,
    ReadsPeerKeys,
    WritesPeerKeys,
    ConflictsPeerKeys,
    /// Reading a peer's audit fails.
    ReadsAudits,
    /// Writing a peer's audit fails.
    WritesAudits,
    /// Writing a peer's audit always conflicts.
    ConflictsAudits,
    /// Forgetting expired audits fails.
    ExpiresAudits,
    /// Reading a peer's window fails.
    ReadsWindows,
    /// Writing a peer's window always conflicts.
    ConflictsWindows,
    /// Writing a peer's window fails.
    WritesWindows,
    KeepsNoPublished,
    ListsSchedules,
    Reads,
    Claims,
    ClaimConflicts,
    Writes,
    Schedules,
    Pins,
    RemembersPins,
    ForgetsPins,
    ForgetsNotes,
    ForgetsLatest,
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
    SeesDeliveries,
    SeesNoDeliveries,
    ReadsDeliveries,
    AttemptsDeliveries,
    LandsDeliveries,
    LandsOneDelivery,
    ReadsPositions,
    KeepsPositions,
    KeepsNoPositions,
    KeepsNoDecisions,
    ReadsDecisions,
    WritesDecisions,
    ExpiresDecisions,
    KeepsNoPulls,
    ReadsPulls,
    ListsPulls,
    WritesPulls,
    RacesPulls,
    ExpiresPulls,
    ReadsReconciles,
    WritesReconciles,
    ReadsCodeowners,
    WritesCodeowners,
    KeepsNoRecalls,
    ReadsRecalls,
    WritesRecalls,
    RecallsConflict,
    KeepsNoActions,
    ReadsActions,
    WritesActions,
    ActionsConflict,
    SettlesNoAction,
    /// Every read of a pull request's leaving of herald's queue fails.
    ReadsLeft,
    /// Every write of a pull request's leaving of herald's queue fails.
    WritesLeft,
    OneAtATime,
    /// Every read of a source's members after the first fails.
    ReadsMembersOnce,
    /// Every read of a cursor after the first fails.
    ReadsCursorOnce,
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

impl Failing {
    /// Whether this is the first read it counts.
    fn read_once(&self) -> bool {
        let mut reads = self.landings.lock().unwrap();
        *reads += 1;
        *reads == 1
    }

    /// Whether this is one of the first two landings it counts.
    fn landed_once(&self) -> bool {
        let mut landings = self.landings.lock().unwrap();
        *landings += 1;
        *landings <= 2
    }
}

impl Ledger for Failing {
    async fn entry(&self, key: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        let allowed = match self.fault {
            Fault::ReadsOnce => Some(1),
            Fault::ReadsTwice => Some(2),
            _ => None,
        };
        let read = allowed.is_some_and(|allowed| {
            let mut reads = self.landings.lock().unwrap();
            *reads += 1;
            *reads > allowed
        });
        if self.fault == Fault::Reads || read {
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
            (Fault::GroupedConflicts, _) if entry.group.is_some() => Ok(Written::Conflict),
            (Fault::LandsChunkOnce, _)
                if entry.facts(&Step::Chunk(1)).is_some() && self.landed_once() =>
            {
                Err(broken())
            }
            (Fault::LandsReplacedPostOnce, _)
                if entry
                    .replacing
                    .as_ref()
                    .is_some_and(|replacing| replacing.deleting.is_empty())
                    && entry.facts(&Step::Post).is_some()
                    && self.landed_once() =>
            {
                Err(broken())
            }
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
        if self.fault == Fault::Cursors
            || (self.fault == Fault::ReadsCursorOnce && !self.read_once())
        {
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

    async fn listing(
        &self,
        source: &SourceName,
    ) -> Result<Option<Versioned<herald_core::Listing>>, StoreError> {
        self.inner.listing(source).await
    }

    async fn put_listing(
        &self,
        source: &SourceName,
        listing: &herald_core::Listing,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        if self.fault == Fault::ListingConflicts {
            return Ok(Written::Conflict);
        }
        self.inner.put_listing(source, listing, expected).await
    }

    async fn members(
        &self,
        source: &SourceName,
    ) -> Result<Vec<Versioned<herald_core::Membership>>, StoreError> {
        if self.fault == Fault::ReadsMembers
            || (self.fault == Fault::ReadsMembersOnce && !self.read_once())
        {
            return Err(broken());
        }
        self.inner.members(source).await
    }

    async fn member(
        &self,
        source: &SourceName,
        id: &str,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Membership>>, StoreError> {
        if self.fault == Fault::ReadsMembers {
            return Err(broken());
        }
        self.inner.member(source, id).await
    }

    async fn put_member(
        &self,
        source: &SourceName,
        membership: &herald_core::Membership,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self.fault {
            Fault::WritesMembers => Err(broken()),
            Fault::MemberConflicts if membership.cursor.is_some() => Ok(Written::Conflict),
            _ => self.inner.put_member(source, membership, expected).await,
        }
    }

    async fn forget_member(&self, source: &SourceName, id: &str) -> Result<(), StoreError> {
        if self.fault == Fault::ForgetsMembers {
            return Err(broken());
        }
        self.inner.forget_member(source, id).await
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

    async fn forget_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        if self.fault == Fault::ForgetsPins {
            return Err(broken());
        }
        self.inner.forget_pinned(route, post).await
    }
}

impl herald_ports::Decisions for Failing {
    async fn decision(&self, key: &Key) -> Result<Option<herald_core::Decided>, StoreError> {
        if self.fault == Fault::ReadsDecisions {
            return Err(broken());
        }
        self.inner.decision(key).await
    }

    async fn put_decision(&self, decided: &herald_core::Decided) -> Result<(), StoreError> {
        if self.fault == Fault::WritesDecisions {
            return Err(broken());
        }
        self.inner.put_decision(decided).await
    }

    async fn expire_decisions(&self, before: Timestamp) -> Result<(), StoreError> {
        if self.fault == Fault::ExpiresDecisions {
            return Err(broken());
        }
        self.inner.expire_decisions(before).await
    }
}

impl Positions for Failing {
    async fn position(&self, destination: &DestinationName) -> Result<Option<String>, StoreError> {
        if self.fault == Fault::ReadsPositions {
            return Err(broken());
        }
        self.inner.position(destination).await
    }

    async fn put_position(
        &self,
        destination: &DestinationName,
        position: &str,
    ) -> Result<(), StoreError> {
        if self.fault == Fault::KeepsPositions {
            return Err(broken());
        }
        self.inner.put_position(destination, position).await
    }
}

impl Deliveries for Failing {
    async fn admit(
        &self,
        namespace: &'static str,
        id: &str,
        at: jiff::Timestamp,
    ) -> Result<herald_ports::Admitted, StoreError> {
        if self.fault == Fault::Admits {
            return Err(broken());
        }
        self.inner.admit(namespace, id, at).await
    }

    async fn seen(&self, namespace: &'static str, id: &str) -> Result<bool, StoreError> {
        match self.fault {
            Fault::SeesDeliveries => Err(broken()),
            Fault::SeesNoDeliveries => Ok(false),
            _ => self.inner.seen(namespace, id).await,
        }
    }

    async fn release(&self, namespace: &'static str, id: &str) -> Result<(), StoreError> {
        if self.fault == Fault::ReleasesDeliveries {
            return Err(broken());
        }
        self.inner.release(namespace, id).await
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

    async fn forget_note(&self, key: &herald_core::Key) -> Result<(), StoreError> {
        if self.fault == Fault::ForgetsNotes {
            return Err(broken());
        }
        self.inner.forget_note(key).await
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

    async fn forget_latest(&self, project: &str) -> Result<(), StoreError> {
        if self.fault == Fault::ForgetsLatest {
            return Err(broken());
        }
        self.inner.forget_latest(project).await
    }
}

impl Keeper for Failing {
    fn holds(&self, kind: Held) -> bool {
        match kind {
            Held::Deliveries => self.fault != Fault::KeepsNoDeliveries,
            Held::Inboxes => self.fault != Fault::KeepsNoInboxes,
            Held::Latest => self.fault != Fault::KeepsNoLatest,
            Held::Positions => self.fault != Fault::KeepsNoPositions,
            Held::Decisions => self.fault != Fault::KeepsNoDecisions,
            Held::Pulls => self.fault != Fault::KeepsNoPulls,
            Held::Recalls => self.fault != Fault::KeepsNoRecalls,
            Held::Actions => self.fault != Fault::KeepsNoActions,
            Held::Pending => self.fault != Fault::KeepsNoPending,
            Held::Published => self.fault != Fault::KeepsNoPublished,
            _ => true,
        }
    }

    fn writes_one_at_a_time(&self) -> bool {
        self.fault == Fault::OneAtATime
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

/// Every destination, source and store kind herald ships, in the order herald lists
/// them.
pub fn kinds() -> herald_config::Kinds {
    herald_config::Kinds::new()
        .destination::<herald_dest_discord::DiscordKind>()
        .destination::<herald_dest_fediverse::MastodonKind>()
        .destination::<herald_dest_matrix::MatrixKind>()
        .destination::<herald_dest_slack::SlackKind>()
        .destination::<herald_dest_fediverse::ActorKind>()
        .destination::<herald_dest_herald::HeraldKind>()
        .destination::<herald_dest_herald::PushKind>()
        .source::<herald_sources_forge::GitHubKind>()
        .source::<herald_sources_forge::GitLabKind>()
        .source::<herald_sources_forge::ForgejoKind>()
        .source::<herald_sources_forge::GhcrKind>()
        .source::<herald_sources_registry::PackagistKind>()
        .source::<herald_sources_registry::CratesIoKind>()
        .source::<herald_sources_registry::NpmKind>()
        .source::<herald_sources_registry::DockerHubKind>()
        .source::<herald_sources_registry::OciKind>()
        .source::<herald_sources_feed::FeedKind>()
        .source::<herald_sources_herald::HeraldKind>()
        .store::<herald_store_sqlite::SqliteKind>()
        .store::<herald_store_postgres::PostgresKind>()
        .store::<herald_store_d1::D1Kind>()
        .store::<herald_store_durable_object::DurableObjectKind>()
        .store::<herald_store_deno_kv::DenoKvKind>()
        .store::<herald_store_fastly_kv::FastlyKvKind>()
        .store::<herald_store_dynamodb::DynamodbKind>()
        .store::<herald_store_git_branch::GitBranchKind>()
        .store::<herald_store_markers::MarkersKind>()
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
        (retention, herald_config::Federation::KEEP_AUDIT),
    ))
}
