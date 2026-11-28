//! A Bot Framework bot: it signs in with its app registration's client
//! secret for a token held in memory alone, then posts, replies, edits and
//! deletes through its service URL (ADR-0093).

use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};

use herald_core::{
    Announcement, Declined, Entry, Facts, Phrase, Sent, Step, Waits, Wording, media, status,
    unreserved,
};
use herald_kinds::Probe;
use herald_ports::{DynHttp, HttpError, Method, Request, Response, Secret, SecretText};
use jiff::{SignedDuration, Timestamp};
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::card::{Measured, Offers, announced, chunk, of_part};
use crate::cloud::Hosts;
use crate::options::RouteOptions;
use crate::request::JSON;
use crate::teams::{TEAMS, said};

/// The most bytes a bot's message holds, counted as UTF-16: 80 KB, read
/// as thousands of bytes, the size Microsoft says guarantees delivery
/// within its approximate 100 KB
/// (<https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/format-your-bot-messages>,
/// "Message size limits").
pub const BOT_MESSAGE: usize = 80_000;

/// The sign-in service, as a refusal names it.
pub(crate) const LOGIN: &str = "Microsoft's sign-in service";

/// The name the bot's token goes by wherever a request shows it.
pub(crate) const TOKEN: &str = "the Microsoft Teams bot's token";

/// What a token answer may hold, in bytes.
const TOKEN_ANSWER: usize = 16_384;

/// The longest a token is held, as Microsoft gives one for 3600 seconds
/// (<https://learn.microsoft.com/en-us/azure/bot-service/rest-api/bot-framework-rest-connector-authentication?view=azure-bot-service-4.0>,
/// "Step 2").
const LIFETIME: SignedDuration = SignedDuration::from_secs(3600);

/// How long before its end a held token is no longer sent.
const MARGIN: SignedDuration = SignedDuration::from_secs(300);

/// What doctor says of a route whose parts reply in their post's thread.
pub(crate) const PREVIEW: &str = "its overflow parts reply in the post's thread through replyChains, which Microsoft marks as a preview (https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/conversations/agent-chat-threads); leave threads unset to post each part on its own";

const fn cannot(what: &'static str) -> Declined {
    Declined::Cannot {
        platform: TEAMS,
        what,
    }
}

/// The token a sign-in answers, as much of it as herald reads.
#[derive(Deserialize)]
struct Answered {
    access_token: Option<String>,
    expires_in: Option<i64>,
}

/// What a refused sign-in names: its error and Microsoft Entra's codes.
#[derive(Deserialize)]
struct Refusal {
    error: Option<String>,
    #[serde(default)]
    error_codes: Vec<u64>,
}

/// The token the last sign-in gave, and when it stops being sent.
#[derive(Default)]
struct Held(RwLock<Option<(Secret, Timestamp)>>);

impl Held {
    fn current(&self, now: Timestamp) -> Option<Secret> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .filter(|(_, until)| now < *until)
            .map(|(token, _)| token.clone())
    }

    fn keep(&self, token: Secret, until: Timestamp) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = Some((token, until));
    }
}

/// One bot, posting to one conversation. Its debug form names its app and
/// its secret alone.
#[derive(Clone)]
pub struct Bot {
    app_id: String,
    password: Secret,
    login: String,
    tenant: String,
    scope: String,
    service: String,
    conversation: String,
    threads: bool,
    button: bool,
    wording: Wording,
    bases: Vec<String>,
    held: Arc<Held>,
}

impl PartialEq for Bot {
    fn eq(&self, other: &Self) -> bool {
        self.app_id == other.app_id
            && self.password == other.password
            && (&self.login, &self.tenant, &self.scope, &self.service)
                == (&other.login, &other.tenant, &other.scope, &other.service)
            && (&self.conversation, self.threads, self.button)
                == (&other.conversation, other.threads, other.button)
            && self.wording == other.wording
            && Arc::ptr_eq(&self.held, &other.held)
    }
}

impl Eq for Bot {}

impl fmt::Debug for Bot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bot")
            .field("app_id", &self.app_id)
            .field("password", &self.password)
            .finish_non_exhaustive()
    }
}

/// The origin of `url`, where it has one.
fn origin(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .map(|url| url.origin().ascii_serialization())
        .filter(|origin| origin != "null")
}

