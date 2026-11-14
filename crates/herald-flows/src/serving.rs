//! Every request `herald server` and the serverless drivers take, answered:
//! the router, forge webhooks, the HTTP API for apps, herald's ActivityPub
//! actors, the replies they give, and the work they leave.

use herald_config::Config;

use crate::announce::Ports;
use crate::prepare::Connected;
use crate::verification::{Admission, Keys};

mod actors;
mod apps;
mod jobs;
mod replies;
mod router;
mod webhooks;

pub use jobs::{Did, Interacted, Job, Worked, unplaced, unworked, work};
pub use replies::{Answered, Reply, busy, refusal};
pub(crate) use replies::{missing, rejection, replied, unmethodical, unread};
pub use router::{HEALTH, answer, uncomposed};
pub use webhooks::Webhooks;

/// An inbound HTTP request, as a driver hands it over.
#[derive(Debug, Clone, Copy)]
pub struct Inbound<'a> {
    /// The method, in capitals.
    pub method: &'a str,
    /// The path, without its query.
    pub path: &'a str,
    /// The query, without its `?`, where there is one.
    pub query: Option<&'a str>,
    /// Each header, as its name and value.
    pub headers: &'a [(&'a str, &'a str)],
    /// The raw body.
    pub body: &'a [u8],
}

impl Inbound<'_> {
    /// The value of the header `name`, in any case.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(named, _)| named.eq_ignore_ascii_case(name))
            .map(|(_, value)| *value)
    }

    fn host(&self) -> &str {
        self.header("host").unwrap_or_default()
    }
}

/// What a request is answered through.
#[derive(Debug)]
pub struct Serving<'a, S, C, G, V, H, P, K> {
    /// The config.
    pub config: &'a Config,
    /// Every route and source, connected.
    pub connected: &'a Connected<H>,
    /// The store, the clock and the signer.
    pub ports: &'a Ports<'a, S, C, G>,
    /// What checks signatures.
    pub verifier: &'a V,
    /// What reads actor documents, on any public address.
    pub public: &'a P,
    /// Where the keys activities are checked with are kept.
    pub keys: &'a Keys<'a, K>,
    /// The secret each forge source's webhooks are signed with.
    pub webhooks: &'a Webhooks,
    /// Whether a request's delivery is let through once, or again.
    pub admission: Admission,
}
