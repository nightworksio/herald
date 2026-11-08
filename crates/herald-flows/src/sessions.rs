//! The sessions a destination holds where it carries commands without
//! posting them to herald (ADR-0016, ADR-0017): Discord's gateway, Slack's
//! Socket Mode and a Matrix sync, held by `herald server` and by the
//! Worker's Durable Object alike. Each takes up a command as it comes,
//! deferring it where Discord waits for that, and hands its work to a
//! [`Taker`]. A session reconnects after a lost connection, waiting longer
//! each time it connects and is not taken up.

use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::task::Poll;
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use herald_config::Reconnecting;
use herald_core::{Asked, DestinationName};
use herald_dest_discord::{self as discord, Effect, Gateway};
use herald_dest_matrix::{self as matrix, SYNC_WAIT};
use herald_dest_slack::{self as slack};
use herald_ports::{Clock, Frame, Http, Secret, Socket, Sockets, Store, Text, Timer};

use crate::interact::{completed, deferral};
use crate::interacting::{Interacting, Replying};
use crate::room::Room;
use crate::serving::{Job, Serving};

/// The shortest a heartbeat's interval is taken to be.
const LEAST_BEAT: Duration = Duration::from_secs(1);
/// The nanoseconds in a second, which a jittered wait's seed counts.
const NANOS: u32 = 1_000_000_000;

/// What a session does as it takes commands up: deferring each and handing
/// on its work, and sending completions.
type Taking<'a> = FuturesUnordered<Pin<Box<dyn Future<Output = ()> + 'a>>>;

/// Where a session hands the work of the commands it takes up, and where it
/// says what it came to.
pub trait Taker {
    /// A place held for one command's work.
    type Place;

    /// A place for one more command's work, or none where there is no
    /// room for it.
    fn place(&self) -> Option<Self::Place>;

    /// Fills `place` with `job`.
    fn fill(&self, place: Self::Place, job: Job);

    /// Says what a session of `destination` came to.
    fn said(&self, destination: &DestinationName, said: String);
}

/// How long a session waits before it connects again, doubling each time it
/// is not taken up, up to the longest wait `reconnecting` sets.
struct Backoff {
    now: Duration,
    first: Duration,
    longest: Duration,
    jitter: bool,
}

impl Backoff {
    fn new(reconnecting: &Reconnecting) -> Self {
        let first = Duration::try_from(reconnecting.first_wait).unwrap_or(LEAST_BEAT);
        Self {
            now: first,
            first,
            longest: Duration::try_from(reconnecting.longest_wait)
                .unwrap_or(first)
                .max(first),
            jitter: reconnecting.jitter,
        }
    }

    /// The wait now: all of it, or with jitter between half of it and all
    /// of it, as far along as `seed` nanoseconds are of a second.
    fn pause(&self, seed: u32) -> Duration {
        if !self.jitter {
            return self.now;
        }
        let half = self.now / 2;
        let along = u128::from(seed.min(NANOS - 1));
        let spread = along * half.as_nanos() / u128::from(NANOS);
        half + Duration::from_nanos(u64::try_from(spread).unwrap_or(u64::MAX))
    }

    async fn wait<T: Timer>(&mut self, timer: &T, seed: u32) {
        timer.sleep(self.pause(seed)).await;
        self.now = (self.now * 2).min(self.longest);
    }

    fn reset(&mut self) {
        self.now = self.first;
    }
}

/// `doing`, done while what `taking` holds goes on.
async fn meanwhile<T>(taking: &mut Taking<'_>, doing: impl Future<Output = T>) -> T {
    let mut doing = pin!(doing);
    poll_fn(|context| {
        if let Poll::Ready(done) = doing.as_mut().poll(context) {
            return Poll::Ready(done);
        }
        while let Poll::Ready(Some(())) = taking.poll_next_unpin(context) {}
        Poll::Pending
    })
    .await
}

/// A gateway's destination, as a session joined to it needs it.
struct Joined<'a, 's, H> {
    name: &'a DestinationName,
    interacting: &'s discord::Interacting,
    http: &'s H,
}

/// What a gateway session heard first.
enum Heard {
    Frame(Frame),
    Beat,
}

/// What every session shares: the ports, where its commands' work goes,
/// and what it waits with.
pub struct Sessions<'s, 'a, S, C, G, V, H, P, K, T, Ti> {
    serving: &'s Serving<'a, S, C, G, V, H, P, K>,
    taker: &'s T,
    timer: &'s Ti,
    completing: Room,
}