/// `fields` as an `application/x-www-form-urlencoded` body.
fn form(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(name, value)| format!("{name}={}", unreserved(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// What a refused sign-in said: its error and Microsoft Entra's first
/// code, never its description.
fn refused(response: &Response) -> String {
    let refusal = serde_json::from_slice::<Refusal>(&response.body).ok();
    let error = refusal
        .as_ref()
        .and_then(|refusal| refusal.error.as_deref())
        .filter(|error| {
            error.len() <= 64
                && error
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        });
    let code = refusal
        .as_ref()
        .and_then(|refusal| refusal.error_codes.first())
        .map(|code| format!("AADSTS{code}"));
    match (error, code) {
        (Some(error), Some(code)) => format!("{error} ({code})"),
        (Some(said), None) => said.to_owned(),
        (None, Some(said)) => said,
        (None, None) => String::new(),
    }
}

/// Why a sign-in that succeeded gave no token.
fn tokenless() -> String {
    format!("{LOGIN} answered with no token herald can send")
}

/// Why a sign-in that gave no token, as `sent` says, leaves an answer
/// unsent.
fn unsigned(sent: Sent) -> String {
    match sent {
        Sent::RetryAfter(wait) => format!("{LOGIN} asked herald to wait {wait:#} to sign in"),
        Sent::Refused(why) => why,
        Sent::Landed(_) => tokenless(),
    }
}

/// The token `answer` holds and how long it lasts, where it holds one
/// herald can send; its body wiped either way.
fn minted(answer: &mut Response) -> Option<(Secret, SignedDuration)> {
    let read = serde_json::from_slice::<Answered>(&answer.body).ok();
    answer.wipe();
    let Answered {
        access_token,
        expires_in,
    } = read?;
    let token = access_token.filter(|token| sendable(token))?;
    let lasts = expires_in
        .map_or(SignedDuration::ZERO, SignedDuration::from_secs)
        .clamp(SignedDuration::ZERO, LIFETIME);
    Some((Secret::new(TOKEN, token), lasts))
}

/// What a sign-in's `answer` gives: a token and how long it lasts, a
/// wait, or why not; its body wiped either way.
fn signed(mut answer: Response, waits: Waits) -> Result<(Secret, SignedDuration), Sent> {
    if let Some(failed) = answer.failure(waits, None, LOGIN, || refused(&answer)) {
        answer.wipe();
        return Err(failed);
    }
    minted(&mut answer).ok_or_else(|| Sent::Refused(tokenless()))
}

/// What a sign-in's `answer` says of the bot's credential; its body wiped
/// either way.
fn probed(mut answer: Response) -> Probe {
    let taken = match answer.accepted(None, LOGIN, || refused(&answer)) {
        Ok(()) if minted(&mut answer).is_some() => Probe::Taken,
        Ok(()) => Probe::Credential(tokenless()),
        Err(why) => Probe::Credential(why),
    };
    answer.wipe();
    taken
}

/// Whether `token` may go in a header as it is: printable ASCII, no space.
fn sendable(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_graphic())
}

impl Bot {
    /// The bot `app_id`, its client secret `password`, signing in and
    /// posting through `hosts`, to the conversation `route` names.
    #[must_use]
    pub fn new(app_id: &str, password: Secret, hosts: Hosts<'_>, route: &RouteOptions) -> Self {
        let login = hosts.login.trim_end_matches('/').to_owned();
        let service = hosts.service.trim_end_matches('/').to_owned();
        let bases = [&login, &service]
            .into_iter()
            .filter_map(|base| origin(base))
            .collect();
        Self {
            app_id: app_id.to_owned(),
            password,
            login,
            tenant: hosts.tenant.to_owned(),
            scope: hosts.scope.trim_end_matches('/').to_owned(),
            service,
            conversation: route.conversation.clone().unwrap_or_default(),
            threads: route.threads.unwrap_or_default(),
            button: route
                .subscribe
                .as_ref()
                .is_some_and(|subscribe| subscribe.button),
            wording: Wording::default(),
            bases,
            held: Arc::default(),
        }
    }

    /// The bot, labelling its mentions and its subscribe button as
    /// `wording` words them.
    #[must_use]
    pub fn worded(self, wording: Wording) -> Self {
        Self { wording, ..self }
    }

    /// What its cards carry beyond an announcement.
    fn offers(&self) -> Offers<'_> {
        Offers {
            mentioned: Some(self.wording.mentioned()),
            button: self
                .button
                .then(|| self.wording.text(Phrase::SubscribeButton)),
        }
    }

    /// The origins it reaches: its sign-in service's and its service URL's.
    #[must_use]
    pub fn bases(&self) -> &[String] {
        &self.bases
    }

    /// Whether its overflow parts reply in their post's thread.
    #[must_use]
    pub const fn threads(&self) -> bool {
        self.threads
    }

    /// The sign-in that asks for its token: the client credentials grant
    /// for its scope's `.default`, its body sealed
    /// (<https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-client-creds-grant-flow>).
    fn signing_in(&self) -> Result<Request, Declined> {
        let tenant = Declined::segment(&self.tenant, "bot's tenant")?;
        let scope = format!("{}/.default", self.scope);
        let body = form(&[
            ("grant_type", "client_credentials"),
            ("client_id", &self.app_id),
            ("client_secret", self.password.expose()),
            ("scope", &scope),
        ]);
        Ok(Request::new(
            Method::Post,
            SecretText::plain(format!("{}/{tenant}/oauth2/v2.0/token", self.login)),
        )
        .identified()
        .header(
            "Content-Type",
            SecretText::plain("application/x-www-form-urlencoded"),
        )
        .accepting(media::JSON)
        .at_most(TOKEN_ANSWER)
        .sealed_body(body))
    }

    /// A token from its sign-in, and how long it lasts; why not otherwise.
    async fn signed_in(
        &self,
        http: &dyn DynHttp,
        waits: Waits,
    ) -> Result<Result<(Secret, SignedDuration), Sent>, HttpError> {
        let request = match self.signing_in() {
            Ok(request) => request,
            Err(declined) => return Ok(Err(Sent::Refused(declined.to_string()))),
        };
        Ok(signed(http.send_boxed(request).await?, waits))
    }

    /// The token its answers to commands carry at `now`: the one it holds,
    /// or a new one; why it has none otherwise.
    pub(crate) async fn answering(
        &self,
        http: &dyn DynHttp,
        now: Timestamp,
        waits: Waits,
    ) -> Result<Secret, String> {
        match self.token(http, now, waits).await {
            Ok(token) => token.map_err(unsigned),
            Err(error) => Err(error.to_string()),
        }
    }

    /// The token it sends at `now`: the one it holds, or a new one, held
    /// until shortly before it ends.
    async fn token(
        &self,
        http: &dyn DynHttp,
        now: Timestamp,
        waits: Waits,
    ) -> Result<Result<Secret, Sent>, HttpError> {
        if let Some(token) = self.held.current(now) {
            return Ok(Ok(token));
        }
        Ok(self.kept(self.signed_in(http, waits).await?, now))
    }

    /// The token a sign-in at `now` gave, held until shortly before it
    /// ends; why it gave none otherwise.
    fn kept(
        &self,
        signed: Result<(Secret, SignedDuration), Sent>,
        now: Timestamp,
    ) -> Result<Secret, Sent> {
        let (token, lasts) = signed?;
        if let Ok(until) = now.checked_add(lasts - MARGIN) {
            self.held.keep(token.clone(), until);
        }
        Ok(token)
    }

    /// The URL of `path` under its conversation.
    fn conversation_url(&self, path: &str) -> Result<String, Declined> {
        let conversation = Declined::segment(&self.conversation, "route's conversation")?;
        Ok(format!(
            "{}/v3/conversations/{conversation}{path}",
            self.service
        ))
    }

    /// The URL of the message `id`, as an earlier step recorded it.
    fn activity(&self, id: Option<&str>) -> Result<String, Declined> {
        let id = id.ok_or(Declined::Missing("the message"))?;
        let id = Declined::segment(id, "message's id")?;
        self.conversation_url(&format!("/activities/{id}"))
    }

    fn sent(method: Method, url: String, card: Option<&Value>) -> Result<Request, String> {
        let request = Request::new(method, SecretText::plain(url)).identified();
        match card {
            Some(card) => {
                let body = Measured::Utf16(BOT_MESSAGE).fitted(card.to_string())?;
                Ok(request
                    .header("Content-Type", SecretText::plain(JSON))
                    .body(body))
            }
            None => Ok(request),
        }
    }

    /// The request for `step` of `announcement`, its token not yet sent;
    /// why the bot cannot take it otherwise.
    pub(crate) fn request(
        &self,
        step: &Step,
        announcement: &Announcement,
        entry: &Entry,
    ) -> Result<Request, String> {
        let (method, url, card) = match step {
            Step::Post => (
                Method::Post,
                self.conversation_url("/activities"),
                Some(announced(announcement, self.offers())),
            ),
            Step::Chunk(index) if self.threads => {
                let root = entry
                    .message(&Step::Post)
                    .ok_or(Declined::Missing("the announcement's message"))
                    .and_then(|root| Declined::segment(root, "announcement's message id"));
                (
                    Method::Post,
                    root.and_then(|root| self.conversation_url(&format!("/replyChains/{root}"))),
                    Some(chunk(announcement, *index, self.offers())),
                )
            }
            Step::Chunk(index) => (
                Method::Post,
                self.conversation_url("/activities"),
                Some(chunk(announcement, *index, self.offers())),
            ),
            Step::Edit { part, .. } => (
                Method::Put,
                self.activity(entry.part(*part)),
                Some(of_part(announcement, *part, self.offers())),
            ),
            Step::Delete(part) => (Method::Delete, self.activity(entry.part(*part)), None),
            Step::Crosspost => return Err(cannot("crosspost").to_string()),
            Step::OpenThread | Step::Rename(_) => {
                return Err(cannot("open a thread; its replies are the thread").to_string());
            }
            Step::Pin | Step::Unpin => return Err(cannot("pin").to_string()),
            Step::Deliver(_) | Step::Deliveries | Step::Updates(_) | Step::Deletes => {
                return Err(cannot("deliver to an inbox").to_string());
            }
        };
        let url = url.map_err(|declined| declined.to_string())?;
        Self::sent(method, url, card.as_ref())
    }

    /// Sends `step` of `announcement` at `now`, signing in first where it
    /// holds no token, and reads Teams' answer.
    ///
    /// # Errors
    ///
    /// When a request got no response at all.
    pub async fn send(
        &self,
        http: &dyn DynHttp,
        (step, announcement, entry): (&Step, &Announcement, &Entry),
        now: Timestamp,
        waits: Waits,
    ) -> Result<Sent, HttpError> {
        let request = match self.request(step, announcement, entry) {
            Ok(request) => request,
            Err(refusal) => return Ok(Sent::Refused(refusal)),
        };
        let token = match self.token(http, now, waits).await? {
            Ok(token) => token,
            Err(unsigned) => return Ok(unsigned),
        };
        Ok(read(&http.send_boxed(request.bearer(token)).await?, waits))
    }

    /// What its sign-in says of its app id and client secret, with
    /// nothing posted, and what doctor names of a route replying in
    /// threads.
    pub(crate) async fn probe(&self, http: &dyn DynHttp) -> Vec<Probe> {
        let request = match self.signing_in() {
            Ok(request) => request,
            Err(declined) => return vec![Probe::Credential(declined.to_string())],
        };
        let taken = match http.send_boxed(request).await {
            Ok(answer) => probed(answer),
            Err(error) => Probe::credential(LOGIN, &Err(error)),
        };
        let mut probes = vec![taken];
        if self.threads {
            probes.push(Probe::Said(PREVIEW.to_owned()));
        }
        probes
    }
}

/// What an answer to the bot says: a wait where Teams asks for one, 412
/// among them, as Microsoft says to retry it; the message it made where it
/// took the request; why it refused it otherwise
/// (<https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/rate-limit>).
fn read(response: &Response, waits: Waits) -> Sent {
    if response.status == status::PRECONDITION_FAILED {
        return Sent::RetryAfter(response.retry_after().unwrap_or(waits.outage));
    }
    if let Some(failed) = response.failure(waits, None, TEAMS, || said(response)) {
        return failed;
    }
    let message = serde_json::from_slice::<Value>(&response.body)
        .ok()
        .as_ref()
        .and_then(|body| body.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned);
    Sent::Landed(Facts {
        message,
        ..Facts::default()
    })
}

/// Whether `answer` is the Bot Framework's own error envelope, an
/// `ErrorResponse` whose `error` names its code
/// (<https://learn.microsoft.com/en-us/azure/bot-service/rest-api/bot-framework-rest-connector-api-reference?view=azure-bot-service-4.0>,
/// "Errors").
pub(crate) fn declines(answer: &Response) -> bool {
    serde_json::from_slice::<Value>(&answer.body)
        .ok()
        .as_ref()
        .and_then(|body| body.get("error"))
        .and_then(|error| error.get("code"))
        .is_some_and(Value::is_string)
}

#[cfg(test)]
mod tests;
