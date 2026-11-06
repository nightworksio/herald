//! `herald server` (ADR-0002, layer 3): the HTTP endpoint every native
//! driver serves.
//!
//! [`serve`] answers each request through [`herald_flows::answer`] on one
//! task: forge webhooks, commands, herald's ActivityPub actors and their
//! inboxes, and health. The work a request or a command a session carries
//! leaves runs once it is answered, at most [`Limits::jobs`] at once, with
//! as many again waiting; a request that could leave more is answered 503. At most [`Limits::connections`] are
//! held, each for one request whose headers and body are read in time.
//! One round of each kind runs at a time. Sources are polled, threads kept and
//! keys forgotten on the config's schedule. TLS is a reverse proxy's.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::{Pin, pin};
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use herald_core::DestinationName;
use herald_flows::{
    Inbound, Job, Kept, Serving, Sessions, Stop, Swept, Worked, answer, housekeep, poll, unworked,
    work,
};
use herald_native::TokioTimer as Waiting;
use herald_ports::{
    Clock, Http, HttpError, Keyring, Request, Response, Signer, Sockets, Store, Verifier,
};
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{CONTENT_TYPE, HeaderValue, RETRY_AFTER};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use jiff::Timestamp;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, Semaphore};
use tokio::time::{Instant, Interval, MissedTickBehavior, interval_at};

use crate::sessions::Queued;

mod sessions;

/// The name the server takes its leases under.
pub(crate) const RUN: &str = "server";
/// How many seconds a sender is asked to wait when the work queue is full.
const BUSY_RETRY: &str = "30";
/// How long a connection may take to send a request's headers, and then
/// its body, unless its limits say otherwise.
pub const READ: Duration = Duration::from_secs(30);
/// How long a connection is held at most, its one request answered.
const HELD: Duration = Duration::from_secs(90);
/// How long the server waits before taking connections again after one
/// could not be taken, so a lack of file descriptors does not spin it.
const ACCEPT_PAUSE: Duration = Duration::from_millis(100);
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
    Kept(Result<Vec<Kept>, Stop>),
    /// A connection could not be taken, and why.
    Unaccepted(String),
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
pub struct Limits {
    /// How many pieces of work run at once; as many again may wait.
    pub jobs: usize,
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
    /// Where a host asks for rounds, where one does.
    pub asked: Option<Asked>,
}

/// Where a host asks for rounds by posting to herald, as Azure Functions'
/// timer triggers post to their functions, and what it is answered once a
/// round is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asked {
    /// The path a poll is asked for at.
    pub poll: &'static str,
    /// The path housekeeping is asked for at; keys read longest ago are
    /// forgotten with it.
    pub housekeep: &'static str,
    /// What the host is answered, as JSON.
    pub reply: &'static str,
}

/// An `Http` sending at most a number of requests at once.
#[derive(Debug)]
pub struct Capped<H> {
    http: H,
    permits: Semaphore,
}

impl<H> Capped<H> {
    /// `http`, sending at most `most` requests at once.
    #[must_use]
    pub fn new(http: H, most: usize) -> Self {
        Self {
            http,
            permits: Semaphore::new(most),
        }
    }
}

