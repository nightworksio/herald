//! What applies a planned re-run on GitHub or GitLab: the operator's
//! sign-in by the forge's device flow, what their token may do and their
//! role on the repository, the job read right before the call, and the call
//! re-running that one job, made with the source's write credential alone
//! (ADR-0068).

use herald_core::media::{FORM, JSON};
use herald_core::{Address, Rerun, unreserved};
use herald_kinds::{Credential, Device, Granted, Polled, Rerunner, RunNow, SignIn, Unpolled};
use herald_ports::{
    Boxed, DynHttp, Erased, Http as _, HttpError, Method, OperatorToken, Request, Response, Secret,
    SecretText,
};
use std::collections::{BTreeMap, BTreeSet};

use jiff::SignedDuration;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::forge::{REPOSITORY, Token, authorized, named, whole};
use crate::options::{read_only, unread};
use crate::queue::{PAGE, failed};
use crate::{Forge, gitlab};

/// What a run's or a job's id is named as where it cannot be asked for.
const RUN: &str = "run's or job's id";

/// What a GitHub App's slug is named as where it cannot be asked for.
const APP_SLUG: &str = "App's slug";

/// How long herald waits between asks where the forge names no interval.
const INTERVAL: SignedDuration = SignedDuration::from_secs(5);

/// How long a device code lasts where the forge names no expiry.
const EXPIRES: SignedDuration = SignedDuration::from_mins(15);

/// The grant a device code is exchanged under (RFC 8628).
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// The header GitHub names an OAuth token's scopes in.
const GITHUB_SCOPES_HEADER: &str = "X-OAuth-Scopes";

/// Where GitLab says what an OAuth token holds, on its web root.
const GITLAB_TOKEN_INFO: &str = "/oauth/token/info";

/// Where GitLab revokes an OAuth token, on its web root.
const GITLAB_REVOKE: &str = "/oauth/revoke";

/// The most pages of a pipeline's jobs read to learn whether a job was
/// retried.
const MOST_JOB_PAGES: usize = 10;

/// The most pages of the installations a GitHub App's user token reaches
/// read to learn what it may do.
const MOST_INSTALLATION_PAGES: usize = 10;

/// The one permission level an installation a GitHub App's user token
/// reaches may hold.
const READ: &str = "read";

/// The least GitLab access level of each permission: Developer,
/// Maintainer and Owner.
const GITLAB_LEVELS: [(herald_core::Permission, u64); 3] = [
    (herald_core::Permission::Write, 30),
    (herald_core::Permission::Maintain, 40),
    (herald_core::Permission::Admin, 50),
];

/// What the operator is told where the code expired.
pub const EXPIRED: &str = "the code expired before the sign-in finished; run the command again";

/// What the operator is told where they declined the sign-in.
pub const DENIED: &str = "the sign-in was declined at the forge, so nothing was re-run";

/// What the operator is told where the forge ended the sign-in otherwise.
pub const ENDED: &str = "the forge ended the sign-in without a token; check sign_in.client_id and that the application has the device flow turned on";

/// What the operator is told where a run's or a job's id is not a number.
pub const UNNUMBERED: &str = "a run's or a job's id is a number, so it is not asked for";

/// What the operator is told where the forge named a sign-in page off its
/// own web root.
pub const ELSEWHERE: &str = "the forge named a page to sign in at that is not on the sign-in's own web root over https, so herald shows none and signs nobody in";

/// What the operator is told where a pipeline holds more jobs than herald
/// reads.
pub const UNCOUNTED: &str = "the pipeline holds more jobs than herald reads, so whether this job was retried is unknown and herald leaves it";

/// The documented prefix of a GitHub OAuth application's token.
pub const GITHUB_OAUTH_TOKEN: &str = "gho_";

/// The documented prefix of a GitHub App's user token.
pub const GITHUB_APP_USER_TOKEN: &str = "ghu_";

/// What a GitHub token an operator signs in with is, by its documented
/// prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GithubToken {
    /// An OAuth application's: the scopes GitHub names decide.
    OAuth,
    /// A GitHub App's user token: the installations it reaches decide.
    AppUser,
}

/// Each GitHub token an operator may sign in with, by its prefix.
const GITHUB_TOKENS: [(&str, GithubToken); 2] = [
    (GITHUB_OAUTH_TOKEN, GithubToken::OAuth),
    (GITHUB_APP_USER_TOKEN, GithubToken::AppUser),
];

