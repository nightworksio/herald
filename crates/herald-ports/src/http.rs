use std::fmt;
use std::future::Future;

use herald_core::{Sent, Waits};
use jiff::{SignedDuration, Timestamp};

use crate::outbound::Outbound;
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
/// its `Display` redacts, and a sealed body is never shown.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    /// The method.
    pub method: Method,
    /// The URL.
    pub url: Text,
    /// The headers, in order.
    pub headers: Vec<(String, Text)>,
    /// The body.
    pub body: Vec<u8>,
    /// The most bytes of its response's body that are read.
    pub most: usize,
    /// Whether the body carries a secret, so it is shown by its length
    /// alone, even in a dry run.
    pub sealed: bool,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("url", &self.url.redacted())
            .field(
                "headers",
                &self
                    .headers
                    .iter()
                    .map(|(name, value)| (name, value.redacted()))
                    .collect::<Vec<_>>(),
            )
            .field("body", &format_args!("{} bytes", self.body.len()))
            .field("most", &self.most)
            .field("sealed", &self.sealed)
            .finish()
    }
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
            most: Outbound::BODY_MOST,
            sealed: false,
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

    /// The request with `body`, which carries a secret: it is sent as it
    /// is, and shown by its length alone.
    #[must_use]
    pub fn sealed_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self.sealed = true;
        self
    }

    /// The request reading at most `bytes` of its response's body, and
    /// none of a response that says it is longer.
    #[must_use]
    pub fn at_most(mut self, bytes: usize) -> Self {
        self.most = bytes.min(Outbound::BODY_MOST);
        self
    }

    /// The request reading no more than `body` bytes of its response's
    /// body, nor more than it already reads.
    #[must_use]
    pub fn within(mut self, body: usize) -> Self {
        self.most = self.most.min(body);
        self
    }
}

impl Request {
    fn head(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.method.as_str(), self.url.redacted())?;
        for (name, value) in &self.headers {
            write!(f, "\n{name}: {}", value.redacted())?;
        }
        Ok(())
    }

    /// The request as [`fmt::Display`] shows it, with its body in full in
    /// place of its length: for a dry run, which shows what would be sent.
    #[must_use]
    pub fn with_body(&self) -> WithBody<'_> {
        WithBody(self)
    }
}

/// A request shown with its body in full, its secrets still redacted.
#[derive(Debug)]
pub struct WithBody<'r>(&'r Request);

impl fmt::Display for WithBody<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.head(f)?;
        if self.0.sealed {
            write!(f, "\n\n«sealed: {} bytes»", self.0.body.len())?;
        } else if !self.0.body.is_empty() {
            write!(f, "\n\n{}", String::from_utf8_lossy(&self.0.body))?;
        }
        Ok(())
    }
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.head(f)?;
        if !self.body.is_empty() {
            write!(f, "\n\n{} bytes", self.body.len())?;
        }
        Ok(())
    }
}

/// The base a destination names when it reaches any public address rather
/// than one origin, as an ActivityPub actor's deliveries to its followers'
/// inboxes do. Its `Http` admits only public addresses (ADR-0007).
pub const PUBLIC: &str = "public:";

