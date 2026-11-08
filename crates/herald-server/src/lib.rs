//! `herald server` (ADR-0002, layer 3): the HTTP endpoint every native
//! driver serves.
//!
//! [`serve`] answers each request through [`herald_flows::answer`] on one
//! task: forge webhooks, commands, herald's ActivityPub actors and their
//! inboxes, and health. The work a request or a command a session carries
//! leaves runs once it is answered, at most [`Limits::jobs`] at once, with
//! as many again waiting. A request is read and verified before its work
//! takes a place; work the queue has no place for is released and answered
//! 503. At most [`Limits::connections`] are held, each for one request whose
//! headers and body are read in time.
//! One round of each kind runs at a time. Sources are polled, threads kept and
//! keys forgotten on the config's schedule. TLS is a reverse proxy's.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::{Pin, pin};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use herald_core::DestinationName;
use herald_flows::{
    Answered, Housekept, Inbound, Job, Round, Running, Serving, Sessions, Stop, Swept, Worked,
    answer, busy, housekeep, poll, refusal, unworked, work,
};
use herald_native::TokioTimer as Waiting;
use herald_ports::{Clock, Http, Keyring, Secret, Signer, Sockets, Store, Verifier};
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue, RETRY_AFTER};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use jiff::Timestamp;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::time::{Instant, Interval, MissedTickBehavior, interval_at};

use crate::sessions::Queued;

mod sessions;

/// The name the server takes its leases under.
pub(crate) const RUN: &str = "server";

/// The name a job of this server takes its leases under: `server`, then a
/// nonce no other job takes (ADR-0005).
fn named() -> String {
    format!("{RUN}/{}", herald_environment::nonce())
}

/// What a reply in words is.
const TEXT: &str = "text/plain; charset=utf-8";
/// What an answer to a host asking for a round is.
const JSON: &str = "application/json";

/// What the server heard, for its driver to say.
#[derive(Debug)]
pub enum Heard {
    /// It listens at this address.
    Listening(SocketAddr),
    /// A request's work was done.
    Worked(Worked),
    /// The sources were polled.
    Polled(Vec<Swept>),
    /// The threads whose chores fell due were kept.
    Kept(Result<Housekept, Stop>),
    /// A connection could not be taken, and why.
    Unaccepted(String),
    /// A request was refused, and why; its reply named no reason.
    Refused(String),
    /// A session a destination holds said this.
    Session {
        /// The destination.
        destination: DestinationName,
        /// What it said.
        said: String,
    },
}

/// What the server takes on at once, and how often it does its rounds.
#[derive(Debug, Clone, Copy)]
pub struct Limits<'a> {
    /// How many pieces of work run at once.
    pub jobs: usize,
    /// How many pieces of work may wait for a place.
    pub queue: usize,
    /// How many connections are held at once; the next waits to be taken.
    pub connections: usize,
    /// The most bytes a request's body may hold.
    pub body: usize,
    /// How often sources are polled, where they are.
    pub poll: Option<Duration>,
    /// How often threads are kept, where they are.
    pub housekeep: Option<Duration>,
    /// How often keys read longer ago than this are forgotten.
    pub keys: Duration,
    /// How long a connection may take to send a request's headers, and then
    /// its body.
    pub read: Duration,
    /// How long a connection is held at most, its one request answered.
    pub held: Duration,
    /// How long the server waits before taking connections again after one
    /// could not be taken, so a lack of file descriptors does not spin it.
    pub accept_pause: Duration,
    /// Where a host's own timers ask for rounds, where they do.
    pub asked: Option<Asked<'a>>,
    /// The secret a request for a round at `/herald/round/<kind>` carries;
    /// with none, no request runs a round.
    pub secret: Option<&'a Secret>,
}

/// Where a host asks for rounds by posting to herald, as Azure Functions'
/// timer triggers post to their functions, and what it is answered once a
/// round is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asked<'a> {
    /// The path a poll is asked for at.
    pub poll: &'static str,
    /// The path housekeeping is asked for at; keys read longest ago are
    /// forgotten with it.
    pub housekeep: &'static str,
    /// What the host is answered, as JSON.
    pub reply: &'static str,
    /// The first segment of every path the host forwards from outside, so
    /// no request from outside reaches the timers' paths.
    pub prefix: &'a str,
}

