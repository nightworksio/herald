use std::collections::VecDeque;
use std::sync::Mutex;

use herald_config::{Config, load};
use herald_core::{
    Cursor, DestinationName, Draft, Entry, Event, Key, Kind, RouteId, Schedule, SourceName, Step,
    Subscription,
};
use herald_flows::{Announced, Outcome, Ports, announce, prepare};
use herald_ports::{
    Capabilities, Http, HttpError, Request, Response, Store, StoreError, Version, Versioned,
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
channel = "100"
template = "release"
crosspost = true
thread = {{ name = "{{{{ event.version }}}}", archive_after = "14d" }}
pin = "markers"
{extra}
[templates.release]
title = "📦 {{{{ event.project }}}} {{{{ event.version }}}}"
body = "{{% if event.version == 'boom' %}}{{{{ nothing.here }}}}{{% endif %}}{{{{ event.body }}}}"
"#
    );
    load("herald.toml", &text, &|_| Err("no file".to_owned())).unwrap()
}

pub(crate) fn event(version: &str) -> Event {
    Event::normalise(Draft {
        identity: format!("github.com/1/{version}"),
        kind: Kind::Release,
        project: "php-mutation-gate".to_owned(),
        version: Some(version.to_owned()),
        title: "php-mutation-gate".to_owned(),
        body: "## Added".to_owned(),
        links: [("release".to_owned(), "https://example.org/r".to_owned())].into(),
        prerelease: false,
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
}

impl Http for Script {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let line = format!("{} {}", request.method.as_str(), request.url.redacted());
        self.seen.lock().unwrap().push(line.clone());
        let Some((expected, status, body)) = self.answers.lock().unwrap().pop_front() else {
            return Err(HttpError::Unreachable {
                reason: format!("nothing scripted for {line}"),
            });
        };
        assert!(line.ends_with(&expected), "{line} is not {expected}");
        Ok(Response {
            status,
            headers: self.headers.clone(),
            body: body.into_bytes(),
        })
    }
}

pub(crate) const POSTED: (&str, u16, &str) = ("/channels/100/messages", 200, r#"{"id":"1"}"#);
pub(crate) const CROSSPOSTED: (&str, u16, &str) =
    ("/channels/100/messages/1/crosspost", 200, r#"{"id":"1"}"#);
pub(crate) const THREADED: (&str, u16, &str) =
    ("/channels/100/messages/1/threads", 200, r#"{"id":"7"}"#);
pub(crate) const PINNED: (&str, u16, &str) = ("/channels/100/messages/pins/1", 204, "");
pub(crate) const WHO: (&str, u16, &str) = ("/users/@me", 200, r#"{"id":"99"}"#);
pub(crate) const NO_PINS: (&str, u16, &str) = (
    "/channels/100/messages/pins?limit=50",
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
    let prepared = prepare(config, &secrets(), &FakeSigner).await.unwrap();
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
}

pub(crate) struct Failing {
    pub(crate) fault: Fault,
    pub(crate) inner: FakeStore,
    pub(crate) writes: Mutex<u32>,
}

pub(crate) fn healthy() -> Failing {
    Failing {
        fault: Fault::Nothing,
        inner: FakeStore::default(),
        writes: Mutex::new(0),
    }
}

pub(crate) fn broken() -> StoreError {
    StoreError::Unavailable {
        reason: "broken".to_owned(),
    }
}

impl Store for Failing {
    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

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

    async fn schedules(&self) -> Result<Vec<Schedule>, StoreError> {
        if self.fault == Fault::ListsSchedules {
            return Err(broken());
        }
        self.inner.schedules().await
    }

    async fn put_schedules(&self, add: &[Schedule], remove: &[Schedule]) -> Result<(), StoreError> {
        if self.fault == Fault::Schedules {
            return Err(broken());
        }
        self.inner.put_schedules(add, remove).await
    }

    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<String>, StoreError> {
        self.inner.subscribers(destination, project).await
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.inner.subscribe(subscription).await
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.inner.unsubscribe(subscription).await
    }

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