impl<'s, 'a, S, C, G, V, H, P, K, T, Ti> Sessions<'s, 'a, S, C, G, V, H, P, K, T, Ti>
where
    S: Store,
    C: Clock,
    H: Http,
    T: Taker,
    Ti: Timer,
{
    /// The sessions `serving`'s destinations hold, handing their work to
    /// `taker` and waiting with `timer`.
    pub fn new(serving: &'s Serving<'a, S, C, G, V, H, P, K>, taker: &'s T, timer: &'s Ti) -> Self {
        Self {
            serving,
            taker,
            timer,
            completing: Room::new(serving.config.server.completions),
        }
    }

    /// Where between half of a wait and all of it this one falls, from the
    /// clock's nanoseconds.
    fn seed(&self) -> u32 {
        self.serving
            .ports
            .clock
            .now()
            .subsec_nanosecond()
            .unsigned_abs()
    }

    fn said(&self, destination: &DestinationName, what: impl Into<String>) {
        self.taker.said(destination, what.into());
    }

    /// Takes `asked` up: defers it where its destination waits for that, and
    /// hands on its work, or says it cannot where there is no room for it.
    fn take(
        &self,
        taking: &mut Taking<'s>,
        destination: &DestinationName,
        asked: Asked,
        replying: Replying,
    ) {
        let Some(place) = self.taker.place() else {
            self.said(
                destination,
                format!(
                    "{} was not answered: no more is answered at once",
                    asked.command.name()
                ),
            );
            return;
        };
        let serving = self.serving;
        let taker = self.taker;
        let destination = destination.clone();
        taking.push(Box::pin(async move {
            if let Err(trouble) = deferral(serving.connected, &destination, &replying, &asked).await
            {
                taker.said(
                    &destination,
                    format!(
                        "{} was not answered: it could not be deferred: {trouble}",
                        asked.command.name()
                    ),
                );
                return;
            }
            taker.fill(
                place,
                Job::Interact {
                    destination,
                    asked: vec![(asked, replying)],
                },
            );
        }));
    }

    /// Every session the destinations hold, opened through `sockets`, until
    /// each stops.
    pub async fn held<So: Sockets>(&self, sockets: &So) {
        let mut sessions: FuturesUnordered<Pin<Box<dyn Future<Output = ()> + '_>>> =
            FuturesUnordered::new();
        for (name, interacting, http, opening) in self.serving.connected.interactions() {
            match interacting {
                Interacting::Discord(discord)
                    if discord.via() == herald_dest_discord::Via::Gateway =>
                {
                    if let Some(bot) = discord.bot() {
                        let gateway = Gateway::new(bot.clone(), &discord.session_domains());
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
        let mut backoff = Backoff::new(&self.serving.config.server.sessions);
        let mut taking: Taking<'s> = FuturesUnordered::new();
        loop {
            let mut socket = match meanwhile(&mut taking, sockets.open(&gateway.url())).await {
                Ok(socket) => socket,
                Err(error) => {
                    self.said(
                        name,
                        format!("Discord's gateway could not be reached: {error}"),
                    );
                    meanwhile(&mut taking, backoff.wait(self.timer, self.seed())).await;
                    continue;
                }
            };
            let joined = Joined {
                name,
                interacting,
                http,
            };
            let stopped = self
                .joined(
                    &joined,
                    &mut socket,
                    &mut gateway,
                    &mut backoff,
                    &mut taking,
                )
                .await;
            if let Some(reason) = stopped {
                self.said(name, reason);
                while taking.next().await.is_some() {}
                return;
            }
            meanwhile(&mut taking, backoff.wait(self.timer, self.seed())).await;
        }
    }

    /// Hears a joined gateway until it is left: `None` to join again, or why
    /// the session stops.
    async fn joined<So: Socket>(
        &self,
        joined: &Joined<'_, 's, H>,
        socket: &mut So,
        gateway: &mut Gateway,
        backoff: &mut Backoff,
        taking: &mut Taking<'s>,
    ) -> Option<String> {
        let mut every: Option<Duration> = None;
        let mut beat: Option<Pin<Box<dyn Future<Output = ()> + Send + 's>>> = None;
        loop {
            let heard = {
                let mut receiving = pin!(socket.receive());
                poll_fn(|context| {
                    if let Poll::Ready(frame) = receiving.as_mut().poll(context) {
                        return Poll::Ready(Heard::Frame(frame));
                    }
                    if let Some(due) = beat.as_mut()
                        && due.as_mut().poll(context).is_ready()
                    {
                        return Poll::Ready(Heard::Beat);
                    }
                    while let Poll::Ready(Some(())) = taking.poll_next_unpin(context) {}
                    Poll::Pending
                })
                .await
            };
            let effects = match heard {
                Heard::Frame(Frame::Text(text)) => {
                    gateway.received(joined.interacting, joined.name, &text)
                }
                Heard::Frame(Frame::Closed(code)) => vec![gateway.closed(code)],
                Heard::Beat => {
                    beat = every.map(|every| Box::pin(self.timer.sleep(every)) as _);
                    vec![gateway.beat()]
                }
            };
            let mut next = None;
            for effect in effects {
                match effect {
                    Effect::Send(frame) => {
                        if socket.send(frame).await.is_err() {
                            next = Some(None);
                        }
                    }
                    Effect::Beat(interval) => {
                        let interval = beating(interval);
                        every = Some(interval);
                        beat = Some(Box::pin(self.timer.sleep(interval)));
                    }
                    Effect::Ready => backoff.reset(),
                    Effect::Received(received) => {
                        self.discord(
                            taking,
                            joined.name,
                            joined.interacting,
                            joined.http,
                            *received,
                        );
                    }
                    Effect::Reconnect { .. } => next = Some(None),
                    Effect::Stop(reason) => next = Some(Some(reason)),
                }
            }
            if let Some(next) = next {
                return next;
            }
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
                let Some(completion) = self.completing.place() else {
                    self.said(
                        name,
                        "a completion was dropped: no more are answered at once",
                    );
                    return;
                };
                let serving = self.serving;
                let taker = self.taker;
                let name = name.clone();
                taking.push(Box::pin(async move {
                    let _completion = completion;
                    let projects =
                        completed(serving.config, serving.ports.store, &name, &place, &typed).await;
                    let sent = http.send(interacting.complete(&replying, &projects)).await;
                    if let Err(trouble) = sent
                        .map_err(|error| error.to_string())
                        .and_then(|response| discord::took(&response))
                    {
                        taker.said(&name, format!("completions could not be sent: {trouble}"));
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
        app: &Secret,
        opening: &H,
    ) {
        let mut backoff = Backoff::new(&self.serving.config.server.sessions);
        let mut taking: Taking<'s> = FuturesUnordered::new();
        loop {
            let opened = meanwhile(&mut taking, async {
                let url = match opening.send(interacting.opening(app)).await {
                    Ok(response) => slack::opened(&response, &interacting.session_domains())?,
                    Err(error) => return Err(error.to_string()),
                };
                sockets.open(&url).await.map_err(|error| error.to_string())
            })
            .await;
            match opened {
                Ok(mut socket) => loop {
                    let frame = {
                        let receiving = socket.receive();
                        meanwhile(&mut taking, receiving).await
                    };
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
                },
                Err(trouble) => self.said(
                    name,
                    format!("Slack's Socket Mode could not be opened: {trouble}"),
                ),
            }
            meanwhile(&mut taking, backoff.wait(self.timer, self.seed())).await;
        }
    }

    async fn sync(&self, name: &DestinationName, interacting: &matrix::Interacting, http: &H) {
        let mut backoff = Backoff::new(&self.serving.config.server.sessions);
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
                    meanwhile(&mut taking, backoff.wait(self.timer, self.seed())).await;
                }
            }
        }
    }
}

/// How often to beat for `every`, Discord's interval: at least
/// [`LEAST_BEAT`].
fn beating(every: jiff::SignedDuration) -> Duration {
    Duration::try_from(every)
        .unwrap_or(LEAST_BEAT)
        .max(LEAST_BEAT)
}

#[cfg(test)]
mod tests {
    use herald_testing::FakeTimer;

    use super::*;

    fn reconnecting(jitter: bool) -> Reconnecting {
        Reconnecting {
            first_wait: jiff::SignedDuration::from_secs(2),
            longest_wait: jiff::SignedDuration::from_secs(5),
            jitter,
        }
    }

    #[test]
    fn a_wait_doubles_up_to_the_longest_and_starts_again_once_taken_up() {
        let timer = FakeTimer::default();
        let mut backoff = Backoff::new(&reconnecting(false));
        pollster::block_on(async {
            for _ in 0..3 {
                backoff.wait(&timer, 0).await;
            }
            backoff.reset();
            backoff.wait(&timer, 0).await;
        });
        assert_eq!(
            timer.waited(),
            [2, 4, 5, 2].map(Duration::from_secs).to_vec()
        );
    }

    #[test]
    fn a_jittered_wait_falls_between_half_of_it_and_all_of_it() {
        let backoff = Backoff::new(&reconnecting(true));
        assert_eq!(backoff.pause(0), Duration::from_secs(1));
        assert_eq!(backoff.pause(NANOS / 2), Duration::from_millis(1_500));
        assert_eq!(backoff.pause(u32::MAX), Duration::from_nanos(1_999_999_999));
        assert_eq!(
            Backoff::new(&reconnecting(false)).pause(NANOS / 2),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn a_heartbeat_is_at_least_a_second_apart() {
        assert_eq!(beating(jiff::SignedDuration::from_millis(10)), LEAST_BEAT);
        assert_eq!(
            beating(jiff::SignedDuration::from_secs(41)),
            Duration::from_secs(41)
        );
        assert_eq!(beating(jiff::SignedDuration::from_secs(-1)), LEAST_BEAT);
    }
}