impl Asked<'_> {
    /// The round the host's timer asks for at `path`, where it asks for one.
    fn round(&self, path: &str) -> Option<Round> {
        if path == self.poll {
            Some(Round::Poll)
        } else if path == self.housekeep {
            Some(Round::Housekeep)
        } else {
            None
        }
    }

    /// `path` from outside within [`Asked::prefix`], the prefix taken off;
    /// none where it lies outside it.
    fn within<'p>(&self, path: &'p str) -> Option<&'p str> {
        let rest = path.strip_prefix('/')?.strip_prefix(self.prefix)?;
        match rest {
            "" => Some("/"),
            rest if rest.starts_with('/') => Some(rest),
            _ => None,
        }
    }
}

/// Where connections come from.
pub trait Listener {
    /// A connection.
    type Stream: AsyncRead + AsyncWrite + Unpin;

    /// The next connection.
    fn take(&self) -> impl Future<Output = io::Result<Self::Stream>>;

    /// Where it listens.
    ///
    /// # Errors
    ///
    /// When the address cannot be read.
    fn address(&self) -> io::Result<SocketAddr>;
}

impl Listener for TcpListener {
    type Stream = TcpStream;

    async fn take(&self) -> io::Result<TcpStream> {
        self.accept().await.map(|(stream, _)| stream)
    }

    fn address(&self) -> io::Result<SocketAddr> {
        self.local_addr()
    }
}

/// The work requests left, waiting to run, and the places held for the
/// work of requests still being answered.
struct Queue {
    jobs: RefCell<VecDeque<Job>>,
    held: Cell<usize>,
    most: usize,
    queued: Notify,
}

impl Queue {
    fn new(most: usize) -> Self {
        Self {
            jobs: RefCell::new(VecDeque::new()),
            held: Cell::new(0),
            most,
            queued: Notify::new(),
        }
    }

    /// A place for one more piece of work, where the queue has one.
    fn hold(&self) -> Option<Held<'_>> {
        if self.jobs.borrow().len() + self.held.get() >= self.most {
            return None;
        }
        self.held.set(self.held.get() + 1);
        Some(Held(self))
    }

    fn next(&self) -> Option<Job> {
        self.jobs.borrow_mut().pop_front()
    }

    fn left(&self) -> Vec<Job> {
        self.jobs.borrow_mut().drain(..).collect()
    }
}

/// A place held in the queue, given up when it is filled or dropped.
struct Held<'a>(&'a Queue);

impl Held<'_> {
    fn fill(self, job: Job) {
        self.0.jobs.borrow_mut().push_back(job);
        self.0.queued.notify_one();
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.0.held.set(self.0.held.get().saturating_sub(1));
    }
}

fn replied(status: u16, kind: &'static str, body: Vec<u8>) -> hyper::Response<Full<Bytes>> {
    let mut response = hyper::Response::new(Full::new(Bytes::from(body)));
    *response.status_mut() =
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(kind));
    response
}

/// The reply `answered` gives, a refusal's reason said through `rounds`.
fn sent(rounds: &Rounds<'_>, answered: Answered) -> hyper::Response<Full<Bytes>> {
    if let Some(reason) = answered.refused {
        (rounds.heard)(Heard::Refused(reason));
    }
    let mut response = replied(
        answered.reply.status,
        answered.reply.kind,
        answered.reply.body,
    );
    if let Some(after) = answered.reply.retry_after {
        response
            .headers_mut()
            .insert(RETRY_AFTER, HeaderValue::from(after));
    }
    response
}

