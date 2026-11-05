//! The sessions `herald server` holds for the destinations that carry
//! commands without posting them to herald (ADR-0016): Discord's gateway,
//! Slack's Socket Mode and a Matrix sync. Each takes up a command as it
//! comes, deferring it where Discord waits for that, and queues its work
//! with every other request's, answered within the server's limits. A
//! session reconnects after a lost connection, waiting longer each time it
//! connects and is not taken up.

use std::future::Future;
use std::pin::{Pin, pin};
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use herald_core::{Asked, DestinationName};
use herald_dest_discord::{self as discord, Effect, Gateway};
use herald_dest_matrix::{self as matrix, SYNC_WAIT};
use herald_dest_slack::{self as slack};
use herald_flows::{Interacting, Job, Replying, Serving, completed, deferral};
use herald_ports::{Frame, Http, Socket, Sockets, Store, Text};
use tokio::time::{Instant, Interval, MissedTickBehavior, interval_at, sleep};

use crate::{Heard, Queue};

/// The first wait after a session fails to connect.
const FIRST_WAIT: Duration = Duration::from_secs(1);
/// The longest wait between attempts.
const LONGEST_WAIT: Duration = Duration::from_secs(60);

/// What a session does as it takes commands up: deferring each and queueing
/// its work, and sending completions.
type Taking<'a> = FuturesUnordered<Pin<Box<dyn Future<Output = ()> + 'a>>>;

/// How long a session waits before it connects again, doubling each time it
/// is not taken up, up to [`LONGEST_WAIT`].
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

/// `doing`, done while what `taking` holds goes on.
async fn meanwhile<T>(taking: &mut Taking<'_>, doing: impl Future<Output = T>) -> T {
    let mut doing = pin!(doing);
    loop {
        tokio::select! {
            biased;
            done = &mut doing => return done,
            Some(()) = taking.next(), if !taking.is_empty() => {}
        }
    }
}

/// What every session shares: the server's ports, the queue its commands'
/// work joins, and where it says what happened.
pub(crate) struct Session<'s, 'a, S, C, G, V, H, P, K> {
    pub(crate) serving: &'s Serving<'a, S, C, G, V, H, P, K>,
    pub(crate) queue: &'s Queue,
    pub(crate) heard: &'s dyn Fn(Heard),
}

