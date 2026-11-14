//! What makes herald's connections: the native binary's, or whatever a
//! test or another host hands [`crate::run_until`].

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use herald_native::{ConnectError, NativeHttp, PublicHttp};
use herald_ports::{Http, HttpError, Outbound, PUBLIC_BASE, Request, Response};

use crate::worded;

type Sending<'a> = Pin<Box<dyn Future<Output = Result<Response, HttpError>> + Send + 'a>>;

trait Sends: Send + Sync {
    fn sending(&self, request: Request) -> Sending<'_>;
}

impl<H: Http + Send> Sends for H {
    fn sending(&self, request: Request) -> Sending<'_> {
        Box::pin(self.send(request))
    }
}

/// One base URL's `Http`, whatever reaches it.
#[derive(Clone)]
pub struct Connection(Arc<dyn Sends>);

impl Connection {
    /// The connection `http` makes.
    pub fn new(http: impl Http + Send + 'static) -> Self {
        Self(Arc::new(http))
    }
}

impl Http for Connection {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        self.0.sending(request)
    }
}

/// A function making a connection to a base URL within outbound bounds.
type Making<'a> = &'a (dyn Fn(&str, &Outbound) -> Result<Connection, String> + Send + Sync);

/// What makes connections: to a base URL's origin, each request bounded in
/// time, and to Lambda's runtime API, which holds a request open until an
/// invocation arrives.
pub struct Connector<'a> {
    reaching: Making<'a>,
    waiting: Making<'a>,
    outbound: Outbound,
}

impl<'a> Connector<'a> {
    /// Connections made by `reaching`, and by `waiting` for Lambda's
    /// runtime API, within the bounds no config sets.
    #[must_use]
    pub const fn new(reaching: Making<'a>, waiting: Making<'a>) -> Self {
        Self {
            reaching,
            waiting,
            outbound: Outbound::STANDARD,
        }
    }

    /// The same connections, within `outbound`.
    #[must_use]
    pub fn within(&self, outbound: Outbound) -> Self {
        Self {
            reaching: self.reaching,
            waiting: self.waiting,
            outbound,
        }
    }

    /// The connection to `base`.
    ///
    /// # Errors
    ///
    /// When `base` may not be reached.
    pub fn reach(&self, base: &str) -> Result<Connection, String> {
        self.reach_within(base, &self.outbound)
    }

    /// The connection to `base`, within `outbound`.
    ///
    /// # Errors
    ///
    /// When `base` may not be reached.
    pub(crate) fn reach_within(
        &self,
        base: &str,
        outbound: &Outbound,
    ) -> Result<Connection, String> {
        (self.reaching)(base, outbound)
    }

    pub(crate) fn wait(&self, base: &str) -> Result<Connection, String> {
        (self.waiting)(base, &self.outbound)
    }
}

/// The native binary's connections.
pub const NATIVE: Connector<'static> = Connector::new(&native, &waiting);

/// What makes the connections a run needs.
pub type Connect<'a> = &'a Connector<'a>;

/// The native connection to Lambda's runtime API at `base`, which bounds
/// no answer's time, as the runtime holds a request until an invocation
/// arrives.
///
/// # Errors
///
/// When `base` may not be reached, or the client cannot be built.
pub fn waiting(base: &str, _: &Outbound) -> Result<Connection, String> {
    worded(NativeHttp::waiting(base).map(Connection::new))
}

/// The native connection to `base` within `outbound`: one reaching only its
/// origin, or, for [`PUBLIC_BASE`], one reaching any public address (ADR-0007).
///
/// # Errors
///
/// When `base` may not be reached, or the client cannot be built.
pub fn native(base: &str, outbound: &Outbound) -> Result<Connection, String> {
    let connection = if base == PUBLIC_BASE {
        PublicHttp::new(outbound)
            .map(Connection::new)
            .map_err(ConnectError::Build)
    } else {
        NativeHttp::reaching(base, outbound).map(Connection::new)
    };
    worded(connection)
}
