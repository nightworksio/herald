use herald_core::{Sent, Waits, status};
use herald_kinds::Probe;
use herald_ports::{DynHttp, HttpError, Method, Request, Response, Secret, SecretText};
use serde_json::Value;

use crate::email::landed;
use crate::message::Written;

/// Mailgun, as herald names it.
pub(crate) const MAILGUN: &str = "Mailgun";

/// The base URL of Mailgun in each region a domain is made in, as its
/// region's value names it
/// (<https://documentation.mailgun.com/docs/mailgun/api-reference/api-overview>,
/// "Base URL").
const REGIONS: [(&str, &str); 2] = [
    ("us", "https://api.mailgun.net"),
    ("eu", "https://api.eu.mailgun.net"),
];

/// The most bytes of one message Mailgun takes: 25 MB, read as 25,000,000
/// bytes, the smaller reading
/// (<https://documentation.mailgun.com/docs/mailgun/user-manual/sending-messages/send-http>).
pub const MAILGUN_BYTES: usize = 25_000_000;

/// The status Mailgun answers where a domain sends to more recipients than
/// its rate allows (same overview page, "Rate Limits").
const RECIPIENTS_LIMITED: u16 = 420;

/// The user Mailgun takes an API key as the password of
/// (<https://documentation.mailgun.com/docs/mailgun/api-reference/authentication>).
const USER: &str = "api";

/// The delimiter of the form a message is sent in: `=_` stands in no
/// quoted-printable body and no header line starts with `--`, so no line
/// of the message is taken for it.
const FORM: &str = "=_herald_form";

/// Each option sent with every message: open and click tracking off,
/// whatever the domain says (`POST /v3/{domain_name}/messages.mime`,
/// `o:tracking-clicks` and `o:tracking-opens`).
const UNTRACKED: [(&str, &str); 2] = [("o:tracking-clicks", "no"), ("o:tracking-opens", "no")];

/// The base URL of Mailgun in `region`.
///
/// # Errors
///
/// Why `region` names none.
pub fn hosted(region: &str) -> Result<&'static str, String> {
    REGIONS
        .iter()
        .find(|(named, _)| *named == region)
        .map(|(_, url)| *url)
        .ok_or_else(|| {
            format!(
                "it is not a region {MAILGUN} makes domains in; give \"us\" or \"eu\", where the \
                 domain was made"
            )
        })
}

/// Mailgun, sending as one domain with one API key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mailgun {
    url: String,
    domain: String,
    key: Secret,
}

impl Mailgun {
    /// Mailgun at the base URL `url`, sending as `domain` with the API key
    /// `key`.
    #[must_use]
    pub fn new(url: &str, domain: &str, key: Secret) -> Self {
        Self {
            url: url.to_owned(),
            domain: domain.to_owned(),
            key,
        }
    }

    /// The base URL its requests reach, and no other.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// `request` to `path` under its domain, carrying its key as HTTP Basic
    /// credentials.
    fn authorised(&self, method: Method, path: &str) -> Request {
        let url = format!("{}{path}", self.url);
        Request::new(method, SecretText::plain(url))
            .identified()
            .header(
                "Authorization",
                SecretText::plain("Basic ").then_secret(Secret::new(
                    self.key.name(),
                    herald_core::basic(USER, self.key.expose()),
                )),
            )
    }

    /// The `messages.mime` request carrying `written`: the whole message as
    /// a file of a form, each address of its envelope, `bcc` among them, as
    /// a `to` beside it, and tracking off.
    #[must_use]
    pub fn request(&self, written: &Written) -> Request {
        let mut form = String::new();
        let fields = written
            .to
            .iter()
            .map(|address| ("to", address.to_string()))
            .chain(
                UNTRACKED
                    .iter()
                    .map(|(name, value)| (*name, (*value).to_owned())),
            );
        for (name, value) in fields {
            form.extend([
                "--",
                FORM,
                "\r\nContent-Disposition: form-data; name=\"",
                name,
                "\"\r\n\r\n",
                &value,
                "\r\n",
            ]);
        }
        form.extend([
            "--",
            FORM,
            "\r\nContent-Disposition: form-data; name=\"message\"; filename=\"message.mime\"\r\n",
            "Content-Type: message/rfc822\r\n\r\n",
            &written.message,
            "\r\n--",
            FORM,
            "--\r\n",
        ]);
        self.authorised(Method::Post, &format!("/v3/{}/messages.mime", self.domain))
            .header(
                "Content-Type",
                SecretText::plain(format!("multipart/form-data; boundary=\"{FORM}\"")),
            )
            .body(form)
    }

