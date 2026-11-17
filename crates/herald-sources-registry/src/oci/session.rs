use herald_core::{Address, Glob, Visibility, status, unreserved, web_url};
use herald_kinds::{Probed, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _, Method, Request, Response, Secret, SecretText};
use herald_wire::{Reading, SourceError};

use super::options::OciOptions;
use super::wire::{self, Challenge};

/// A registry as a source reaches it: where it asks, who may hand out its
/// tokens and serve its blobs, and how it reads.
#[derive(Debug)]
pub(crate) struct Reached {
    /// The registry's base URL.
    pub(crate) registry: String,
    /// The registry's origin.
    origin: String,
    /// The one other origin whose token service it trusts, where there is
    /// one.
    auth: Option<String>,
    /// The origins a config blob may be redirected to.
    blobs: Vec<String>,
    /// Every base URL it asks, the registry's first, where it asks more
    /// than the registry.
    pub(crate) bases: Vec<String>,
    /// The registry's name in each tag's identity.
    pub(crate) name: String,
    /// Whether the registry is Docker Hub's.
    pub(crate) docker_hub: bool,
    /// The account and token it is asked with, where it is given one.
    credentials: Option<(String, Secret)>,
    /// How a poll reads.
    pub(crate) reading: Reading,
    /// How many entries one page of a listing asks for.
    pub(crate) listed_page: usize,
    /// The most tags a repository's listing reads.
    pub(crate) most_tags: usize,
    /// The tags that move.
    pub(crate) moving: Vec<Glob>,
    /// Who may see its repositories.
    pub(crate) visibility: Visibility,
    /// How many of the newest tags a first poll announces.
    pub(crate) backfill: usize,
}

/// The origin of `url`, where it is a web address naming no user.
pub(crate) fn origin_of(url: &str) -> Option<String> {
    Address::parse(url)
        .filter(|address| web_url(url) && !address.names_a_user())
        .map(|address| address.origin())
}

impl Reached {
    /// The registry `options` name, asked with `token`.
    pub(crate) fn new(options: &OciOptions, token: Option<Secret>) -> Self {
        let registry = options.registry();
        let auth = options.auth();
        let blobs = options.blobs();
        let mut bases = vec![registry.clone()];
        bases.extend(auth.iter().cloned());
        bases.extend(
            blobs
                .iter()
                .map(|blob| blob.trim_end_matches('/').to_owned()),
        );
        Self {
            origin: origin_of(&registry).unwrap_or_default(),
            auth: auth.as_deref().and_then(origin_of),
            blobs: blobs.iter().filter_map(|blob| origin_of(blob)).collect(),
            bases: if bases.len() > 1 { bases } else { Vec::new() },
            registry,
            name: options.registry_name(),
            docker_hub: options.docker_hub(),
            credentials: options.username.clone().zip(token),
            reading: options.reading(),
            listed_page: options.listed_page(),
            most_tags: options.most_tags().get(),
            moving: options.moving().unwrap_or_default(),
            visibility: options.visibility(),
            backfill: options.backfill,
        }
    }
}

/// Why asking the registry failed: the key of the source's table to look
/// at, why, and whether the registry could not be reached at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    pub(crate) key: &'static str,
    pub(crate) why: String,
    pub(crate) unreachable: bool,
}

impl Failure {
    /// A failure at `key`, for `why`.
    pub(crate) fn at(key: &'static str, why: impl Into<String>) -> Self {
        Self {
            key,
            why: why.into(),
            unreachable: false,
        }
    }

    /// A failure at `key`, as `error` says.
    pub(crate) fn read(key: &'static str, error: &SourceError) -> Self {
        Self::at(key, error.to_string())
    }
}

impl From<Failure> for Unpolled {
    fn from(failure: Failure) -> Self {
        if failure.unreachable {
            Self::Unreachable(failure.why)
        } else {
            Self::Unreadable(failure.why)
        }
    }
}

impl From<Failure> for Probed {
    fn from(failure: Failure) -> Self {
        Self {
            key: failure.key,
            why: failure.why,
        }
    }
}

/// A config blob's answer, or the origin it was redirected to that the
/// source may not reach.
#[derive(Debug)]
pub(crate) enum Blob {
    /// The blob's answer.
    Read(Response),
    /// The origin, or a placeholder where the location names none, as the
    /// location itself may carry a signed credential.
    Redirected(String),
}

/// What a blob's redirected address, which may sign its own access, is
/// shown as.
const LOCATION: &str = "the blob's redirected address";

/// What a redirect to an address that is no web URL naming no user is
/// shown as, as its location may carry a signed credential.
const UNPARSED: &str = "an address that is not a web URL";

/// The statuses that redirect a blob.
const REDIRECTS: [u16; 5] = [301, 302, 303, 307, 308];

/// One poll's requests to a registry, answering its challenge once and
/// asking on with what it was given.
pub(crate) struct Session<'a> {
    reached: &'a Reached,
    http: &'a dyn DynHttp,
    scope: String,
    authorization: Option<SecretText>,
}

impl<'a> Session<'a> {
    /// Requests to `reached` through `http`, for a token of `scope` where
    /// the challenge names none.
    pub(crate) fn new(reached: &'a Reached, http: &'a dyn DynHttp, scope: String) -> Self {
        Self {
            reached,
            http,
            scope,
            authorization: None,
        }
    }