impl<H: Http> Http for Capped<H> {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let _permit = self.permits.acquire().await;
        self.http.send(request).await
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

fn busy() -> hyper::Response<Full<Bytes>> {
    let mut response = replied(
        503,
        TEXT,
        b"herald has as much work waiting as it takes; ask again later".to_vec(),
    );
    response
        .headers_mut()
        .insert(RETRY_AFTER, HeaderValue::from_static(BUSY_RETRY));
    response
}

/// The reply to `request`, its work left waiting in `queue`: a request
/// that may leave work is answered 503 where the queue has no place for it.
async fn answered<S, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    request: hyper::Request<Incoming>,
    queue: &Queue,
    limits: Limits,
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
    if let (Some(asked), &Method::POST) = (limits.asked, request.method())
        && let Some(reply) = rounds.asked(serving, asked, request.uri().path()).await
    {
        return Ok(reply);
    }
    let held = if request.method() == Method::POST {
        let Some(held) = queue.hold() else {
            return Ok(busy());
        };
        Some(held)
    } else {
        None
    };
    let (parts, body) = request.into_parts();
    let Ok(read) =
        tokio::time::timeout(limits.read, Limited::new(body, limits.body).collect()).await
    else {
        return Ok(replied(
            408,
            TEXT,
            b"the request's body was not sent in time".to_vec(),
        ));
    };
    let Ok(collected) = read else {
        return Ok(replied(
            413,
            TEXT,
            b"the request's body is larger than herald takes, or could not be read".to_vec(),
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
        path: parts.uri.path(),
        query: parts.uri.query(),
        headers: &borrowed,
        body: &body,
    };
    let answered = answer(serving, &inbound).await;
    if let (Some(job), Some(held)) = (answered.job, held) {
        held.fill(job);
    }
    Ok(replied(
        answered.reply.status,
        answered.reply.kind,
        answered.reply.body,
    ))
}

/// Which rounds are running, so that one of each kind runs at a time, and
/// where what they did is said.
struct Rounds<'a> {
    polling: Cell<bool>,
    keeping: Cell<bool>,
    sweeping: Cell<bool>,
    heard: &'a dyn Fn(Heard),
}

/// `round` done, where no round `running` marks is.
async fn once(running: &Cell<bool>, round: impl Future<Output = Option<Heard>>) -> Option<Heard> {
    if running.replace(true) {
        return None;
    }
    let done = round.await;
    running.set(false);
    done
}

impl Rounds<'_> {
    /// A poll, where none is running.
    async fn polled<S: Store, C: Clock, G: Signer, V, H: Http, P, K>(
        &self,
        serving: &Serving<'_, S, C, G, V, H, P, K>,
    ) -> Option<Heard> {
        once(&self.polling, async {
            Some(Heard::Polled(
                poll(serving.config, serving.connected, serving.ports, RUN).await,
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
                housekeep(serving.connected, ports.store, ports.clock).await,
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

    /// The round `path` asks for of `asked`, done, and its answer; or none
    /// where it asks for none.
    async fn asked<S: Store, C: Clock, G: Signer, V, H: Http, P, K: Keyring>(
        &self,
        serving: &Serving<'_, S, C, G, V, H, P, K>,
        asked: Asked,
        path: &str,
    ) -> Option<hyper::Response<Full<Bytes>>> {
        let done = if path == asked.poll {
            self.polled(serving).await
        } else if path == asked.housekeep {
            self.swept(serving).await;
            self.kept(serving).await
        } else {
            return None;
        };
        if let Some(heard) = done {
            (self.heard)(heard);
        }
        Some(replied(200, JSON, asked.reply.as_bytes().to_vec()))
    }
}

/// The one request `stream` carries, answered, within [`HELD`].
async fn connection<S, C, G, V, H, P, K, T>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    stream: T,
    queue: &Queue,
    limits: Limits,
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
    let _ = tokio::time::timeout(HELD, served).await;
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
    limits: Limits,
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
        polling: Cell::new(false),
        keeping: Cell::new(false),
        sweeping: Cell::new(false),
        heard,
    };
    let queue = Queue::new(limits.jobs);
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
                Some(Heard::Worked(work(serving, job, RUN).await))
            }));
        }
        tokio::select! {
            () = &mut stop => break,
            accepted = listener.take(), if connections.len() < limits.connections => match accepted {
                Ok(stream) => connections.push(connection(serving, stream, &queue, limits, &rounding)),
                Err(error) => {
                    heard(Heard::Unaccepted(error.to_string()));
                    tokio::time::sleep(ACCEPT_PAUSE).await;
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
            () = round(&mut polls), if !rounding.polling.get() => {
                running.push(Box::pin(rounding.polled(serving)));
            }
            () = round(&mut chores), if !rounding.keeping.get() => {
                running.push(Box::pin(rounding.kept(serving)));
            }
            () = round(&mut sweeps), if !rounding.sweeping.get() => {
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

    use super::{Queue, round, rounds};

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