/// The reply to `request`, its work left waiting in `queue`: a request
/// that may leave work is answered 503 where the queue has no place for it.
async fn answered<S, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    request: hyper::Request<Incoming>,
    queue: &Queue,
    limits: Limits<'_>,
    rounds: &Rounds<'_>,
) -> Result<hyper::Response<Full<Bytes>>, Infallible>
where
    S: Store,
    C: Clock,
    G: Signer,
    V: Verifier,
    H: Http,
    P: Http,
    K: Keyring,
{
    let posted = request.method() == Method::POST;
    if let Some(asked) = limits.asked
        && posted
        && let Some(round) = asked.round(request.uri().path())
    {
        rounds.ran(serving, round).await;
        return Ok(replied(200, JSON, asked.reply.as_bytes().to_vec()));
    }
    let path = match limits.asked {
        Some(asked) => match asked.within(request.uri().path()) {
            Some(path) => path.to_owned(),
            None => {
                return Ok(sent(
                    rounds,
                    refusal(404, "its path lies outside the host's prefix"),
                ));
            }
        },
        None => request.uri().path().to_owned(),
    };
    if let Some(round) = Round::named(request.method().as_str(), &path) {
        let authorization = request
            .headers()
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        if !Round::allowed(authorization, limits.secret) {
            return Ok(sent(
                rounds,
                refusal(401, "a round was asked without its secret"),
            ));
        }
        rounds.ran(serving, round).await;
        return Ok(replied(200, TEXT, b"the round ran".to_vec()));
    }
    let (parts, body) = request.into_parts();
    let Ok(read) =
        tokio::time::timeout(limits.read, Limited::new(body, limits.body).collect()).await
    else {
        return Ok(sent(rounds, refusal(408, "its body was not sent in time")));
    };
    let Ok(collected) = read else {
        return Ok(sent(
            rounds,
            refusal(
                413,
                format!(
                    "its body is larger than [server] body, {} bytes, or could not be read",
                    limits.body
                ),
            ),
        ));
    };
    let body = collected.to_bytes();
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect();
    let borrowed: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let inbound = Inbound {
        method: parts.method.as_str(),
        path: &path,
        query: parts.uri.query(),
        headers: &borrowed,
        body: &body,
    };
    let mut answered = answer(serving, &inbound).await;
    if let Some(job) = answered.job.take() {
        let Some(held) = queue.hold() else {
            unworked(serving, &job).await;
            return Ok(sent(rounds, busy(serving.config.server.busy_retry)));
        };
        held.fill(job);
    }
    Ok(sent(rounds, answered))
}

/// Which rounds are running, so that one of each kind runs at a time, and
/// where what they did is said.
struct Rounds<'a> {
    polling: AtomicBool,
    keeping: AtomicBool,
    sweeping: AtomicBool,
    heard: &'a dyn Fn(Heard),
}

/// `round` done, where no round `running` marks is.
async fn once(running: &AtomicBool, round: impl Future<Output = Option<Heard>>) -> Option<Heard> {
    let _running = Running::start(running)?;
    round.await
}

impl Rounds<'_> {
    /// A poll, where none is running.
    async fn polled<S: Store, C: Clock, G: Signer, V, H: Http, P, K>(
        &self,
        serving: &Serving<'_, S, C, G, V, H, P, K>,
    ) -> Option<Heard> {
        once(&self.polling, async {
            Some(Heard::Polled(
                poll(serving.config, serving.connected, serving.ports, &named()).await,
            ))
        })
        .await
    }

    /// Housekeeping, where none is running.
    async fn kept<S: Store, C: Clock, G, V, H: Http, P, K>(
        &self,
        serving: &Serving<'_, S, C, G, V, H, P, K>,
    ) -> Option<Heard> {
        let ports = serving.ports;
        once(&self.keeping, async {
            Some(Heard::Kept(
                housekeep(
                    serving.connected,
                    ports.store,
                    ports.clock,
                    &serving.config.retention,
                )
                .await,
            ))
        })
        .await
    }

    /// The keys read longest ago forgotten, where no sweep is running.
    async fn swept<S, C: Clock, G, V, H, P, K: Keyring>(
        &self,
        serving: &Serving<'_, S, C, G, V, H, P, K>,
    ) -> Option<Heard> {
        once(&self.sweeping, async {
            swept(serving).await;
            None
        })
        .await
    }

    /// `round` done, where none of its kind is running, and what it did
    /// said.
    async fn ran<S: Store, C: Clock, G: Signer, V, H: Http, P, K: Keyring>(
        &self,
        serving: &Serving<'_, S, C, G, V, H, P, K>,
        round: Round,
    ) {
        let done = match round {
            Round::Poll => self.polled(serving).await,
            Round::Housekeep => {
                self.swept(serving).await;
                self.kept(serving).await
            }
        };
        if let Some(heard) = done {
            (self.heard)(heard);
        }
    }
}

