use herald_aws::{Endpoint, Signing, signed};
use herald_core::{Sent, Waits, base64_of, media};
use herald_kinds::Probe;
use herald_ports::{DynHttp, HttpError, Method, Request, Response, Secret, SecretText};
use jiff::Timestamp;
use serde_json::{Map, Value, json};

use crate::email::landed;
use crate::message::{Recipients, Written};

/// Amazon SES, as herald names it.
pub(crate) const SES: &str = "Amazon SES";

/// The service a request to Amazon SES is signed for: the `service` its
/// SMTP passwords are derived under
/// (<https://docs.aws.amazon.com/ses/latest/dg/smtp-credentials.html>).
const SERVICE: &str = "ses";

/// Where `SendEmail` of the API version 2 is reached
/// (<https://docs.aws.amazon.com/ses/latest/APIReference-V2/API_SendEmail.html>).
const SEND: &str = "/v2/email/outbound-emails";

/// Where `GetAccount` is reached
/// (<https://docs.aws.amazon.com/ses/latest/APIReference-V2/API_GetAccount.html>).
const ACCOUNT: &str = "/v2/email/account";

/// The most recipients of one message, `To`, `Cc` and `Bcc` together
/// (<https://docs.aws.amazon.com/ses/latest/dg/quotas.html>, "Sender and
/// recipient quotas").
pub const SES_RECIPIENTS: usize = 50;

/// The most bytes of one message: 40 MB once in base64, read as
/// 40,000,000 bytes, the smaller reading (same page, "Message quotas").
pub const SES_BYTES: usize = 30_000_000;

/// The error Amazon SES answers 400 with where herald sends too often
/// (<https://docs.aws.amazon.com/ses/latest/APIReference-V2/CommonErrors.html>).
const THROTTLED: &str = "ThrottlingException";

/// The error Amazon SES answers 403 with where the credential is taken but
/// may not do what it asks (same page).
const DENIED: &str = "AccessDeniedException";

/// The longest error name herald repeats.
const NAMED: usize = 64;

/// Each region AWS lists an Amazon SES API endpoint for, and whether it
/// lists a FIPS endpoint there too
/// (<https://docs.aws.amazon.com/general/latest/gr/ses.html>, "Service API
/// endpoints").
const REGIONS: [(&str, bool); 34] = [
    ("us-east-2", true),
    ("us-east-1", true),
    ("us-west-1", true),
    ("us-west-2", true),
    ("af-south-1", false),
    ("ap-south-2", false),
    ("ap-southeast-3", false),
    ("ap-southeast-5", false),
    ("ap-southeast-4", false),
    ("ap-south-1", false),
    ("ap-southeast-6", false),
    ("ap-northeast-3", false),
    ("ap-northeast-2", false),
    ("ap-southeast-1", false),
    ("ap-southeast-2", false),
    ("ap-east-2", false),
    ("ap-southeast-7", false),
    ("ap-northeast-1", false),
    ("ca-central-1", true),
    ("ca-west-1", true),
    ("eu-central-1", false),
    ("eu-west-1", false),
    ("eu-west-2", false),
    ("eu-south-1", false),
    ("eu-west-3", false),
    ("eu-south-2", false),
    ("eu-north-1", false),
    ("eu-central-2", false),
    ("il-central-1", false),
    ("mx-central-1", false),
    ("me-south-1", false),
    ("sa-east-1", false),
    ("us-gov-east-1", true),
    ("us-gov-west-1", true),
];

