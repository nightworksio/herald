use herald_flows::{Admission, Inbound, Job, Reply};
use herald_ports::Secret;

use crate::composed::Composed;
use crate::host::Host;
use crate::reaching::asked_query;
use crate::round::Round;
use crate::told;

const PLAIN: &str = "text/plain; charset=utf-8";

/// A plain text reply.
#[must_use]
pub fn plain(status: u16, text: &str) -> Reply {
    Reply {
        status,
        kind: PLAIN,
        body: text.as_bytes().to_vec(),
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

/// What came of a request: its reply, the work it leaves, and what a round
/// it asked for did, in words.
#[derive(Debug)]
pub struct Reached {
    /// The reply.
    pub reply: Reply,
    /// The work to do once the reply is sent.
    pub job: Option<Job>,
    /// What a round did.
    pub said: Vec<String>,
}

impl<H: Host> Composed<H> {
    /// What comes of `asked`: a round where it names one and carries
    /// `secret`, and otherwise herald's own answer.
    pub async fn reached(&self, asked: &Asked, secret: Option<&Secret>) -> Reached {
        let (path, query) = match asked.target.split_once('?') {
            Some((path, query)) => (path, asked_query(Some(query.to_owned()))),
            None => (asked.target.as_str(), None),
        };
        if let Some(round) = Round::named(&asked.method, path) {
            return self.round(round, asked, secret).await;
        }
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
        let answered = self.answer(&inbound, Admission::Once).await;
        Reached {
            reply: answered.reply,
            job: answered.job,
            said: Vec::new(),
        }
    }

    async fn round(&self, round: Round, asked: &Asked, secret: Option<&Secret>) -> Reached {
        let authorization = asked
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.as_str());
        if !Round::allowed(authorization, secret) {
            return Reached {
                reply: plain(401, "a round needs its secret"),
                job: None,
                said: Vec::new(),
            };
        }
        let said = match round {
            Round::Poll => self.poll().await.iter().flat_map(told::polled).collect(),
            Round::Housekeep => told::kept(&self.housekeep().await),
        };
        Reached {
            reply: plain(200, "the round ran"),
            job: None,
            said,
        }
    }
}
