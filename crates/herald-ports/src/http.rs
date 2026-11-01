use std::fmt;
use std::future::Future;

use crate::secret::Text;

/// An HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
    /// `PUT`.
    Put,
    /// `PATCH`.
    Patch,
    /// `DELETE`.
    Delete,
}

impl Method {
    /// The method as HTTP spells it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

/// One request, as a value. Its URL and header values may carry secrets, which
/// its `Display` redacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The method.
    pub method: Method,
    /// The URL.
    pub url: Text,
    /// The headers, in order.
    pub headers: Vec<(String, Text)>,
    /// The body.
    pub body: Vec<u8>,
}

impl Request {
    /// A request with no headers and no body.
    #[must_use]
    pub fn new(method: Method, url: Text) -> Self {
        Self {
            method,
            url,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// The request with the header `name` set to `value`.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: Text) -> Self {
        self.headers.push((name.into(), value));
        self
    }

    /// The request with `body`.
    #[must_use]
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.method.as_str(), self.url.redacted())?;
        for (name, value) in &self.headers {
            write!(f, "\n{name}: {}", value.redacted())?;
        }
        if !self.body.is_empty() {
            write!(f, "\n\n{}", String::from_utf8_lossy(&self.body))?;
        }
        Ok(())
    }
}

/// One response, as a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// The status code.
    pub status: u16,
    /// The headers, in order.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

impl Response {
    /// The first value of the header `name`, matched without regard to case.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Whether the status is a success, `2xx`.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Why a request got no response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The request named a host or scheme the destination may not reach.
    Refused {
        /// The URL refused, with its secrets redacted.
        url: String,
    },
    /// The host could not be reached.
    Unreachable {
        /// What the transport said.
        reason: String,
    },
    /// The host did not answer in time.
    TimedOut,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { url } => write!(f, "{url} is not a host this destination may reach"),
            Self::Unreachable { reason } => write!(f, "the host could not be reached: {reason}"),
            Self::TimedOut => f.write_str("the host did not answer in time"),
        }
    }
}

/// One request, one response.
pub trait Http: Sync {
    /// The response to `request`.
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::Secret;

    #[test]
    fn each_method_is_spelled_as_http_spells_it() {
        let methods = [
            Method::Get,
            Method::Post,
            Method::Put,
            Method::Patch,
            Method::Delete,
        ];
        let names: Vec<&str> = methods.iter().map(|method| method.as_str()).collect();
        assert_eq!(names, ["GET", "POST", "PUT", "PATCH", "DELETE"]);
    }

    #[test]
    fn a_request_displays_with_its_secrets_redacted() {
        let request = Request::new(
            Method::Post,
            Text::plain("https://discord.com/api/v10/channels/1/messages"),
        )
        .header(
            "Authorization",
            Text::plain("Bot ").then_secret(Secret::new("TOKEN", "hunter2")),
        )
        .header("Content-Type", Text::plain("application/json"))
        .body(r#"{"content":"hi"}"#);
        assert_eq!(
            request.to_string(),
            "POST https://discord.com/api/v10/channels/1/messages\nAuthorization: Bot «secret:TOKEN»\nContent-Type: application/json\n\n{\"content\":\"hi\"}"
        );
    }

    #[test]
    fn a_request_without_a_body_displays_no_blank_line() {
        let request = Request::new(Method::Get, Text::plain("https://example.org"));
        assert_eq!(request.to_string(), "GET https://example.org");
    }

    #[test]
    fn a_response_finds_its_headers_without_regard_to_case() {
        let response = Response {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            body: Vec::new(),
        };
        assert_eq!(response.header("content-type"), Some("application/json"));
        assert_eq!(response.header("etag"), None);
    }

    #[test]
    fn only_a_2xx_status_succeeded() {
        let response = |status| Response {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert!(!response(199).succeeded());
        assert!(response(200).succeeded());
        assert!(response(299).succeeded());
        assert!(!response(300).succeeded());
    }

    #[test]
    fn every_http_error_reads_as_a_sentence() {
        assert_eq!(
            HttpError::Refused {
                url: "http://10.0.0.1".to_owned()
            }
            .to_string(),
            "http://10.0.0.1 is not a host this destination may reach"
        );
        assert_eq!(
            HttpError::Unreachable {
                reason: "dns".to_owned()
            }
            .to_string(),
            "the host could not be reached: dns"
        );
        assert_eq!(
            HttpError::TimedOut.to_string(),
            "the host did not answer in time"
        );
    }
}
