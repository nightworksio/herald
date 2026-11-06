//! What each connection the host's `fetch` makes may reach, and what of
//! its answer is read.

use herald_compose::{Reach, public, reachable};
use herald_ports::{HttpError, PUBLIC, Request, Response, Text};
use url::Url;

/// What one connection may reach: the origins of its base, or any public
/// HTTPS origin, on others' behalf (ADR-0007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reaching {
    /// The origins of a base.
    Base(Reach),
    /// Any public HTTPS origin.
    Public,
}

impl Reaching {
    /// What a connection to `base` may reach.
    ///
    /// # Errors
    ///
    /// Where `base` is not a URL a connection may be bound to, saying why.
    pub fn of(base: &str) -> Result<Self, String> {
        if base == PUBLIC {
            return Ok(Self::Public);
        }
        Reach::new([base])
            .map(Self::Base)
            .map_err(|error| error.to_string())
    }

    /// Whether the connection reaches on others' behalf.
    #[must_use]
    pub fn public(&self) -> bool {
        *self == Self::Public
    }

    /// The URL `request` names, where it may be reached.
    ///
    /// # Errors
    ///
    /// Where it may not, refused unsent.
    pub fn admitted(&self, url: &Text) -> Result<Url, HttpError> {
        Url::parse(&url.expose())
            .ok()
            .filter(|parsed| match self {
                Self::Base(reach) => reach.allows(parsed),
                Self::Public => reachable(parsed, "https", public),
            })
            .ok_or_else(|| HttpError::Refused {
                url: url.redacted(),
            })
    }
}

/// Whether `text` is an address on the public internet, as a host that
/// resolves names itself checks each address a name resolves to.
#[must_use]
pub fn public_address(text: &str) -> bool {
    text.parse().is_ok_and(public)
}

/// The response the host answered `request` with, where its body is no
/// larger than the request reads.
///
/// # Errors
///
/// Where the body is larger.
pub fn received(request: &Request, response: Response) -> Result<Response, HttpError> {
    if response.body.len() > request.most {
        return Err(HttpError::Unreachable {
            reason: format!("the response is larger than {} bytes", request.most),
        });
    }
    Ok(response)
}

/// The query a request asked with, where it asked with one.
#[must_use]
pub fn asked_query(query: Option<String>) -> Option<String> {
    query.filter(|query| !query.is_empty())
}

#[cfg(test)]
mod tests {
    use herald_ports::Method;

    use super::*;

    #[test]
    fn a_request_asks_with_a_query_only_where_it_has_one() {
        assert_eq!(
            asked_query(Some("page=2".to_owned())).as_deref(),
            Some("page=2")
        );
        assert_eq!(asked_query(Some(String::new())), None);
        assert_eq!(asked_query(None), None);
    }

    fn plain(url: &str) -> Text {
        Text::plain(url)
    }

    #[test]
    fn a_base_connection_reaches_its_own_origin_alone() {
        let reaching = Reaching::of("https://discord.com/api/v10").unwrap();
        assert!(!reaching.public());
        assert_eq!(
            reaching
                .admitted(&plain("https://discord.com/api/v10/users/@me"))
                .map(String::from),
            Ok("https://discord.com/api/v10/users/@me".to_owned())
        );
        assert_eq!(
            reaching.admitted(&plain("https://example.org/")),
            Err(HttpError::Refused {
                url: "https://example.org/".to_owned()
            })
        );
        assert_eq!(
            Reaching::of("http://discord.com"),
            Err(
                "http://discord.com is not HTTPS; plain HTTP is allowed only on a loopback host"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_public_connection_reaches_public_https_origins_alone() {
        let reaching = Reaching::of(PUBLIC).unwrap();
        assert!(reaching.public());
        assert!(
            reaching
                .admitted(&plain("https://social.example/users/ana"))
                .is_ok()
        );
        for refused in ["https://10.0.0.1/", "http://social.example/", "not a url"] {
            assert!(reaching.admitted(&plain(refused)).is_err(), "{refused}");
        }
    }

    #[test]
    fn only_a_public_address_is_public() {
        assert!(public_address("93.184.216.34"));
        assert!(public_address("2606:4700::1111"));
        assert!(!public_address("10.0.0.1"));
        assert!(!public_address("::1"));
        assert!(!public_address("social.example"));
    }

    #[test]
    fn a_body_larger_than_the_request_reads_is_refused() {
        let request = Request::new(Method::Get, plain("https://x.example/")).at_most(2);
        let response = |body: &[u8]| Response {
            status: 200,
            headers: Vec::new(),
            body: body.to_vec(),
        };
        assert_eq!(received(&request, response(b"ok")), Ok(response(b"ok")));
        assert_eq!(
            received(&request, response(b"big")),
            Err(HttpError::Unreachable {
                reason: "the response is larger than 2 bytes".to_owned()
            })
        );
    }
}