/// The `User-Agent` every request herald sends carries: herald, its version
/// and its repository (ADR-0003).
pub const USER_AGENT: &str = concat!(
    "herald/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/nightworksio/herald)"
);

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

    /// When the response was sent, as its `Date` names it.
    #[must_use]
    pub fn date(&self) -> Option<Timestamp> {
        http_date(self.header("date")?)
    }

    /// How long its `Retry-After` asks to wait: the seconds it names, or the
    /// time from the response's `Date` to the date it names.
    #[must_use]
    pub fn retry_after(&self) -> Option<SignedDuration> {
        let after = self.header("retry-after")?.trim();
        if let Ok(seconds) = after.parse::<f64>() {
            return SignedDuration::try_from_secs_f64(seconds)
                .ok()
                .filter(|wait| !wait.is_negative());
        }
        self.until(http_date(after)?)
    }

    /// The time from the response's `Date` to `at`, where `at` is not before
    /// it: a wait measured by the server's own clock.
    #[must_use]
    pub fn until(&self, at: Timestamp) -> Option<SignedDuration> {
        Some(at.duration_since(self.date()?)).filter(|wait| !wait.is_negative())
    }

    /// How long to wait before asking again, where its status asks for a
    /// wait: the one `named` in its body, else its `Retry-After`, else the
    /// one `waits` gives.
    #[must_use]
    pub fn wait(&self, waits: Waits, named: Option<SignedDuration>) -> Option<SignedDuration> {
        waits.after(self.status, named.or_else(|| self.retry_after()))
    }

    /// What it means where it did not succeed: a wait, as [`Self::wait`]
    /// reads one, or else `who` refusing it with what it `said`.
    #[must_use]
    pub fn trouble(
        &self,
        waits: Waits,
        named: Option<SignedDuration>,
        who: &str,
        said: impl FnOnce() -> String,
    ) -> Option<Sent> {
        if let Some(wait) = self.wait(waits, named) {
            return Some(Sent::RetryAfter(wait));
        }
        (!self.succeeded()).then(|| Sent::Refused(self.refused(who, &said())))
    }

    /// Whether `who` took a request it answered with this, or else why
    /// not: what it said, and the wait it asked for, the one `named` in its
    /// body or else its `Retry-After`.
    ///
    /// # Errors
    ///
    /// Where it did not succeed.
    pub fn took(
        &self,
        named: Option<SignedDuration>,
        who: &str,
        said: impl FnOnce() -> String,
    ) -> Result<(), String> {
        if self.succeeded() {
            return Ok(());
        }
        let refused = self.refused(who, &said());
        Err(match named.or_else(|| self.retry_after()) {
            Some(wait) => format!("{refused}, and asked to wait {wait:#}"),
            None => refused,
        })
    }

    fn refused(&self, who: &str, said: &str) -> String {
        let status = self.status;
        match said.trim() {
            "" => format!("{who} refused it with {status}"),
            said => format!("{who} refused it with {status}: {said}"),
        }
    }
}