/// What the operator is told where GitHub named no scopes for their OAuth
/// application's token.
pub const UNSCOPED: &str = "GitHub named no scopes for the operator's OAuth token, so what it may do is unknown and herald signs nobody in with it";

/// What the operator is told where the installations or the App their
/// GitHub App's user token reaches could not be read whole.
pub const UNINSTALLED: &str = "the installations and the App the operator's GitHub App token reaches could not be read whole, so what it may do is unknown and herald signs nobody in with it";

/// What the operator is told where their token is neither an OAuth
/// application's nor a GitHub App's user token.
pub const UNKNOWN_TOKEN: &str = "the operator's token is neither an OAuth application's nor a GitHub App's user token, so herald signs nobody in with it";

/// What the operator is told where their GitHub App's user token reaches
/// no installation naming its App.
pub const UNNAMED_APP: &str = "the operator's GitHub App token reaches no installation naming its App, so the App's permissions cannot be read and herald signs nobody in with it; install the sign-in App on the repository's account";

/// What the operator is told where their token reaches an installation of
/// another App than the one they signed in through.
pub const FOREIGN_APP: &str = "the operator's token reaches an installation of another App than sign_in.client_id names, so it is not that App's user token and herald signs nobody in with it";

/// What the operator is told where their token reaches an installation of
/// the write App.
pub const WRITER_REACHED: &str = "the operator's token reaches an installation of the GitHub App the source writes as, so it is that App's user token; give sign_in the client id of an application that only reads";