    /// Sends `written`, and reads the answer.
    ///
    /// # Errors
    ///
    /// When the request got no response at all.
    pub async fn send(
        &self,
        http: &dyn DynHttp,
        written: &Written,
        waits: Waits,
    ) -> Result<Sent, HttpError> {
        let answer = http.send_boxed(self.request(written)).await?;
        Ok(sent(&answer, waits))
    }

    /// What Mailgun says of its domain, asked with its key, sending
    /// nothing: whether it takes the key, and sends as the domain to
    /// anyone.
    pub async fn probe(&self, http: &dyn DynHttp) -> Vec<Probe> {
        let request = self.authorised(Method::Get, &format!("/v4/domains/{}", self.domain));
        match http.send_boxed(request).await {
            Ok(answer) => domain(&answer),
            Err(error) => vec![Probe::Credential(format!(
                "{MAILGUN} could not be asked: {error}"
            ))],
        }
    }
}

/// What a `GET /v4/domains/{name}` answer says
/// (<https://documentation.mailgun.com/docs/mailgun/api-reference/send/mailgun/domains/get-v4-domains--name->).
/// A domain sending key may only send
/// (<https://documentation.mailgun.com/docs/mailgun/api-reference/authentication>),
/// so a key refused here may still send.
fn domain(answer: &Response) -> Vec<Probe> {
    if answer.refuses_credential() {
        return vec![Probe::Said(format!(
            "{MAILGUN} does not let its key read the domain, answering {}: a domain sending key \
             may only send, so whether this one sends is not asked; a key Mailgun does not take \
             is answered so too",
            answer.status
        ))];
    }
    if answer.status == status::NOT_FOUND {
        return vec![Probe::Credential(format!(
            "{MAILGUN} knows no such domain in its region, answering 404; check domain and region"
        ))];
    }
    if !answer.succeeded() {
        return vec![Probe::Credential(format!(
            "{MAILGUN} could not be asked, answering {}",
            answer.status
        ))];
    }
    let said = serde_json::from_slice::<Value>(&answer.body).unwrap_or_default();
    let field = |name| said.get("domain").and_then(|domain| domain.get(name));
    let mut probes = vec![Probe::Taken];
    if field("is_disabled").and_then(Value::as_bool) == Some(true) {
        probes.push(Probe::Route {
            key: None,
            why: format!("{MAILGUN} has the domain disabled"),
        });
    }
    if field("type").and_then(Value::as_str) == Some("sandbox") {
        probes.push(Probe::Said(format!(
            "the domain is a {MAILGUN} sandbox domain: it sends only to the recipients the \
             account authorised (https://documentation.mailgun.com/docs/mailgun/user-manual/domains/domains-sandbox)"
        )));
    }
    probes
}

/// What a `messages.mime` answer says: landed, its message id kept, where
/// Mailgun took the message; a wait, or that it refused it, otherwise. No
/// message Mailgun writes is repeated.
fn sent(answer: &Response, waits: Waits) -> Sent {
    if answer.status == RECIPIENTS_LIMITED {
        return Sent::RetryAfter(waits.limited);
    }
    answer
        .failure(waits, None, MAILGUN, String::new)
        .unwrap_or_else(|| landed(answer, "id"))
}

/// Whether `answer`, a client error, is Mailgun's own: a JSON object
/// carrying the `message` every refusal it documents carries.
pub(crate) fn declines(answer: &Response) -> bool {
    answer.is_json()
        && serde_json::from_slice::<Value>(&answer.body)
            .is_ok_and(|said| said.get("message").is_some_and(Value::is_string))
}
