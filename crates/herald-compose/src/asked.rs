use herald_flows::{Admission, Inbound, ROUND_SECRET, Reply, Round, refusal};
use herald_ports::Secrets as _;

use crate::answer::Answer;
use crate::composed::Composed;
use crate::host::Host;
use crate::reaching::asked_query;
use crate::told;

const PLAIN: &str = "text/plain; charset=utf-8";

/// A plain text reply.
fn plain(status: u16, text: &str) -> Reply {
    Reply {
        status,
        kind: PLAIN,
        body: text.as_bytes().to_vec(),
        retry_after: None,
    }
}

/// What a request asked, as a host with no script of its own read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The method, in capitals.
    pub method: String,
    /// The path and its query.
    pub target: String,
    /// Each header, as its name and value.
    pub headers: Vec<(String, String)>,
    /// The raw body.
    pub body: Vec<u8>,
}

impl<H: Host> Composed<H> {
    /// What comes of `asked`, a request a host with no script of its own
    /// read.
    pub async fn reached(&self, asked: &Asked) -> Answer {
        let (path, query) = match asked.target.split_once('?') {
            Some((path, query)) => (path, asked_query(Some(query.to_owned()))),
            None => (asked.target.as_str(), None),
        };
        let headers: Vec<(&str, &str)> = asked
            .headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let inbound = Inbound {
            method: &asked.method,
            path,
            query: query.as_deref(),
            headers: &headers,
            body: &asked.body,
        };
        self.answer(&inbound, Admission::Once).await
    }

    /// `round` run, where `authorization` holds the round secret the host
    /// keeps as [`ROUND_SECRET`].
    pub(crate) async fn round(&self, round: Round, authorization: Option<&str>) -> Answer {
        let secret = self.host().secrets().secret(ROUND_SECRET).await.ok();
        if !Round::allowed(authorization, secret.as_ref()) {
            return refusal(401, "a round was asked without its secret").into();
        }
        let said = match round {
            Round::Poll => self.poll().await.iter().flat_map(told::polled).collect(),
            Round::Housekeep => told::kept(&self.housekeep().await),
        };
        Answer {
            reply: plain(200, "the round ran"),
            refused: None,
            left: None,
            said,
        }
    }
}
