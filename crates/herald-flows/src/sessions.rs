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
use herald_core::{Asked, DestinationName, Place};
use herald_kinds::{Connect, Effect, Interacted, Replying, Session};
use herald_ports::{Clock, Frame, Http, Socket, Sockets, Store, Timer};

use crate::interact::{completed, deferral};
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

    async fn wait<T: Timer>(&mut self, timer: &T, clock: &impl Clock) {
        timer.sleep(self.pause(seed(clock))).await;
        self.now = (self.now * 2).min(self.longest);
    }

    fn reset(&mut self) {
        self.now = self.first;
    }
}

/// Where between half of a wait and all of it this one falls, from
/// `clock`'s nanoseconds.
fn seed(clock: &impl Clock) -> u32 {
    clock.now().subsec_nanosecond().unsigned_abs()
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

/// A socket session's destination, as hearing it needs it.
struct Hearing<'a, 's, H> {
    name: &'a DestinationName,
    interacting: &'s Interacted,
    http: &'s H,
}

/// What a socket session heard first.
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
            if let Some(session) = interacting.session() {
                sessions.push(Box::pin(self.hold(
                    sockets,
                    name,
                    interacting,
                    session,
                    http,
                    opening,
                )));
            }
        }
        while sessions.next().await.is_some() {}
    }

    /// Holds `session` of the destination `name` until it stops,
    /// connecting again after each connection it loses.
    async fn hold<So: Sockets>(
        &self,
        sockets: &So,
        name: &DestinationName,
        interacting: &'s Interacted,
        mut session: Box<dyn Session + 's>,
        http: &'s H,
        opening: Option<&H>,
    ) {
        let mut backoff = Backoff::new(&self.serving.config.server.sessions);
        let mut taking: Taking<'s> = FuturesUnordered::new();
        loop {
            let url = match session.connect() {
                Connect::Poll(request) => {
                    let polled = meanwhile(&mut taking, http.send(request))
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|response| session.polled(name, &response));
                    match polled {
                        Ok(effects) => {
                            for effect in effects {
                                if let Effect::Asked(asked, replying) = effect {
                                    self.take(&mut taking, name, asked, replying);
                                } else if effect == Effect::Ready {
                                    backoff.reset();
                                }
                            }
                            continue;
                        }
                        Err(trouble) => Err(trouble),
                    }
                }
                Connect::Socket(url) => Ok(url),
                Connect::Opening(request) => {
                    meanwhile(&mut taking, opening.unwrap_or(http).send(request))
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|response| session.opened(&response))
                }
            };
            let socket = match url {
                Ok(url) => meanwhile(&mut taking, sockets.open(&url))
                    .await
                    .map_err(|error| error.to_string()),
                Err(trouble) => Err(trouble),
            };
            match socket {
                Ok(mut socket) => {
                    let heard = Hearing {
                        name,
                        interacting,
                        http,
                    };
                    let stopped = self
                        .heard(
                            &heard,
                            &mut socket,
                            &mut *session,
                            &mut backoff,
                            &mut taking,
                        )
                        .await;
                    if let Some(reason) = stopped {
                        self.said(name, reason);
                        while taking.next().await.is_some() {}
                        return;
                    }
                }
                Err(trouble) => {
                    self.said(name, format!("{}: {trouble}", session.unreachable()));
                }
            }
            meanwhile(
                &mut taking,
                backoff.wait(self.timer, self.serving.ports.clock),
            )
            .await;
        }
    }

    /// Hears an open socket until it is left: `None` to connect again, or
    /// why the session stops.
    async fn heard<So: Socket>(
        &self,
        hearing: &Hearing<'_, 's, H>,
        socket: &mut So,
        session: &mut (dyn Session + 's),
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
                Heard::Frame(Frame::Text(text)) => session.heard(hearing.name, &text),
                Heard::Frame(Frame::Closed(code)) => session.closed(code),
                Heard::Beat => {
                    beat = every.map(|every| Box::pin(self.timer.sleep(every)) as _);
                    session.beat()
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
                    Effect::Acknowledge(frame) => {
                        if socket.send(frame).await.is_err() {
                            next = Some(None);
                            break;
                        }
                    }
                    Effect::Beat(interval) => {
                        let interval = beating(interval);
                        every = Some(interval);
                        beat = Some(Box::pin(self.timer.sleep(interval)));
                    }
                    Effect::Ready => backoff.reset(),
                    Effect::Asked(asked, replying) => {
                        self.take(taking, hearing.name, asked, replying);
                    }
                    Effect::Completing {
                        place,
                        typed,
                        replying,
                    } => self.completion(taking, hearing, place, typed, replying),
                    Effect::Reconnect => next = Some(None),
                    Effect::Stop(reason) => next = Some(Some(reason)),
                }
            }
            if let Some(next) = next {
                return next;
            }
        }
    }

    /// Offers the projects announced at `place` that start with `typed`,
    /// where `replying` names, or says it cannot where there is no room.
    fn completion(
        &self,
        taking: &mut Taking<'s>,
        hearing: &Hearing<'_, 's, H>,
        place: Place,
        typed: String,
        replying: Replying,
    ) {
        let Some(completion) = self.completing.place() else {
            self.said(
                hearing.name,
                "a completion was dropped: no more are answered at once",
            );
            return;
        };
        let serving = self.serving;
        let taker = self.taker;
        let name = hearing.name.clone();
        let interacting = hearing.interacting;
        let http = hearing.http;
        taking.push(Box::pin(async move {
            let _completion = completion;
            let projects =
                completed(serving.config, serving.ports.store, &name, &place, &typed).await;
            if let Some(request) = interacting.complete(&replying, &projects)
                && let Err(trouble) = http
                    .send(request)
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|response| interacting.took(&response))
            {
                taker.said(&name, format!("completions could not be sent: {trouble}"));
            }
        }));
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
    use herald_testing::{FakeClock, FakeTimer};

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
        let clock = FakeClock::at("2027-01-15T08:00:00.5Z".parse().unwrap());
        let mut backoff = Backoff::new(&reconnecting(false));
        pollster::block_on(async {
            for _ in 0..3 {
                backoff.wait(&timer, &clock).await;
            }
            backoff.reset();
            backoff.wait(&timer, &clock).await;
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
    fn a_wait_s_seed_is_the_nanoseconds_of_the_clock_s_second() {
        let clock = FakeClock::at("2027-01-15T08:00:00.25Z".parse().unwrap());
        assert_eq!(seed(&clock), 250_000_000);
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
