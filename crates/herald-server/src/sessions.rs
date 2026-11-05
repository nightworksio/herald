//! The sessions `herald server` holds for the destinations that carry
//! commands without posting them to herald (ADR-0016): Discord's gateway,
//! Slack's Socket Mode and a Matrix sync. Each answers its commands as they
//! come, at most [`Limits::jobs`](crate::Limits) at once, and reconnects
//! after a lost connection, waiting longer each time it fails.

use std::future::Future;
use std::pin::{Pin, pin};
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use herald_core::{Asked, DestinationName};
use herald_dest_discord::{self as discord, Effect, Gateway};
use herald_dest_matrix::{self as matrix, SYNC_WAIT};
use herald_dest_slack::{self as slack};
use herald_flows::{Interacting, Job, Replying, Serving, Worked, completed, work};
use herald_ports::{Clock, Frame, Http, Signer, Socket, Sockets, Store, Text};
use tokio::time::{Instant, Interval, MissedTickBehavior, interval_at, sleep};

use crate::{Heard, RUN};

/// The first wait after a session fails to connect.
const FIRST_WAIT: Duration = Duration::from_secs(1);
/// The longest wait between attempts.
const LONGEST_WAIT: Duration = Duration::from_secs(60);

type Answering<'a> = FuturesUnordered<Pin<Box<dyn Future<Output = Worked> + 'a>>>;

/// How long a session waits before it connects again, doubling each time it
/// fails, up to [`LONGEST_WAIT`].
struct Backoff(Duration);

impl Backoff {
    fn new() -> Self {
        Self(FIRST_WAIT)
    }

    async fn wait(&mut self) {
        sleep(self.0).await;
        self.0 = (self.0 * 2).min(LONGEST_WAIT);
    }

    fn reset(&mut self) {
        self.0 = FIRST_WAIT;
    }
}

/// What every session shares: the server's ports, what it may answer at
/// once, and where it says what happened.
pub(crate) struct Session<'s, 'a, S, C, G, V, H, P, K> {
    pub(crate) serving: &'s Serving<'a, S, C, G, V, H, P, K>,
    pub(crate) jobs: usize,
    pub(crate) heard: &'s dyn Fn(Heard),
}