#[derive(Deserialize)]
struct DeviceAnswer {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: Option<i64>,
    interval: Option<i64>,
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct TokenInfo {
    #[serde(default)]
    scope: Vec<String>,
}

#[derive(Deserialize)]
struct User {
    login: Option<String>,
    username: Option<String>,
}

#[derive(Deserialize)]
struct Reached {
    installations: Vec<ReachedInstallation>,
}

#[derive(Deserialize)]
struct ReachedInstallation {
    app_id: Option<u64>,
    app_slug: Option<String>,
    client_id: Option<String>,
    #[serde(default)]
    permissions: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct ReachedApp {
    permissions: BTreeMap<String, String>,
}

#[derive(Deserialize, Default)]
struct GithubPermissions {
    #[serde(default)]
    admin: bool,
    #[serde(default)]
    maintain: bool,
    #[serde(default)]
    push: bool,
}

#[derive(Deserialize)]
struct GithubRepository {
    permissions: Option<GithubPermissions>,
}

#[derive(Deserialize)]
struct Access {
    access_level: u64,
}

#[derive(Deserialize, Default)]
struct GitlabPermissions {
    project_access: Option<Access>,
    group_access: Option<Access>,
}

#[derive(Deserialize)]
struct GitlabProject {
    #[serde(default)]
    permissions: GitlabPermissions,
}

#[derive(Deserialize)]
struct GithubPull {
    state: String,
    head: GithubHead,
}

#[derive(Deserialize)]
struct GithubHead {
    sha: String,
}

#[derive(Deserialize)]
struct GitlabMerge {
    state: String,
    sha: Option<String>,
}

#[derive(Deserialize)]
struct GithubJob {
    run_id: u64,
    head_sha: String,
    status: Option<String>,
    conclusion: Option<String>,
}

#[derive(Deserialize)]
struct GithubRun {
    run_attempt: Option<u64>,
}

#[derive(Deserialize)]
struct GitlabPipeline {
    id: u64,
    sha: String,
}

#[derive(Deserialize)]
struct GitlabJob {
    name: String,
    status: String,
    pipeline: GitlabPipeline,
}

#[derive(Deserialize)]
struct GitlabListedJob {
    name: String,
    #[serde(default)]
    retried: bool,
}

/// A GitHub or GitLab source's re-runs: its API, its read and write
/// credentials, and how an operator signs in.
#[derive(Debug)]
pub struct ForgeRerunner {
    /// The forge.
    pub forge: Forge,
    /// The REST API's base URL.
    pub api: String,
    /// The most of an answer read.
    pub parse: usize,
    /// What it reads with.
    pub read: Option<Token>,
    /// The write credential's name.
    pub credential: String,
    /// How an operator signs in, or why none can.
    pub signing: Result<SignIn, &'static str>,
}

/// `fields` as a form's body, each value URL-encoded.
fn form(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(name, value)| format!("{name}={}", unreserved(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// The answer to `request`, sent through `http`, read as `what` where it
/// succeeded.
async fn succeeded<T: DeserializeOwned>(
    http: &dyn DynHttp,
    request: Request,
    (what, parse): (&'static str, usize),
) -> Result<T, Unpolled> {
    let answer = Erased(http)
        .send(request)
        .await
        .map_err(Unpolled::unreachable)?;
    herald_wire::answered(what, &answer).map_err(Unpolled::unreadable)?;
    herald_wire::read(what, parse, &answer.body).map_err(Unpolled::unreadable)
}

/// Whether `page`, a page the forge names to sign in at, is on `web` over
/// https.
fn on_web(page: &str, web: &str) -> bool {
    let origin = |url: &str| Address::parse(url).map(|address| address.origin());
    Address::parse(page).is_some_and(|address| address.scheme() == "https")
        && origin(page).is_some()
        && origin(page) == origin(web)
}

/// A whole number of seconds, or `default` where the forge named none
/// above zero.
fn seconds(named: Option<i64>, default: SignedDuration) -> SignedDuration {
    named
        .filter(|seconds| *seconds > 0)
        .map_or(default, SignedDuration::from_secs)
}

impl ForgeRerunner {
    /// The sign-in, where an operator can sign in.
    fn sign_in(&self) -> Result<&SignIn, Unpolled> {
        self.signing
            .as_ref()
            .map_err(|reason| Unpolled::Unreadable((*reason).to_owned()))
    }

    /// The base URL of everything asked of `repository`.
    fn base(&self, repository: &str) -> Result<String, Unpolled> {
        let named = named(repository, REPOSITORY)?;
        Ok(match self.forge {
            Forge::GitHub | Forge::Forgejo => format!("{}/repos/{named}", self.api),
            Forge::GitLab => gitlab::project(&self.api, repository),
        })
    }

    /// A run's or a job's id, a number, as one segment of a path.
    fn numbered(id: &str) -> Result<String, Unpolled> {
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(Unpolled::Unreadable(UNNUMBERED.to_owned()));
        }
        whole(id, RUN)
    }

    /// A read of `url` with the source's read credential, read as `what`.
    async fn read<T: DeserializeOwned>(
        &self,
        api: &dyn DynHttp,
        url: String,
        what: &'static str,
    ) -> Result<T, Unpolled> {
        let request = self.request(Method::Get, url, self.read.as_ref());
        succeeded(api, request, (what, self.parse)).await
    }

    /// How many times the jobs named `name` of the GitLab pipeline at
    /// `pipeline` were retried, every page read.
    ///
    /// # Errors
    ///
    /// Where a page could not be read, or the pipeline holds more pages
    /// than herald reads.
    async fn retried(
        &self,
        api: &dyn DynHttp,
        pipeline: &str,
        name: &str,
    ) -> Result<u64, Unpolled> {
        let mut retried = 0;
        for page in 1..=MOST_JOB_PAGES {
            let url = format!("{pipeline}/jobs?include_retried=true&per_page={PAGE}&page={page}");
            let jobs: Vec<GitlabListedJob> = self.read(api, url, "a pipeline's jobs").await?;
            retried += jobs
                .iter()
                .filter(|job| job.retried && job.name == name)
                .count() as u64;
            if jobs.len() < PAGE {
                return Ok(retried);
            }
        }
        Err(Unpolled::Unreadable(UNCOUNTED.to_owned()))
    }

    /// What a GitHub `token` is by its documented prefix, where it is one an
    /// operator may sign in with.
    fn github_token(token: &OperatorToken) -> Option<GithubToken> {
        let sent = token.sent();
        GITHUB_TOKENS
            .iter()
            .find(|(prefix, _)| sent.expose().starts_with(prefix))
            .map(|(_, kind)| *kind)
    }

    /// The first permission of `permissions` whose level is not read.
    fn wider(permissions: &BTreeMap<String, String>) -> Option<(&String, &String)> {
        permissions.iter().find(|(_, level)| level.as_str() != READ)
    }

    /// The answer to the operator's `token` asking `url` through `api`,
    /// read as `what`, or why the installations and the App could not be
    /// read whole.
    async fn reached<T: DeserializeOwned>(
        &self,
        api: &dyn DynHttp,
        token: &OperatorToken,
        (url, what): (String, &'static str),
    ) -> Result<T, Unpolled> {
        succeeded(api, self.operators(url, token), (what, self.parse))
            .await
            .map_err(|unpolled| {
                Unpolled::Unreadable(format!("{UNINSTALLED}: {}", unpolled.into_reason()))
            })
    }

    /// Every installation a GitHub App's user `token` reaches, asked
    /// through `api`, each page read: none may hold a permission but read,
    /// be of the write App `writer`, named by its id or client id, or name
    /// another client id than the sign-in's; then
    /// each App they name, read once by its slug, may hold no permission
    /// but read, its account permissions among them.
    ///
    /// # Errors
    ///
    /// Where an installation or an App holds more, an installation is the
    /// write App's or another App's, none names its App, or any of them
    /// could not be read.
    async fn installed(
        &self,
        api: &dyn DynHttp,
        token: &OperatorToken,
        writer: Option<&str>,
    ) -> Result<(), Unpolled> {
        let sign_in = self.sign_in()?;
        let mut slugs = BTreeSet::new();
        let mut ended = false;
        for page in 1..=MOST_INSTALLATION_PAGES {
            let url = format!(
                "{}/user/installations?per_page={PAGE}&page={page}",
                self.api
            );
            let what = "the installations the operator's token reaches";
            let reached: Reached = self.reached(api, token, (url, what)).await?;
            for installation in &reached.installations {
                if let Some((permission, level)) = Self::wider(&installation.permissions) {
                    return Err(Unpolled::Unreadable(format!(
                        "the operator's token reaches an installation holding {permission}: {level}, which can do more than read, so herald signs nobody in with it; give sign_in the client id of a GitHub App whose permissions only read"
                    )));
                }
                let named = [
                    installation.app_id.map(|id| id.to_string()),
                    installation.client_id.clone(),
                ];
                if writer.is_some_and(|writer| named.iter().flatten().any(|named| named == writer))
                {
                    return Err(Unpolled::Unreadable(WRITER_REACHED.to_owned()));
                }
                if installation
                    .client_id
                    .as_ref()
                    .is_some_and(|client_id| *client_id != sign_in.client_id)
                {
                    return Err(Unpolled::Unreadable(FOREIGN_APP.to_owned()));
                }
                match installation.app_slug.as_deref() {
                    Some(slug) if !slug.is_empty() => slugs.insert(slug.to_owned()),
                    _ => return Err(Unpolled::Unreadable(UNNAMED_APP.to_owned())),
                };
            }
            if reached.installations.len() < PAGE {
                ended = true;
                break;
            }
        }
        if !ended {
            return Err(Unpolled::Unreadable(format!(
                "{UNINSTALLED}: they span more than {MOST_INSTALLATION_PAGES} pages"
            )));
        }
        if slugs.is_empty() {
            return Err(Unpolled::Unreadable(UNNAMED_APP.to_owned()));
        }
        for slug in &slugs {
            let url = format!("{}/apps/{}", self.api, whole(slug, APP_SLUG)?);
            let app: ReachedApp = self
                .reached(api, token, (url, "the App the operator's token is of"))
                .await?;
            if let Some((permission, level)) = Self::wider(&app.permissions) {
                return Err(Unpolled::Unreadable(format!(
                    "the GitHub App the operator signed in through holds {permission}: {level}, which can do more than read, its account permissions among them, so herald signs nobody in with it; give the sign-in App no permission but read"
                )));
            }
        }
        Ok(())
    }

    /// The scopes GitHub names for the operator's `token` in the header of
    /// `user`, its answer naming them, once what the token is was found by
    /// its prefix: an OAuth application's token must carry the header; a
    /// GitHub App's user token must reach, through `api`, installations and
    /// an App that only read, read whole, none of the write App `writer`;
    /// any other token is refused, naming no part of it.
    ///
    /// # Errors
    ///
    /// Where the token is of neither kind, an OAuth token's scopes are not
    /// named, or an App's user token's installations or App hold more or
    /// could not be read whole.
    async fn github_scopes(
        &self,
        api: &dyn DynHttp,
        (token, user): (&OperatorToken, &Response),
        writer: Option<&str>,
    ) -> Result<Vec<String>, Unpolled> {
        let named = user.header(GITHUB_SCOPES_HEADER);
        match Self::github_token(token) {
            None => return Err(Unpolled::Unreadable(UNKNOWN_TOKEN.to_owned())),
            Some(GithubToken::OAuth) if named.is_none() => {
                return Err(Unpolled::Unreadable(UNSCOPED.to_owned()));
            }
            Some(GithubToken::OAuth) => {}
            Some(GithubToken::AppUser) => self.installed(api, token, writer).await?,
        }
        Ok(named
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|scope| !scope.is_empty())
            .map(ToOwned::to_owned)
            .collect())
    }

    /// The answer naming the operator holding `token`, asked through
    /// `api`, once the scopes the forge says it holds were found to read
    /// alone: GitHub names an OAuth token's in a header of that answer and
    /// a GitHub App's user token is held to its installations and App
    /// ([`Self::github_scopes`]); GitLab names its token's on its token's
    /// page, asked through `web`.
    ///
    /// # Errors
    ///
    /// Where a scope, an installation or an App can do more than read, the
    /// token is the write App's or of no kind herald knows, or the forge
    /// could not be asked.
    async fn scoped(
        &self,
        (api, web): (&dyn DynHttp, &dyn DynHttp),
        token: &OperatorToken,
        writer: Option<&str>,
    ) -> Result<Response, Unpolled> {
        let user = Erased(api)
            .send(self.operators(format!("{}/user", self.api), token))
            .await
            .map_err(Unpolled::unreachable)?;
        herald_wire::answered("the signed-in operator", &user).map_err(Unpolled::unreadable)?;
        let held: Vec<String> = if self.forge == Forge::GitLab {
            let url = format!("{}{GITLAB_TOKEN_INFO}", self.sign_in()?.web);
            let info: TokenInfo = succeeded(
                web,
                self.operators(url, token),
                ("the scopes of the operator's token", self.parse),
            )
            .await?;
            info.scope
        } else {
            self.github_scopes(api, (token, &user), writer).await?
        };
        match unread(self.forge, held.iter().map(String::as_str)) {
            Some(scope) => Err(Unpolled::Unreadable(format!(
                "the operator's token holds the scope \"{scope}\", which can do more than read, so herald signs nobody in with it; give sign_in the client id of an application that asks for {} alone",
                read_only(self.forge).join(", ")
            ))),
            None => Ok(user),
        }
    }

    /// A request for `url` with `token`, as the forge is asked.
    fn request(&self, method: Method, url: String, token: Option<&Token>) -> Request {
        authorized(
            self.forge,
            Request::new(method, SecretText::plain(url)).at_most(self.parse),
            token,
        )
    }

    /// A request for `url` with the operator's `token`.
    fn operators(&self, url: String, token: &OperatorToken) -> Request {
        self.request(Method::Get, url, Some(&Token::Named(token.sent())))
    }

    /// The device flow's two endpoints on this forge.
    const fn endpoints(&self) -> (&'static str, &'static str) {
        match self.forge {
            Forge::GitLab => ("/oauth/authorize_device", "/oauth/token"),
            Forge::GitHub | Forge::Forgejo => ("/login/device/code", "/login/oauth/access_token"),
        }
    }

    /// A form's POST to the sign-in's web root at `path`.
    fn posted(sign_in: &SignIn, path: &str, body: String, sealed: bool) -> Request {
        let request = Request::new(
            Method::Post,
            SecretText::plain(format!("{}{path}", sign_in.web)),
        )
        .header("Content-Type", SecretText::plain(FORM))
        .accepting(JSON)
        .identified();
        if sealed {
            request.sealed_body(body)
        } else {
            request.body(body)
        }
    }
}

impl Rerunner for ForgeRerunner {
    fn credential(&self) -> &str {
        &self.credential
    }

    fn signing(&self) -> Result<&SignIn, &'static str> {
        self.signing.as_ref().map_err(|reason| *reason)
    }

    fn api(&self) -> &str {
        &self.api
    }

    fn device<'a>(&'a self, web: &'a dyn DynHttp) -> Boxed<'a, Result<Device, Unpolled>> {
        Box::pin(async move {
            let sign_in = self.sign_in()?;
            let scope = sign_in.scopes.join(" ");
            let body = form(&[("client_id", &sign_in.client_id), ("scope", &scope)]);
            let request = Self::posted(sign_in, self.endpoints().0, body, false);
            let answer: DeviceAnswer =
                succeeded(web, request, ("a device code", self.parse)).await?;
            if !on_web(&answer.verification_uri, &sign_in.web) {
                return Err(Unpolled::Unreadable(ELSEWHERE.to_owned()));
            }
            Ok(Device {
                user_code: answer.user_code,
                verification: answer.verification_uri,
                device_code: Secret::new("the device code", answer.device_code),
                interval: seconds(answer.interval, INTERVAL),
                expires: seconds(answer.expires_in, EXPIRES),
            })
        })
    }

    fn polled<'a>(
        &'a self,
        web: &'a dyn DynHttp,
        device: &'a Device,
    ) -> Boxed<'a, Result<Polled, Unpolled>> {
        Box::pin(async move {
            let sign_in = self.sign_in()?;
            let body = form(&[
                ("client_id", &sign_in.client_id),
                ("device_code", device.device_code.expose()),
                ("grant_type", DEVICE_GRANT),
            ]);
            let request = Self::posted(sign_in, self.endpoints().1, body, true);
            let mut answer = Erased(web)
                .send(request)
                .await
                .map_err(Unpolled::unreachable)?;
            let read =
                herald_wire::read::<TokenAnswer>("a sign-in's token", self.parse, &answer.body);
            answer.wipe();
            let answer = read.map_err(Unpolled::unreadable)?;
            let token = answer.access_token.map(OperatorToken::new);
            Ok(match (token, answer.error.as_deref()) {
                (Some(token), None) => Polled::Signed(token),
                (_, Some("authorization_pending")) => Polled::Pending,
                (_, Some("slow_down")) => Polled::Slower,
                (_, Some("expired_token")) => Polled::Ended(EXPIRED.to_owned()),
                (_, Some("access_denied")) => Polled::Ended(DENIED.to_owned()),
                _ => Polled::Ended(ENDED.to_owned()),
            })
        })
    }

    fn operator<'a>(
        &'a self,
        (api, web): (&'a dyn DynHttp, &'a dyn DynHttp),
        token: &'a OperatorToken,
        writer: Option<&'a str>,
    ) -> Boxed<'a, Result<String, Unpolled>> {
        Box::pin(async move {
            let answer = self.scoped((api, web), token, writer).await?;
            let user: User = herald_wire::read("the signed-in operator", self.parse, &answer.body)
                .map_err(Unpolled::unreadable)?;
            user.login
                .or(user.username)
                .filter(|login| !login.is_empty())
                .ok_or_else(|| {
                    Unpolled::Unreadable("the forge named no login for the operator".to_owned())
                })
        })
    }

