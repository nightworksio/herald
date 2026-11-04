//! `herald server` (ADR-0002, layer 3): the HTTP endpoint every native
//! driver serves.
//!
//! [`serve`] answers each request through [`herald_flows::answer`] on one
//! task: forge webhooks, herald's ActivityPub actors and their inboxes, and
//! health. The work a request leaves runs once it is answered, at most
//! [`Limits::jobs`] at once, with as many again waiting; a request that
//! could leave more is answered 503. At most [`Limits::connections`] are
//! held, and each has its headers and its body read in time. Sources are polled, threads kept and
//! keys forgotten on the config's schedule. TLS is a reverse proxy's.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::{Pin, pin};
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use herald_flows::{
    Inbound, Job, Kept, Serving, Stop, Swept, Worked, answer, housekeep, poll, unworked, work,
};
use herald_ports::{Clock, Http, HttpError, Keyring, Request, Response, Signer, Store, Verifier};
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{CONTENT_TYPE, HeaderValue, RETRY_AFTER};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, Semaphore};
use tokio::time::{Instant, Interval, MissedTickBehavior, interval_at};

/// The name the server takes its leases under.
const RUN: &str = "server";
/// How many seconds a sender is asked to wait when the work queue is full.
const BUSY_RETRY: &str = "30";
/// How long a connection may take to send a request's headers, and then
/// its body.
const READ: Duration = Duration::from_secs(30);
/// What a reply in words is.
const TEXT: &str = "text/plain; charset=utf-8";

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
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        async move {
            let _permit = self.permits.acquire().await;
            self.http.send(request).await
        }
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
) -> Result<hyper::Response<Full<Bytes>>, Infallible>
where
    S: Store,
    C: Clock,
    G: Signer,
    V: Verifier,
    P: Http,
    K: Keyring,
{
    let held = if request.method() == Method::POST {
        let Some(held) = queue.hold() else {
            return Ok(busy());
        };
        Some(held)
    } else {
        None
    };
    let (parts, body) = request.into_parts();
    let Ok(read) = tokio::time::timeout(READ, Limited::new(body, limits.body).collect()).await
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

/// Every request `stream` carries, answered.
async fn connection<S, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    stream: TcpStream,
    queue: &Queue,
    limits: Limits,
) where
    S: Store,
    C: Clock,
    G: Signer,
    V: Verifier,
    P: Http,
    K: Keyring,
{
    let service = service_fn(move |request| answered(serving, request, queue, limits));
    let _ = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(READ)
        .serve_connection(TokioIo::new(stream), service)
        .await;
}

/// A round every `period`, the first a period from now.
fn rounds(period: Option<Duration>) -> Option<Interval> {
    period.filter(|period| !period.is_zero()).map(|period| {
        let mut rounds = interval_at(Instant::now() + period, period);
        rounds.set_missed_tick_behavior(MissedTickBehavior::Delay);
        rounds
    })
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
pub async fn serve<S, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    listener: TcpListener,
    limits: Limits,
    heard: &dyn Fn(Heard),
    stop: impl Future<Output = ()>,
) where
    S: Store,
    C: Clock,
    G: Signer,
    V: Verifier,
    H: Http,
    P: Http,
    K: Keyring,
{
    if let Ok(address) = listener.local_addr() {
        heard(Heard::Listening(address));
    }
    let queue = Queue::new(limits.jobs);
    let mut connections = FuturesUnordered::new();
    let mut running: FuturesUnordered<Pin<Box<dyn Future<Output = Option<Heard>> + '_>>> =
        FuturesUnordered::new();
    let mut polls = rounds(limits.poll);
    let mut chores = rounds(limits.housekeep);
    let mut sweeps = rounds(Some(limits.keys));
    let mut stop = pin!(stop);
    loop {
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
            accepted = listener.accept(), if connections.len() < limits.connections => match accepted {
                Ok((stream, _)) => {
                    connections.push(connection(serving, stream, &queue, limits));
                }
                Err(error) => heard(Heard::Unaccepted(error.to_string())),
            },
            () = queue.queued.notified() => {}
            Some(()) = connections.next(), if !connections.is_empty() => {}
            Some(done) = running.next(), if !running.is_empty() => {
                if let Some(said) = done {
                    heard(said);
                }
            }
            () = round(&mut polls) => running.push(Box::pin(async move {
                Some(Heard::Polled(
                    poll(serving.config, serving.connected, serving.ports, RUN).await,
                ))
            })),
            () = round(&mut chores) => running.push(Box::pin(async move {
                Some(Heard::Kept(
                    housekeep(serving.connected, serving.ports.store, serving.ports.clock).await,
                ))
            })),
            () = round(&mut sweeps) => running.push(Box::pin(async move {
                let before = serving
                    .ports
                    .clock
                    .now()
                    .checked_sub(serving.keys.refreshed)
                    .ok()?;
                let _ = serving.keys.keyring.forget_before(before).await;
                None
            })),
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