/// The time an HTTP date names.
fn http_date(text: &str) -> Option<Timestamp> {
    jiff::fmt::rfc2822::parse(text)
        .ok()
        .map(|zoned| zoned.timestamp())
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

impl<H: Http> Http for &H {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        (**self).send(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::Secret;

    #[test]
    fn a_request_reads_no_more_than_its_cap_its_ask_and_the_ceiling() {
        let request = || Request::new(Method::Get, Text::plain("https://example.org"));
        assert_eq!(request().most, Outbound::BODY_MOST);
        assert_eq!(request().at_most(usize::MAX).most, Outbound::BODY_MOST);
        assert_eq!(request().at_most(10).most, 10);
        assert_eq!(request().within(Outbound::BODY).most, Outbound::BODY);
        assert_eq!(request().at_most(10).within(20).most, 10);
        assert_eq!(request().at_most(20).within(10).most, 10);
    }

    fn answered(headers: &[(&str, &str)]) -> Response {
        Response {
            status: 429,
            headers: headers
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            body: Vec::new(),
        }
    }

    const SENT: (&str, &str) = ("Date", "Sun, 08 Nov 2026 22:00:00 GMT");

    #[test]
    fn retry_after_names_seconds_or_a_date_measured_from_the_answer_s_own() {
        let after = |value| answered(&[SENT, ("Retry-After", value)]).retry_after();
        assert_eq!(after("120"), Some(SignedDuration::from_secs(120)));
        assert_eq!(after(" 1.5 "), Some(SignedDuration::from_millis(1_500)));
        assert_eq!(after("0"), Some(SignedDuration::ZERO));
        assert_eq!(
            after("Sun, 08 Nov 2026 22:05:00 GMT"),
            Some(SignedDuration::from_mins(5))
        );
        for unread in ["-1", "NaN", "inf", "soon", "Sun, 08 Nov 2026 21:59:59 GMT"] {
            assert_eq!(after(unread), None, "{unread}");
        }
        assert_eq!(answered(&[]).retry_after(), None);
        let undated = answered(&[("retry-after", "Sun, 08 Nov 2026 22:05:00 GMT")]);
        assert_eq!(undated.retry_after(), None);
        assert_eq!(
            answered(&[("retry-after", "7")]).retry_after(),
            Some(SignedDuration::from_secs(7))
        );
    }

    fn status(status: u16, headers: &[(&str, &str)]) -> Response {
        Response {
            status,
            ..answered(headers)
        }
    }

    const WAITS: Waits = Waits {
        outage: SignedDuration::from_secs(3),
        limited: SignedDuration::from_secs(9),
    };

    #[test]
    fn a_wait_is_the_one_the_body_names_else_retry_after_else_the_status_s_own() {
        let named = Some(SignedDuration::from_secs(1));
        let limited = status(429, &[("Retry-After", "2")]);
        assert_eq!(limited.wait(WAITS, named), named);
        assert_eq!(
            limited.wait(WAITS, None),
            Some(SignedDuration::from_secs(2))
        );
        assert_eq!(status(429, &[]).wait(WAITS, None), Some(WAITS.limited));
        assert_eq!(status(503, &[]).wait(WAITS, None), Some(WAITS.outage));
        assert_eq!(
            status(400, &[("Retry-After", "2")]).wait(WAITS, named),
            None
        );
    }

    #[test]
    fn trouble_is_a_wait_a_refusal_naming_who_and_what_was_said_or_none() {
        let said = || " Missing Access ".to_owned();
        assert_eq!(
            status(429, &[]).trouble(WAITS, None, "Discord", said),
            Some(Sent::RetryAfter(WAITS.limited))
        );
        assert_eq!(
            status(403, &[]).trouble(WAITS, None, "Discord", said),
            Some(Sent::Refused(
                "Discord refused it with 403: Missing Access".to_owned()
            ))
        );
        assert_eq!(
            status(404, &[]).trouble(WAITS, None, "the inbox", String::new),
            Some(Sent::Refused("the inbox refused it with 404".to_owned()))
        );
        assert_eq!(status(204, &[]).trouble(WAITS, None, "Discord", said), None);
    }

    #[test]
    fn took_says_what_was_refused_and_any_wait_asked_for() {
        let said = || "slow down".to_owned();
        assert_eq!(status(200, &[]).took(None, "Slack", said), Ok(()));
        assert_eq!(
            status(500, &[]).took(None, "Slack", String::new),
            Err("Slack refused it with 500".to_owned())
        );
        assert_eq!(
            status(429, &[("Retry-After", "3")]).took(None, "Slack", said),
            Err("Slack refused it with 429: slow down, and asked to wait 3s".to_owned())
        );
        assert_eq!(
            status(429, &[]).took(Some(SignedDuration::from_millis(1_500)), "Slack", said),
            Err("Slack refused it with 429: slow down, and asked to wait 1s 500ms".to_owned())
        );
    }

    #[test]
    fn a_wait_until_a_time_is_measured_from_the_answer_s_date() {
        let sent = answered(&[SENT]);
        let at = |text: &str| text.parse::<Timestamp>().unwrap();
        assert_eq!(sent.date(), Some(at("2026-11-08T22:00:00Z")));
        assert_eq!(
            sent.until(at("2026-11-08T22:00:30Z")),
            Some(SignedDuration::from_secs(30))
        );
        assert_eq!(
            sent.until(at("2026-11-08T22:00:00Z")),
            Some(SignedDuration::ZERO)
        );
        assert_eq!(sent.until(at("2026-11-08T21:00:00Z")), None);
        assert_eq!(answered(&[]).until(at("2026-11-08T22:00:30Z")), None);
        assert_eq!(answered(&[("Date", "yesterday")]).date(), None);
    }

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
            request.with_body().to_string(),
            "POST https://discord.com/api/v10/channels/1/messages\nAuthorization: Bot «secret:TOKEN»\nContent-Type: application/json\n\n{\"content\":\"hi\"}"
        );
        assert_eq!(
            request.to_string(),
            "POST https://discord.com/api/v10/channels/1/messages\nAuthorization: Bot «secret:TOKEN»\nContent-Type: application/json\n\n16 bytes"
        );
    }

    #[test]
    fn a_sealed_body_is_sent_as_it_is_and_shown_by_its_length_alone() {
        let request = Request::new(Method::Post, Text::plain("https://hub.example/auth"))
            .header(
                "Authorization",
                Text::plain("Bearer ").then_secret(Secret::new("HUB", "pat-1")),
            )
            .sealed_body(r#"{"secret":"pat-1"}"#);
        assert_eq!(request.body, br#"{"secret":"pat-1"}"#);
        assert!(request.sealed);
        let shown = [
            request.to_string(),
            request.with_body().to_string(),
            format!("{request:?}"),
        ];
        for shown in &shown {
            assert!(!shown.contains("pat-1"), "{shown}");
        }
        assert!(shown[1].ends_with("«sealed: 18 bytes»"), "{}", shown[1]);
        assert!(shown[2].contains("18 bytes"), "{}", shown[2]);
        assert!(!Request::new(Method::Get, Text::plain("https://example.org")).sealed);
    }

    #[test]
    fn a_request_without_a_body_displays_no_blank_line() {
        let request = Request::new(Method::Get, Text::plain("https://example.org"));
        assert_eq!(request.to_string(), "GET https://example.org");
        assert_eq!(request.with_body().to_string(), "GET https://example.org");
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