/// The one request `stream` carries, answered, within `limits.held`.
async fn connection<S, C, G, V, H, P, K, T>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    stream: T,
    queue: &Queue,
    limits: Limits<'_>,
    rounds: &Rounds<'_>,
) where
    T: AsyncRead + AsyncWrite + Unpin,
    S: Store,
    C: Clock,
    G: Signer,
    V: Verifier,
    H: Http,
    P: Http,
    K: Keyring,
{
    let service = service_fn(move |request| answered(serving, request, queue, limits, rounds));
    let served = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(limits.read)
        .keep_alive(false)
        .serve_connection(TokioIo::new(stream), service);
    let _ = tokio::time::timeout(limits.held, served).await;
}

/// A round every `period`, the first a period from now.
fn rounds(period: Option<Duration>) -> Option<Interval> {
    period.filter(|period| !period.is_zero()).map(|period| {
        let mut rounds = interval_at(Instant::now() + period, period);
        rounds.set_missed_tick_behavior(MissedTickBehavior::Delay);
        rounds
    })
}

/// Every key read longer ago than its refresh, forgotten.
async fn swept<S, C: Clock, G, V, H, P, K: Keyring>(serving: &Serving<'_, S, C, G, V, H, P, K>) {
    let now = serving.ports.clock.now();
    let before = now
        .checked_sub(serving.keys.refreshed)
        .unwrap_or(Timestamp::MIN);
    let _ = serving.keys.keyring.forget_before(before).await;
}

/// The next of `rounds`, or never where there are none.
async fn round(rounds: &mut Option<Interval>) {
    match rounds {
        Some(rounds) => {
            rounds.tick().await;
        }
        None => std::future::pending().await,
    }
}