impl<'s, S: Store, C: Clock, G: Signer, V, H: Http, P, K> Session<'s, '_, S, C, G, V, H, P, K> {
    fn said(&self, destination: &DestinationName, what: impl Into<String>) {
        (self.heard)(Heard::Session {
            destination: destination.clone(),
            said: what.into(),
        });
    }

    /// Answers `asked` alongside what `answering` holds, or says it cannot
    /// where that is already as much as the server answers at once.
    fn answer(
        &self,
        answering: &mut Answering<'s>,
        destination: &DestinationName,
        asked: Asked,
        replying: Replying,
    ) {
        if answering.len() >= self.jobs {
            self.said(
                destination,
                format!(
                    "{} was not answered: the server answers no more at once",
                    asked.command.name()
                ),
            );
            return;
        }
        let serving = self.serving;
        let job = Job::Interact {
            destination: destination.clone(),
            asked: vec![(asked, replying)],
            deferred: false,
        };
        answering.push(Box::pin(async move { work(serving, job, RUN).await }));
    }

    fn worked(&self, worked: Option<Worked>) {
        if let Some(worked) = worked {
            (self.heard)(Heard::Worked(worked));
        }
    }

    /// Every session `serving`'s destinations hold, until each stops.
    pub(crate) async fn held<So: Sockets>(&self, sockets: &So) {
        let mut sessions: FuturesUnordered<Pin<Box<dyn Future<Output = ()> + '_>>> =
            FuturesUnordered::new();
        for (name, interacting, http, opening) in self.serving.connected.interactions() {
            match interacting {
                Interacting::Discord(discord)
                    if discord.via() == herald_dest_discord::Via::Gateway =>
                {
                    if let Some(bot) = discord.bot() {
                        let gateway = Gateway::new(bot.clone());
                        sessions.push(Box::pin(
                            self.gateway(sockets, name, discord, gateway, http),
                        ));
                    }
                }
                Interacting::Slack {
                    interacting: slack,
                    app: Some(app),
                    ..
                } if slack.via() == herald_dest_slack::Via::Socket => {
                    if let Some(opening) = opening {
                        sessions.push(Box::pin(self.socket(sockets, name, slack, app, opening)));
                    }
                }
                Interacting::Matrix {
                    interacting: matrix,
                    ..
                } if matrix.via() == herald_dest_matrix::Via::Sync => {
                    sessions.push(Box::pin(self.sync(name, matrix, http)));
                }
                _ => {}
            }
        }
        while sessions.next().await.is_some() {}
    }

    async fn gateway<So: Sockets>(
        &self,
        sockets: &So,
        name: &DestinationName,
        interacting: &discord::Interacting,
        mut gateway: Gateway,
        http: &H,
    ) {
        let mut backoff = Backoff::new();
        let mut answering: Answering<'s> = FuturesUnordered::new();
        loop {
            let mut socket = match sockets.open(&gateway.url()).await {
                Ok(socket) => socket,
                Err(error) => {
                    self.said(
                        name,
                        format!("Discord's gateway could not be reached: {error}"),
                    );
                    backoff.wait().await;
                    continue;
                }
            };
            let mut beats: Option<Interval> = None;
            let stopped = loop {
                tokio::select! {
                    biased;
                    frame = socket.receive() => {
                        let effects = match frame {
                            Frame::Text(text) => gateway.received(interacting, name, &text),
                            Frame::Closed(code) => vec![gateway.closed(code)],
                        };
                        let mut next = None;
                        for effect in effects {
                            match effect {
                                Effect::Send(frame) => {
                                    if socket.send(frame).await.is_err() {
                                        next = Some(None);
                                    }
                                }
                                Effect::Beat(every) => {
                                    backoff.reset();
                                    beats = Some(beating(every));
                                }
                                Effect::Received(received) => {
                                    self.discord(&mut answering, name, interacting, http, received).await;
                                }
                                Effect::Reconnect { .. } => next = Some(None),
                                Effect::Stop(reason) => next = Some(Some(reason)),
                            }
                        }
                        if let Some(next) = next {
                            break next;
                        }
                    }
                    () = beat(&mut beats) => match gateway.beat() {
                        Effect::Send(frame) => {
                            if socket.send(frame).await.is_err() {
                                break None;
                            }
                        }
                        _ => break None,
                    },
                    Some(worked) = answering.next(), if !answering.is_empty() => self.worked(Some(worked)),
                }
            };
            if let Some(reason) = stopped {
                self.said(name, reason);
                while let Some(worked) = answering.next().await {
                    self.worked(Some(worked));
                }
                return;
            }
            backoff.wait().await;
        }
    }

    async fn discord(
        &self,
        answering: &mut Answering<'s>,
        name: &DestinationName,
        interacting: &discord::Interacting,
        http: &H,
        received: discord::Received,
    ) {
        match received {
            discord::Received::Asked { asked, replying } => {
                self.answer(answering, name, asked, Replying::Discord(replying));
            }
            discord::Received::Completing {
                place,
                typed,
                replying,
            } => {
                let serving = self.serving;
                let projects =
                    completed(serving.config, serving.ports.store, name, &place, &typed).await;
                let sent = http.send(interacting.complete(&replying, &projects)).await;
                if let Err(trouble) = sent
                    .map_err(|error| error.to_string())
                    .and_then(|response| discord::took(&response))
                {
                    self.said(name, format!("completions could not be sent: {trouble}"));
                }
            }
            discord::Received::Ping | discord::Received::Ignored => {}
        }
    }

    async fn socket<So: Sockets>(
        &self,
        sockets: &So,
        name: &DestinationName,
        interacting: &slack::Interacting,
        app: &herald_ports::Secret,
        opening: &H,
    ) {
        let mut backoff = Backoff::new();
        let mut answering: Answering<'s> = FuturesUnordered::new();
        loop {
            let url = match opening.send(interacting.opening(app)).await {
                Ok(response) => slack::opened(&response),
                Err(error) => Err(error.to_string()),
            };
            let opened = match url {
                Ok(url) => sockets.open(&url).await.map_err(|error| error.to_string()),
                Err(trouble) => Err(trouble),
            };
            let mut socket = match opened {
                Ok(socket) => socket,
                Err(trouble) => {
                    self.said(
                        name,
                        format!("Slack's Socket Mode could not be opened: {trouble}"),
                    );
                    backoff.wait().await;
                    continue;
                }
            };
            backoff.reset();
            loop {
                tokio::select! {
                    frame = socket.receive() => {
                        let Frame::Text(text) = frame else {
                            break;
                        };
                        let enveloped = interacting.envelope(name, &text);
                        if let Some(acknowledgement) = enveloped.acknowledgement
                            && socket.send(Text::plain(acknowledgement)).await.is_err()
                        {
                            break;
                        }
                        if let Some(slack::Received::Asked { asked, replying }) = enveloped.received {
                            self.answer(&mut answering, name, asked, Replying::Slack(replying));
                        }
                        if enveloped.reconnect {
                            break;
                        }
                    }
                    Some(worked) = answering.next(), if !answering.is_empty() => self.worked(Some(worked)),
                }
            }
        }
    }

    async fn sync(&self, name: &DestinationName, interacting: &matrix::Interacting, http: &H) {
        let mut backoff = Backoff::new();
        let mut answering: Answering<'s> = FuturesUnordered::new();
        let mut since: Option<String> = None;
        loop {
            let wait = if since.is_some() { SYNC_WAIT } else { 0 };
            let mut sending = pin!(http.send(interacting.sync(since.as_deref(), wait)));
            let synced = loop {
                tokio::select! {
                    synced = &mut sending => break synced,
                    Some(worked) = answering.next(), if !answering.is_empty() => self.worked(Some(worked)),
                }
            };
            let read = synced
                .map_err(|error| error.to_string())
                .and_then(|response| {
                    matrix::took(&response)?;
                    interacting
                        .synced(name, &response.body)
                        .map_err(|unread| unread.to_string())
                });
            match read {
                Ok(read) => {
                    backoff.reset();
                    if since.is_some() {
                        for (asked, replying) in read.asked {
                            self.answer(&mut answering, name, asked, Replying::Matrix(replying));
                        }
                    }
                    since = Some(read.next);
                }
                Err(trouble) => {
                    self.said(
                        name,
                        format!("the homeserver could not be synced: {trouble}"),
                    );
                    backoff.wait().await;
                }
            }
        }
    }
}

/// Beats every `every`, the first a beat from now.
fn beating(every: jiff::SignedDuration) -> Interval {
    let every = Duration::try_from(every)
        .unwrap_or(FIRST_WAIT)
        .max(FIRST_WAIT);
    let mut beats = interval_at(Instant::now() + every, every);
    beats.set_missed_tick_behavior(MissedTickBehavior::Delay);
    beats
}

/// The next beat, or never where none is due.
async fn beat(beats: &mut Option<Interval>) {
    match beats {
        Some(beats) => {
            beats.tick().await;
        }
        None => std::future::pending().await,
    }
}
