//! A request as a host with no script of its own reads it, and the answer
//! herald gives it.

use std::fmt;

use herald_core::{media, status};
use herald_flows::{Admission, Inbound, ROUND_SECRET, Reply, Round, refusal, round};
use herald_ports::Secrets as _;

use crate::answer::Answer;
use crate::composed::Composed;
use crate::host::Host;
use crate::reaching::asked_query;
use crate::told;

/// A plain text reply.
fn plain(status: u16, text: &str) -> Reply {
    Reply {
        status,
        kind: media::TEXT,
        body: text.as_bytes().to_vec(),
        retry_after: None,
    }
}

/// What a request asked, as a host with no script of its own read it.
#[derive(Clone, PartialEq, Eq)]
pub struct HostRequest {
    /// The method, in capitals.
    pub method: String,
    /// The path and its query.
    pub target: String,
    /// Each header, as its name and value.
    pub headers: Vec<(String, String)>,
    /// The raw body.
    pub body: Vec<u8>,
}

impl fmt::Debug for HostRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("HostRequest")
            .field("method", &self.method)
            .field(
                "path",
                &self
                    .target
                    .split_once('?')
                    .map_or(self.target.as_str(), |(path, _)| path),
            )
            .field("headers", &headers)
            .field("body", &format_args!("{} bytes", self.body.len()))
            .finish()
    }
}

impl<H: Host> Composed<'_, H> {
    /// What comes of `asked`, a request a host with no script of its own
    /// read.
    pub async fn reached(&self, asked: &HostRequest) -> Answer {
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
            return refusal(status::UNAUTHORIZED, round::UNASKED).into();
        }
        let said = match round {
            Round::Poll => self.poll().await.iter().flat_map(told::polled).collect(),
            Round::Housekeep => told::kept(&self.housekeep().await),
        };
        Answer {
            reply: plain(status::OK, round::RAN),
            refused: None,
            left: None,
            said,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_debugs_without_its_query_header_values_or_body() {
        let asked = HostRequest {
            method: "POST".to_owned(),
            target: "/matrix/chat/transactions/1?access_token=hs-secret".to_owned(),
            headers: vec![("Authorization".to_owned(), "Bearer round-secret".to_owned())],
            body: b"{}".to_vec(),
        };
        let shown = format!("{asked:?}");
        assert_eq!(
            shown,
            r#"HostRequest { method: "POST", path: "/matrix/chat/transactions/1", headers: ["Authorization"], body: 2 bytes }"#
        );
    }
}