/// Where Amazon SES is reached in a region, as AWS names its endpoints:
/// FIPS or not, on IPv4 alone or on IPv6 too.
///
/// # Errors
///
/// The key that names what AWS lists no endpoint for, and why.
pub fn regional(
    region: &str,
    fips: bool,
    dual_stack: bool,
) -> Result<String, (&'static str, String)> {
    let Some((_, has_fips)) = REGIONS.iter().find(|(listed, _)| *listed == region) else {
        return Err((
            "region",
            "AWS lists no Amazon SES endpoint in it; give a region it lists, such as \
             \"eu-west-1\", or the endpoint's URL as endpoint"
                .to_owned(),
        ));
    };
    if fips && !has_fips {
        return Err((
            "fips",
            "AWS lists no FIPS endpoint for Amazon SES in this region; drop it".to_owned(),
        ));
    }
    let service = if fips { "email-fips" } else { "email" };
    let suffix = if dual_stack {
        "api.aws"
    } else {
        "amazonaws.com"
    };
    Ok(format!("https://{service}.{region}.{suffix}"))
}

/// The credentials a request to Amazon SES is signed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    /// The access key's id.
    pub access_key_id: Secret,
    /// The secret access key, which leaves only as the signature it makes.
    pub secret_access_key: Secret,
    /// The session token of temporary credentials.
    pub session_token: Option<Secret>,
}

/// Amazon SES in one region, reached with one set of credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ses {
    endpoint: Endpoint,
    region: String,
    credentials: Credentials,
    configuration_set: Option<String>,
}

impl Ses {
    /// Amazon SES in `region`, at the base URL `url`, signed with
    /// `credentials`, sending each message with `configuration_set` where it
    /// names one.
    #[must_use]
    pub fn new(
        url: &str,
        region: &str,
        credentials: Credentials,
        configuration_set: Option<String>,
    ) -> Self {
        Self {
            endpoint: Endpoint::new(SERVICE, region, Some(url)),
            region: region.to_owned(),
            credentials,
            configuration_set,
        }
    }

    /// The base URL its requests reach, and no other.
    #[must_use]
    pub fn url(&self) -> &str {
        self.endpoint.url()
    }

    /// `request` to `path`, signed at `now`.
    fn signed(&self, request: Request, path: &str, kind: Option<&str>, now: Timestamp) -> Request {
        let signing = Signing {
            access_key_id: &self.credentials.access_key_id,
            secret_access_key: &self.credentials.secret_access_key,
            session_token: self.credentials.session_token.as_ref(),
            region: &self.region,
            service: SERVICE,
            host: self.endpoint.host(),
        };
        signed(
            request,
            &self.endpoint.canonical(path),
            kind,
            &[],
            &signing,
            now,
        )
    }

    /// The `SendEmail` request carrying `written` to `recipients`, signed at
    /// `now`: the whole message in base64, its sender, its recipients each
    /// in the list it stands in, `bcc` among them outside the message, and
    /// open and click tracking off.
    #[must_use]
    pub fn request(&self, written: &Written, recipients: &Recipients, now: Timestamp) -> Request {
        let mut destination = Map::new();
        for (field, listed) in [
            ("ToAddresses", recipients.to()),
            ("CcAddresses", recipients.cc()),
            ("BccAddresses", recipients.bcc()),
        ] {
            if !listed.is_empty() {
                let addresses = listed
                    .iter()
                    .map(|mailbox| Value::String(mailbox.address().to_string()));
                destination.insert(field.to_owned(), Value::Array(addresses.collect()));
            }
        }
        let mut body = json!({
            "Content": { "Raw": { "Data": base64_of(written.message.as_bytes()) } },
            "Destination": destination,
            "FromEmailAddress": written.sender.to_string(),
            "ConfigurationOverrides": {
                "Tracking": { "ClickTrackingEnabled": "DISABLED", "OpenTrackingEnabled": "DISABLED" }
            },
        });
        if let (Some(set), Some(fields)) = (&self.configuration_set, body.as_object_mut()) {
            fields.insert("ConfigurationSetName".to_owned(), json!(set));
        }
        let request = Request::new(Method::Post, SecretText::plain(self.endpoint.at(SEND)))
            .identified()
            .body(body.to_string());
        self.signed(request, SEND, Some(media::JSON), now)
    }