impl<'s, S: Store, C, G, V, H: Http, P, K> Session<'s, '_, S, C, G, V, H, P, K> {
    fn said(&self, destination: &DestinationName, what: impl Into<String>) {
        (self.heard)(Heard::Session {
            destination: destination.clone(),
            said: what.into(),
        });
    }

    /// Takes `asked` up: defers it where its destination waits for that, and
    /// queues its work, or says it cannot where the queue is full.
    fn take(
        &self,
        taking: &mut Taking<'s>,
        destination: &DestinationName,
        asked: Asked,
        replying: Replying,
    ) {
        let Some(held) = self.queue.hold() else {
            self.said(
                destination,
                format!(
                    "{} was not answered: the server answers no more at once",
                    asked.command.name()
                ),
            );
            return;
        };
        let serving = self.serving;
        let heard = self.heard;
        let destination = destination.clone();
        taking.push(Box::pin(async move {
            if let Err(trouble) = deferral(serving.connected, &destination, &replying, &asked).await
            {
                heard(Heard::Session {
                    said: format!(
                        "{} was not answered: it could not be deferred: {trouble}",
                        asked.command.name()
                    ),
                    destination,
                });
                return;
            }
            held.fill(Job::Interact {
                destination,
                asked: vec![(asked, replying)],
            });
        }));
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
        interacting: &'s discord::Interacting,
        mut gateway: Gateway,
        http: &'s H,
    ) {
        let mut backoff = Backoff::new();
        let mut taking: Taking<'s> = FuturesUnordered::new();
        loop {
            let mut socket = match meanwhile(&mut taking, sockets.open(&gateway.url())).await {
                Ok(socket) => socket,
                Err(error) => {
                    self.said(
                        name,
                        format!("Discord's gateway could not be reached: {error}"),
                    );
                    meanwhile(&mut taking, backoff.wait()).await;
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
                                Effect::Beat(every) => beats = Some(beating(every)),
                                Effect::Ready => backoff.reset(),
                                Effect::Received(received) => {
                                    self.discord(&mut taking, name, interacting, http, *received);
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
                    Some(()) = taking.next(), if !taking.is_empty() => {}
                }
            };
            if let Some(reason) = stopped {
                self.said(name, reason);
                while taking.next().await.is_some() {}
                return;
            }
            meanwhile(&mut taking, backoff.wait()).await;
        }
    }

    fn discord(
        &self,
        taking: &mut Taking<'s>,
        name: &DestinationName,
        interacting: &'s discord::Interacting,
        http: &'s H,
        received: discord::Received,
    ) {
        match received {
            discord::Received::Asked { asked, replying } => {
                self.take(taking, name, asked, Replying::Discord(replying));
            }
            discord::Received::Completing {
                place,
                typed,
                replying,
            } => {
                let serving = self.serving;
                let heard = self.heard;
                let name = name.clone();
                taking.push(Box::pin(async move {
                    let projects =
                        completed(serving.config, serving.ports.store, &name, &place, &typed).await;
                    let sent = http.send(interacting.complete(&replying, &projects)).await;
                    if let Err(trouble) = sent
                        .map_err(|error| error.to_string())
                        .and_then(|response| discord::took(&response))
                    {
                        heard(Heard::Session {
                            destination: name,
                            said: format!("completions could not be sent: {trouble}"),
                        });
                    }
                }));
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
        let mut taking: Taking<'s> = FuturesUnordered::new();
        loop {
            let opened = meanwhile(&mut taking, async {
                let url = match opening.send(interacting.opening(app)).await {
                    Ok(response) => slack::opened(&response)?,
                    Err(error) => return Err(error.to_string()),
                };
                sockets.open(&url).await.map_err(|error| error.to_string())
            })
            .await;
            match opened {
                Ok(mut socket) => loop {
                    tokio::select! {
                        biased;
                        frame = socket.receive() => {
                            let Frame::Text(text) = frame else {
                                break;
                            };
                            let enveloped = interacting.envelope(name, &text);
                            if enveloped.greeted {
                                backoff.reset();
                            }
                            if let Some(acknowledgement) = enveloped.acknowledgement
                                && socket.send(Text::plain(acknowledgement)).await.is_err()
                            {
                                break;
                            }
                            if let Some(slack::Received::Asked { asked, replying }) = enveloped.received {
                                self.take(&mut taking, name, *asked, Replying::Slack(replying));
                            }
                            if enveloped.reconnect {
                                break;
                            }
                        }
                        Some(()) = taking.next(), if !taking.is_empty() => {}
                    }
                },
                Err(trouble) => self.said(
                    name,
                    format!("Slack's Socket Mode could not be opened: {trouble}"),
                ),
            }
            meanwhile(&mut taking, backoff.wait()).await;
        }
    }

    async fn sync(&self, name: &DestinationName, interacting: &matrix::Interacting, http: &H) {
        let mut backoff = Backoff::new();
        let mut taking: Taking<'s> = FuturesUnordered::new();
        let mut since: Option<String> = None;
        loop {
            let wait = if since.is_some() { SYNC_WAIT } else { 0 };
            let synced = meanwhile(
                &mut taking,
                http.send(interacting.sync(since.as_deref(), wait)),
            )
            .await;
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
                            self.take(&mut taking, name, asked, Replying::Matrix(replying));
                        }
                    }
                    since = Some(read.next);
                }
                Err(trouble) => {
                    self.said(
                        name,
                        format!("the homeserver could not be synced: {trouble}"),
                    );
                    meanwhile(&mut taking, backoff.wait()).await;
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