    /// A request for `path` under the registry.
    pub(crate) fn get(&self, path: &str) -> Request {
        Request::new(
            Method::Get,
            SecretText::plain(format!("{}{path}", self.reached.registry)),
        )
        .at_most(self.reached.reading.parse)
        .identified()
    }

    /// The registry's answer to `request`, asked again with a credential
    /// where it challenges for one.
    pub(crate) async fn send(&mut self, request: Request) -> Result<Response, Failure> {
        let answer = self.sent(request.clone()).await?;
        let challenge = (answer.status == status::UNAUTHORIZED)
            .then(|| answer.header("www-authenticate").and_then(wire::challenge))
            .flatten();
        let Some(challenge) = challenge else {
            return Ok(answer);
        };
        self.authorization = Some(self.authorized(challenge).await?);
        let answer = self.sent(request).await?;
        if answer.refuses_credential() {
            return Err(Failure::at(
                "token",
                format!(
                    "the registry answered {} with the credential its challenge asked for; give a username and token that may read {}",
                    answer.status, self.scope
                ),
            ));
        }
        Ok(answer)
    }

    async fn sent(&self, request: Request) -> Result<Response, Failure> {
        let request = match &self.authorization {
            Some(authorization) => request.replacing("Authorization", authorization.clone()),
            None => request,
        };
        Erased(self.http)
            .send(request)
            .await
            .map_err(|error| Failure {
                key: "registry",
                why: error.to_string(),
                unreachable: true,
            })
    }

    /// The credential `challenge` asks for.
    async fn authorized(&self, challenge: Challenge) -> Result<SecretText, Failure> {
        let Challenge::Bearer {
            realm,
            service,
            scope,
        } = challenge
        else {
            return self.basic().ok_or_else(|| {
                Failure::at(
                    "token",
                    "the registry asks for a username and password; give username and token",
                )
            });
        };
        let origin = origin_of(&realm);
        let trusted = origin.as_ref().is_some_and(|origin| {
            *origin == self.reached.origin || Some(origin) == self.reached.auth.as_ref()
        });
        let Some(origin) = origin.clone().filter(|_| trusted) else {
            let shown = origin.unwrap_or_else(|| UNPARSED.to_owned());
            return Err(Failure::at(
                "auth",
                format!(
                    "the registry sends its tokens from {shown}, which is neither its own origin nor auth's; set auth to that service's base URL where it is the registry's own"
                ),
            ));
        };
        let mut query = Vec::new();
        query.extend(service.map(|service| format!("service={}", unreserved(&service))));
        query.push(format!(
            "scope={}",
            unreserved(scope.as_deref().unwrap_or(&self.scope))
        ));
        let separator = if realm.contains('?') { '&' } else { '?' };
        let mut request = Request::new(
            Method::Get,
            SecretText::plain(format!("{realm}{separator}{}", query.join("&"))),
        )
        .at_most(wire::TOKEN_ANSWER)
        .identified();
        if let Some(basic) = self.basic() {
            request = request.header("Authorization", basic);
        }
        let answer = Erased(self.http)
            .send(request)
            .await
            .map_err(|error| Failure {
                key: "auth",
                why: error.to_string(),
                unreachable: true,
            })?;
        if !answer.succeeded() {
            let key = if answer.refuses_credential() {
                "token"
            } else {
                "auth"
            };
            return Err(Failure::at(
                key,
                format!(
                    "the token service at {origin} answered {}; check username and token",
                    answer.status
                ),
            ));
        }
        let token = wire::issued(&answer.body).map_err(|error| Failure::read("auth", &error))?;
        let name = self.reached.credentials.as_ref().map_or_else(
            || wire::TOKEN.to_owned(),
            |(_, secret)| secret.name().to_owned(),
        );
        Ok(SecretText::plain("Bearer ").then_secret(Secret::new(name, token)))
    }

    /// The credentials as HTTP Basic, where the source has them.
    fn basic(&self) -> Option<SecretText> {
        self.reached.credentials.as_ref().map(|(username, token)| {
            SecretText::plain("Basic ").then_secret(Secret::new(
                token.name(),
                herald_core::basic(username, token.expose()),
            ))
        })
    }

    /// The registry's answer to `request` for a blob, followed once where
    /// it redirects to an origin the source may reach, without the
    /// registry's credential.
    pub(crate) async fn blob(&mut self, request: Request) -> Result<Blob, Failure> {
        let answer = self.send(request).await?;
        if !REDIRECTS.contains(&answer.status) {
            return Ok(Blob::Read(answer));
        }
        let location = answer.header("location").unwrap_or_default();
        let target = if location.starts_with('/') && !location.starts_with("//") {
            format!("{}{location}", self.reached.origin)
        } else {
            location.to_owned()
        };
        let request = Request::new(
            Method::Get,
            SecretText::secret(Secret::new(LOCATION, target.clone())),
        )
        .at_most(self.reached.reading.parse)
        .identified();
        match origin_of(&target) {
            Some(origin) if origin == self.reached.origin => {
                self.send(request).await.map(Blob::Read)
            }
            Some(origin) if self.reached.blobs.contains(&origin) => Erased(self.http)
                .send(request)
                .await
                .map(Blob::Read)
                .map_err(|error| Failure {
                    key: "blobs",
                    why: error.to_string(),
                    unreachable: true,
                }),
            origin => Ok(Blob::Redirected(
                origin.unwrap_or_else(|| UNPARSED.to_owned()),
            )),
        }
    }
}