/// Every connection `listener` takes answered through `serving`, within
/// `limits`, each thing worth saying handed to `heard`, until `stop`
/// completes. Then no request is taken, the work waiting is released to be
/// delivered again (ADR-0013), and the work running is finished.
pub async fn serve<S, C, G, V, H, P, K, L, So>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    listener: L,
    sockets: &So,
    limits: Limits<'_>,
    heard: &dyn Fn(Heard),
    stop: impl Future<Output = ()>,
) where
    L: Listener,
    So: Sockets,
    S: Store,
    C: Clock,
    G: Signer,
    V: Verifier,
    H: Http,
    P: Http,
    K: Keyring,
{
    if let Ok(address) = listener.address() {
        heard(Heard::Listening(address));
    }
    let rounding = Rounds {
        polling: AtomicBool::new(false),
        keeping: AtomicBool::new(false),
        sweeping: AtomicBool::new(false),
        heard,
    };
    let queue = Queue::new(limits.queue);
    let mut connections = FuturesUnordered::new();
    let mut running: FuturesUnordered<Pin<Box<dyn Future<Output = Option<Heard>> + '_>>> =
        FuturesUnordered::new();
    let mut polls = rounds(limits.poll);
    let mut chores = rounds(limits.housekeep);
    let mut sweeps = rounds(Some(limits.keys));
    let mut stop = pin!(stop);
    let queued = Queued {
        queue: &queue,
        heard,
    };
    let holding = Sessions::new(serving, &queued, &Waiting);
    let mut sessions = pin!(holding.held(sockets));
    let mut held = true;
    loop {
        tokio::task::coop::consume_budget().await;
        while running.len() < limits.jobs {
            let Some(job) = queue.next() else {
                break;
            };
            running.push(Box::pin(async move {
                Some(Heard::Worked(work(serving, job, &named()).await))
            }));
        }
        tokio::select! {
            () = &mut stop => break,
            accepted = listener.take(), if connections.len() < limits.connections => match accepted {
                Ok(stream) => connections.push(connection(serving, stream, &queue, limits, &rounding)),
                Err(error) => {
                    heard(Heard::Unaccepted(error.to_string()));
                    tokio::time::sleep(limits.accept_pause).await;
                }
            },
            () = queue.queued.notified() => {}
            () = &mut sessions, if held => held = false,
            Some(()) = connections.next(), if !connections.is_empty() => {}
            Some(done) = running.next(), if !running.is_empty() => {
                if let Some(said) = done {
                    heard(said);
                }
            }
            () = round(&mut polls), if !rounding.polling.load(Ordering::Acquire) => {
                running.push(Box::pin(rounding.polled(serving)));
            }
            () = round(&mut chores), if !rounding.keeping.load(Ordering::Acquire) => {
                running.push(Box::pin(rounding.kept(serving)));
            }
            () = round(&mut sweeps), if !rounding.sweeping.load(Ordering::Acquire) => {
                running.push(Box::pin(rounding.swept(serving)));
            }
        }
    }
    drop(connections);
    for job in &queue.left() {
        unworked(serving, job).await;
    }
    while let Some(done) = running.next().await {
        if let Some(said) = done {
            heard(said);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::FutureExt as _;
    use herald_flows::{Job, Verified};

    use super::{Asked, Queue, RUN, named, round, rounds};

    fn job(body: &[u8]) -> Job {
        Job::Follow {
            verified: Verified {
                delivery: None,
                signer: None,
            },
            body: body.to_vec(),
        }
    }

    #[test]
    fn a_path_from_outside_lies_within_the_prefix_or_nowhere() {
        let asked = Asked {
            poll: "/poll",
            housekeep: "/housekeep",
            reply: "{}",
            prefix: "api",
        };
        assert_eq!(asked.within("/api"), Some("/"));
        assert_eq!(asked.within("/api/healthz"), Some("/healthz"));
        assert_eq!(asked.within("/apis/healthz"), None);
        assert_eq!(asked.within("api/healthz"), None);
        assert_eq!(asked.within("/other/healthz"), None);
    }

    #[test]
    fn a_place_held_counts_until_it_is_filled_or_given_up() {
        let queue = Queue::new(2);
        let first = queue.hold().unwrap();
        let second = queue.hold().unwrap();
        assert!(queue.hold().is_none());
        drop(second);
        let third = queue.hold().unwrap();
        assert!(queue.hold().is_none());
        first.fill(job(b"1"));
        assert!(queue.hold().is_none());
        third.fill(job(b"2"));
        assert!(queue.hold().is_none());
        assert_eq!(queue.next(), Some(job(b"1")));
        let fourth = queue.hold().unwrap();
        assert!(queue.hold().is_none());
        drop(fourth);
        assert_eq!(queue.left(), vec![job(b"2")]);
        assert_eq!(queue.next(), None);
        assert!(queue.hold().is_some());
    }

    #[test]
    fn each_job_takes_its_leases_under_the_server_s_name_and_a_nonce_of_its_own() {
        let first = named();
        assert!(first.starts_with(&format!("{RUN}/")), "{first}");
        assert_ne!(named(), first);
    }

    #[test]
    fn a_queue_with_no_places_holds_none() {
        assert!(Queue::new(0).hold().is_none());
    }

    #[test]
    fn no_period_or_a_zero_one_has_no_rounds() {
        assert!(rounds(None).is_none());
        assert!(rounds(Some(Duration::ZERO)).is_none());
    }

    #[test]
    fn without_rounds_a_round_never_comes() {
        assert!(round(&mut None).now_or_never().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn the_first_round_comes_a_period_from_now_and_each_next_a_period_after() {
        let period = Duration::from_secs(60);
        let short = Duration::from_secs(59);
        let mut every = rounds(Some(period));
        assert!(every.is_some());
        assert!(round(&mut every).now_or_never().is_none());
        tokio::time::advance(short).await;
        assert!(round(&mut every).now_or_never().is_none());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(round(&mut every).now_or_never().is_some());
        assert!(round(&mut every).now_or_never().is_none());
        tokio::time::advance(period).await;
        assert!(round(&mut every).now_or_never().is_some());
    }
}