    fn revoked<'a>(
        &'a self,
        web: &'a dyn DynHttp,
        token: &'a OperatorToken,
    ) -> Boxed<'a, Result<bool, Unpolled>> {
        Box::pin(async move {
            if self.forge != Forge::GitLab {
                return Ok(false);
            }
            let sign_in = self.sign_in()?;
            let sent = token.sent();
            let body = form(&[("client_id", &sign_in.client_id), ("token", sent.expose())]);
            let request = Self::posted(sign_in, GITLAB_REVOKE, body, true);
            let answer = Erased(web)
                .send(request)
                .await
                .map_err(Unpolled::unreachable)?;
            herald_wire::answered("the operator's token revoked", &answer)
                .map_err(Unpolled::unreadable)?;
            Ok(true)
        })
    }

    fn granted<'a>(
        &'a self,
        api: &'a dyn DynHttp,
        token: &'a OperatorToken,
        repository: &'a str,
    ) -> Boxed<'a, Result<Granted, Unpolled>> {
        Box::pin(async move {
            let least = self.sign_in()?.least;
            let request = self.operators(self.base(repository)?, token);
            let what = ("the operator's role on the repository", self.parse);
            if self.forge == Forge::GitLab {
                let project: GitlabProject = succeeded(api, request, what).await?;
                let held = [
                    project.permissions.project_access,
                    project.permissions.group_access,
                ]
                .into_iter()
                .flatten()
                .map(|access| access.access_level)
                .max()
                .unwrap_or_default();
                let enough = GITLAB_LEVELS
                    .iter()
                    .any(|(permission, level)| *permission == least && held >= *level);
                return Ok(if enough {
                    Granted::Enough
                } else {
                    Granted::Short(format!("access level {held}"))
                });
            }
            let repository: GithubRepository = succeeded(api, request, what).await?;
            let held = repository.permissions.unwrap_or_default();
            let named = if held.admin {
                herald_core::Permission::Admin
            } else if held.maintain {
                herald_core::Permission::Maintain
            } else if held.push {
                herald_core::Permission::Write
            } else {
                return Ok(Granted::Short("read or none".to_owned()));
            };
            Ok(if named >= least {
                Granted::Enough
            } else {
                Granted::Short(named.name().to_owned())
            })
        })
    }

    fn head<'a>(
        &'a self,
        api: &'a dyn DynHttp,
        (repository, number): (&'a str, u64),
    ) -> Boxed<'a, Result<Option<String>, Unpolled>> {
        Box::pin(async move {
            let base = self.base(repository)?;
            let what = ("a pull request's head", self.parse);
            Ok(if self.forge == Forge::GitLab {
                let url = format!("{base}/merge_requests/{number}");
                let merge: GitlabMerge = succeeded(
                    api,
                    self.request(Method::Get, url, self.read.as_ref()),
                    what,
                )
                .await?;
                merge.sha.filter(|_| merge.state == "opened")
            } else {
                let url = format!("{base}/pulls/{number}");
                let pull: GithubPull = succeeded(
                    api,
                    self.request(Method::Get, url, self.read.as_ref()),
                    what,
                )
                .await?;
                (pull.state == "open").then_some(pull.head.sha)
            })
        })
    }

    fn run<'a>(
        &'a self,
        api: &'a dyn DynHttp,
        (repository, rerun): (&'a str, &'a Rerun),
    ) -> Boxed<'a, Result<RunNow, Unpolled>> {
        Box::pin(async move {
            let base = self.base(repository)?;
            let (run, job) = (Self::numbered(&rerun.run)?, Self::numbered(&rerun.job)?);
            let unrun = || {
                Unpolled::Unreadable(format!(
                    "the job {job} is not of the run {run} the plan named, so nothing is re-run"
                ))
            };
            if self.forge == Forge::GitLab {
                let found: GitlabJob = self
                    .read(api, format!("{base}/jobs/{job}"), "a job")
                    .await?;
                if found.pipeline.id.to_string() != run {
                    return Err(unrun());
                }
                let pipeline = format!("{base}/pipelines/{run}");
                return Ok(RunNow {
                    reruns: self.retried(api, &pipeline, &found.name).await?,
                    head: found.pipeline.sha,
                    failed: found.status == "failed",
                });
            }
            let found: GithubJob = self
                .read(api, format!("{base}/actions/jobs/{job}"), "a job")
                .await?;
            if found.run_id.to_string() != run {
                return Err(unrun());
            }
            let attempts: GithubRun = self
                .read(api, format!("{base}/actions/runs/{run}"), "a run")
                .await?;
            Ok(RunNow {
                head: found.head_sha,
                failed: found.status.as_deref() == Some("completed")
                    && found.conclusion.as_deref().is_some_and(failed),
                reruns: attempts.run_attempt.unwrap_or(1).saturating_sub(1),
            })
        })
    }

    fn rerun<'a>(
        &'a self,
        write: &'a dyn DynHttp,
        credential: Credential,
        (repository, rerun): (&'a str, &'a Rerun),
    ) -> Boxed<'a, Result<Result<Response, HttpError>, Unpolled>> {
        Box::pin(async move {
            let base = self.base(repository)?;
            let job = Self::numbered(&rerun.job)?;
            let url = match self.forge {
                Forge::GitLab => format!("{base}/jobs/{job}/retry"),
                Forge::GitHub | Forge::Forgejo => format!("{base}/actions/jobs/{job}/rerun"),
            };
            let credential = Token::of(credential);
            Ok(Erased(write)
                .send(self.request(Method::Post, url, Some(&credential)))
                .await)
        })
    }
}

#[cfg(test)]
mod tests;