    /// Sends `written` to `recipients` at `now`, and reads the answer.
    ///
    /// # Errors
    ///
    /// When the request got no response at all.
    pub async fn send(
        &self,
        http: &dyn DynHttp,
        (written, recipients): (&Written, &Recipients),
        now: Timestamp,
        waits: Waits,
    ) -> Result<Sent, HttpError> {
        let answer = http
            .send_boxed(self.request(written, recipients, now))
            .await?;
        Ok(sent(&answer, waits))
    }

    /// What Amazon SES says, asked at `now` for the account its
    /// credentials sign for, sending nothing: whether it takes them, sends,
    /// and sends beyond the addresses it verified.
    pub async fn probe(&self, http: &dyn DynHttp, now: Timestamp) -> Vec<Probe> {
        let request =
            Request::new(Method::Get, SecretText::plain(self.endpoint.at(ACCOUNT))).identified();
        match http
            .send_boxed(self.signed(request, ACCOUNT, None, now))
            .await
        {
            Ok(answer) => self.account(&answer),
            Err(error) => vec![Probe::Credential(format!(
                "{SES} could not be asked: {error}"
            ))],
        }
    }

    /// What its `GetAccount` answer says.
    fn account(&self, answer: &Response) -> Vec<Probe> {
        let named = error(answer);
        if named.as_deref() == Some(DENIED) {
            return vec![Probe::Said(format!(
                "{SES} takes its credential, which may not read the account (ses:GetAccount), \
                 so whether it may send in {} is not asked",
                self.region
            ))];
        }
        if !answer.succeeded() {
            let said = named.map(|named| format!(": {named}")).unwrap_or_default();
            return vec![Probe::Credential(format!(
                "{SES} refuses its credential, answering {}{said}",
                answer.status
            ))];
        }
        let account = serde_json::from_slice::<Value>(&answer.body).unwrap_or_default();
        let flag = |name| account.get(name).and_then(Value::as_bool);
        let mut probes = vec![Probe::Taken];
        if flag("SendingEnabled") == Some(false) {
            probes.push(Probe::Route {
                key: None,
                why: format!(
                    "{SES} has sending paused for the account in {}",
                    self.region
                ),
            });
        }
        if flag("ProductionAccessEnabled") == Some(false) {
            probes.push(Probe::Said(format!(
                "the account is in {SES}'s sandbox in {}: it sends only to the addresses and \
                 domains it verified",
                self.region
            )));
        }
        probes
    }
}

/// The name of the error `answer` carries, as AWS's REST JSON protocol
/// names it, where it names one herald repeats: its `X-Amzn-Errortype`
/// header, else its body's `__type` or `code`, cut before any `:` and after
/// any `#`, of ASCII letters and digits alone
/// (<https://smithy.io/2.0/aws/protocols/aws-restjson1-protocol.html>,
/// "Operation error serialization").
pub(crate) fn error(answer: &Response) -> Option<String> {
    let body = serde_json::from_slice::<Value>(&answer.body).unwrap_or_default();
    let named = answer.header("x-amzn-errortype").or_else(|| {
        ["__type", "code"]
            .iter()
            .find_map(|field| body.get(field).and_then(Value::as_str))
    })?;
    let named = named.split(':').next().unwrap_or_default();
    let named = named.split_once('#').map_or(named, |(_, shape)| shape);
    (!named.is_empty()
        && named.len() <= NAMED
        && named.bytes().all(|byte| byte.is_ascii_alphanumeric()))
    .then(|| named.to_owned())
}

/// What a `SendEmail` answer says: landed, its message id kept, where
/// Amazon SES took the message; a wait, or why it refused it, otherwise.
fn sent(answer: &Response, waits: Waits) -> Sent {
    let named = error(answer);
    if named.as_deref() == Some(THROTTLED) {
        return Sent::RetryAfter(waits.limited);
    }
    answer
        .failure(waits, None, SES, || named.unwrap_or_default())
        .unwrap_or_else(|| landed(answer, "MessageId"))
}

/// Whether `answer`, a client error, is Amazon SES's own: it names an
/// error as AWS's REST JSON protocol does.
pub(crate) fn declines(answer: &Response) -> bool {
    error(answer).is_some()
}
