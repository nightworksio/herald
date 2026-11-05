use std::collections::BTreeMap;
use std::fmt;

use herald_config::{Config, Destination, RouteConfig, RouteOptions, Source};
use herald_core::{DestinationName, RouteId, SecretRef, SourceName, hex};
use herald_dest_discord::{Credential, Discord};
use herald_dest_fediverse::{ActivityPub, Mastodon};
use herald_dest_matrix::Matrix;
use herald_dest_slack::Slack;
use herald_ports::{Algorithm, CryptoError, Http, Secret, SecretError, Secrets, Signer};
use herald_sources_feed::Feed;
use herald_sources_forge::{Origin, Token};
use herald_sources_registry::Package;
use herald_store_markers::{MarkedRoute, MarkersStore};

use crate::interacting::Interacting;
use crate::reached::{Posts, Reached};

/// What a marker key is derived from a credential under (ADR-0011).
const DERIVATION: &[u8] = b"herald/marker-key/v1";

/// Why a config's credentials could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    /// A secret the config names could not be read.
    Secret(SecretError),
    /// A marker key could not be derived.
    Crypto(CryptoError),
    /// A destination names neither a token nor a webhook.
    NoCredential {
        /// The destination.
        destination: String,
    },
    /// A route names a destination the config does not have, or one of
    /// another kind than its options.
    NoDestination {
        /// The route.
        route: String,
    },
    /// An ActivityPub destination's key cannot sign as its actor.
    Unkeyed {
        /// The destination.
        destination: String,
        /// The secret its key is read from.
        secret: String,
    },
}

impl fmt::Display for PrepareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Secret(error) => error.fmt(f),
            Self::Crypto(error) => error.fmt(f),
            Self::NoCredential { destination } => {
                write!(f, "the destination \"{destination}\" names no credential")
            }
            Self::NoDestination { route } => {
                write!(
                    f,
                    "the route \"{route}\" names a destination the config does not have, or one of another kind"
                )
            }
            Self::Unkeyed {
                destination,
                secret,
            } => write!(
                f,
                "the destination \"{destination}\" signs with {secret}, which is not an RSA private key of 2048 to 8192 bits in a PKCS#8 PEM; make one with `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048`"
            ),
        }
    }
}

impl From<SecretError> for PrepareError {
    fn from(error: SecretError) -> Self {
        Self::Secret(error)
    }
}

impl From<CryptoError> for PrepareError {
    fn from(error: CryptoError) -> Self {
        Self::Crypto(error)
    }
}

/// A route ready to announce through: its destination reached, and the key
/// its markers are computed under.
#[derive(Debug, Clone)]
pub(crate) struct PreparedRoute {
    pub(crate) config: RouteConfig,
    pub(crate) reached: Reached,
    pub(crate) marker_key: Secret,
    pub(crate) derived: bool,
}

/// What a source is asked, with its token resolved.
#[derive(Debug, Clone)]
pub(crate) enum Asked {
    /// A repository's releases on a forge.
    Forge {
        origin: Origin,
        token: Option<Token>,
    },
    /// A package's versions on a registry.
    Registry {
        package: Package,
        token: Option<Secret>,
    },
    /// A feed's entries.
    Feed(Feed),
}

impl Asked {
    fn api(&self) -> &str {
        match self {
            Self::Forge { origin, .. } => origin.api(),
            Self::Registry { package, .. } => package.api(),
            Self::Feed(feed) => feed.api(),
        }
    }
}

/// A source ready to poll.
#[derive(Debug, Clone)]
pub(crate) struct PreparedSource {
    pub(crate) name: SourceName,
    pub(crate) asked: Asked,
    pub(crate) backfill: usize,
}

/// A config's routes, each with its destination's credential resolved, and
/// its sources, each with its token.
#[derive(Debug, Clone)]
pub struct Prepared {
    routes: Vec<PreparedRoute>,
    sources: Vec<PreparedSource>,
    destinations: Vec<(DestinationName, Destination, Secret)>,
    interacting: Vec<(DestinationName, Interacting)>,
}

/// The prepared routes and sources, each with the `Http` that reaches it.
#[derive(Debug)]
pub struct Connected<H> {
    pub(crate) routes: Vec<(PreparedRoute, H)>,
    pub(crate) sources: Vec<(PreparedSource, H)>,
    pub(crate) interacting: Vec<(DestinationName, Interacting, H, Option<H>)>,
}

impl<H> Connected<H> {
    /// The destination named `name`, where it takes commands, with the
    /// `Http` its answers go through.
    #[must_use]
    pub fn interacting(&self, name: &DestinationName) -> Option<(&Interacting, &H)> {
        self.interacting
            .iter()
            .find(|(named, ..)| named == name)
            .map(|(_, interacting, http, _)| (interacting, http))
    }

    /// Every destination that takes commands, by name, with the `Http` its
    /// answers go through and the one its sessions open through, where it
    /// holds any.
    pub fn interactions(
        &self,
    ) -> impl Iterator<Item = (&DestinationName, &Interacting, &H, Option<&H>)> {
        self.interacting
            .iter()
            .map(|(name, interacting, http, sessions)| (name, interacting, http, sessions.as_ref()))
    }
}

impl<H: Http> Connected<H> {
    /// The store the posts of every route that reads them back make, its
    /// markers computed by `signer` (ADR-0005).
    pub fn marked<G: Signer>(&self, signer: G) -> MarkersStore<Posts<'_, H>, G> {
        let routes = self
            .routes
            .iter()
            .filter(|(route, _)| route.reached.profile().features.reads_back)
            .map(|(route, http)| MarkedRoute {
                route: route.config.route.id.clone(),
                thread: route.config.route.delivery.thread.clone(),
                markers: route.reached.posts(http),
                key: route.marker_key.clone(),
            })
            .collect();
        MarkersStore::new(routes, signer)
    }
}

impl Prepared {
    /// The routes whose marker key is derived from their destination's
    /// credential, as doctor names them (ADR-0011).
    #[must_use]
    pub fn derived(&self) -> Vec<&RouteId> {
        self.routes
            .iter()
            .filter(|route| route.derived)
            .map(|route| &route.config.route.id)
            .collect()
    }

    /// The routes and sources, each with an `Http` made by `http` for its
    /// base URL, one per base.
    ///
    /// # Errors
    ///
    /// The first error `http` gives.
    pub fn connect<H: Clone, E>(
        self,
        mut http: impl FnMut(&str) -> Result<H, E>,
    ) -> Result<Connected<H>, E> {
        let mut made: BTreeMap<String, H> = BTreeMap::new();
        let mut reach = |base: &str| -> Result<H, E> {
            if let Some(reaching) = made.get(base) {
                return Ok(reaching.clone());
            }
            let reaching = http(base)?;
            made.insert(base.to_owned(), reaching.clone());
            Ok(reaching)
        };
        let mut routes = Vec::new();
        for route in self.routes {
            let reaching = reach(route.reached.api())?;
            routes.push((route, reaching));
        }
        let mut sources = Vec::new();
        for source in self.sources {
            let reaching = reach(source.asked.api())?;
            sources.push((source, reaching));
        }
        let mut interacting = Vec::new();
        for (name, destination) in self.interacting {
            let reaching = reach(destination.api())?;
            let sessions = match destination.sessions() {
                Some(base) => Some(reach(base)?),
                None => None,
            };
            interacting.push((name, destination, reaching, sessions));
        }
        Ok(Connected {
            routes,
            sources,
            interacting,
        })
    }
}

struct Resolved<'a> {
    destination: &'a Destination,
    secret: Secret,
    marker_key: Secret,
    derived: bool,
}

impl Resolved<'_> {
    /// The destination reached for a route with `options`, where they are
    /// options of the destination's kind.
    fn reached(&self, options: &RouteOptions) -> Option<Reached> {
        let secret = self.secret.clone();
        match (self.destination, options) {
            (Destination::Discord(reached), RouteOptions::Discord(placed)) => {
                let credential = if reached.token.is_some() {
                    Credential::Bot(secret)
                } else {
                    Credential::Webhook(secret)
                };
                Some(Reached::Discord(Discord::new(
                    credential,
                    placed.clone(),
                    reached.api.as_deref(),
                )))
            }
            (Destination::Mastodon(reached), RouteOptions::Mastodon(placed)) => Some(
                Reached::Mastodon(Mastodon::new(&reached.server, secret, placed.clone())),
            ),
            (Destination::Matrix(reached), RouteOptions::Matrix(placed)) => Some(Reached::Matrix(
                Matrix::new(&reached.homeserver, secret, placed.clone()),
            )),
            (Destination::ActivityPub(reached), RouteOptions::ActivityPub(placed)) => Some(
                Reached::ActivityPub(ActivityPub::new(&reached.domain, secret, placed.clone())),
            ),
            (Destination::Slack(reached), RouteOptions::Slack(placed)) => {
                let credential = if reached.token.is_some() {
                    herald_dest_slack::Credential::Bot(secret)
                } else {
                    herald_dest_slack::Credential::Webhook(secret)
                };
                Some(Reached::Slack(Slack::new(
                    credential,
                    placed.clone(),
                    reached.api.as_deref(),
                )))
            }
            _ => None,
        }
    }
}

/// Whether `key` signs as the actor of the ActivityPub destination
/// `destination`.
///
/// # Errors
///
/// When it is no key `signer` can sign with.
pub(crate) async fn keyed<G: Signer>(
    signer: &G,
    destination: &DestinationName,
    key: &Secret,
) -> Result<(), PrepareError> {
    signer
        .public(Algorithm::RsaSha256, key)
        .await
        .map(drop)
        .map_err(|_| PrepareError::Unkeyed {
            destination: destination.as_str().to_owned(),
            secret: key.name().to_owned(),
        })
}

/// Each destination of `config` with its credential and marker key,
/// resolved through `secrets` and `signer`.
async fn resolved<'c, S: Secrets, G: Signer>(
    config: &'c Config,
    secrets: &S,
    signer: &G,
) -> Result<BTreeMap<&'c DestinationName, Resolved<'c>>, PrepareError> {
    let installation = match &config.marker_key {
        Some(key) => Some(secrets.secret(&key.secret).await?),
        None => None,
    };
    let mut resolved: BTreeMap<&DestinationName, Resolved> = BTreeMap::new();
    for (name, destination) in &config.destinations {
        let named = match destination {
            Destination::Discord(options) => options.token.as_ref().or(options.webhook.as_ref()),
            Destination::Mastodon(options) => Some(&options.token),
            Destination::Matrix(options) => Some(&options.token),
            Destination::Slack(options) => options.token.as_ref().or(options.webhook.as_ref()),
            Destination::ActivityPub(options) => Some(&options.key),
        };
        let Some(reference) = named else {
            return Err(PrepareError::NoCredential {
                destination: name.as_str().to_owned(),
            });
        };
        let secret = secrets.secret(&reference.secret).await?;
        if matches!(destination, Destination::ActivityPub(_)) {
            keyed(signer, name, &secret).await?;
        }
        let (marker_key, derived) = if let Some(key) = &installation {
            (key.clone(), false)
        } else {
            let mac = signer
                .sign(Algorithm::HmacSha256, &secret, DERIVATION)
                .await?;
            let name = format!("the marker key of {}", name.as_str());
            (Secret::new(name, hex(&mac)), true)
        };
        resolved.insert(
            name,
            Resolved {
                destination,
                secret,
                marker_key,
                derived,
            },
        );
    }
    Ok(resolved)
}

/// `config`'s routes and sources with every credential, token and marker
/// key resolved through `secrets` and `signer`.
///
/// # Errors
///
/// The first secret that cannot be read, or marker key that cannot be
/// derived.
pub async fn prepare<S: Secrets, G: Signer>(
    config: &Config,
    secrets: &S,
    signer: &G,
) -> Result<Prepared, PrepareError> {
    let resolved = resolved(config, secrets, signer).await?;
    let mut routes = Vec::new();
    for route in &config.routes {
        let found = resolved.get(&route.route.destination);
        let Some((destination, reached)) =
            found.and_then(|destination| Some((destination, destination.reached(&route.options)?)))
        else {
            return Err(PrepareError::NoDestination {
                route: route.route.id.as_str().to_owned(),
            });
        };
        routes.push(PreparedRoute {
            config: route.clone(),
            reached,
            marker_key: destination.marker_key.clone(),
            derived: destination.derived,
        });
    }
    let mut sources = Vec::new();
    for (name, source) in &config.sources {
        let named = match source {
            Source::Forge(_, options) => options.token.as_ref(),
            Source::Registry(_, options) => options.token.as_ref(),
            Source::Feed(_) => None,
        };
        let token = match named {
            Some(named) => Some(secrets.secret(&named.secret).await?),
            None => None,
        };
        let (asked, backfill) = match source {
            Source::Forge(forge, options) => (
                Asked::Forge {
                    origin: Origin::new(*forge, options),
                    token: token.map(Token::Named),
                },
                options.backfill,
            ),
            Source::Registry(registry, options) => (
                Asked::Registry {
                    package: Package::new(*registry, options),
                    token,
                },
                options.backfill,
            ),
            Source::Feed(options) => (Asked::Feed(Feed::new(options)), options.backfill),
        };
        sources.push(PreparedSource {
            name: name.clone(),
            asked,
            backfill,
        });
    }
    Ok(Prepared {
        routes,
        sources,
        destinations: resolved
            .into_iter()
            .map(|(name, resolved)| (name.clone(), resolved.destination.clone(), resolved.secret))
            .collect(),
        interacting: Vec::new(),
    })
}

impl Prepared {
    /// These routes and sources, with every destination that takes
    /// commands, each with the secrets it answers with read through
    /// `secrets`: what a run that answers commands needs, and no other run
    /// reads.
    ///
    /// # Errors
    ///
    /// The first secret that cannot be read.
    pub async fn answering<S: Secrets>(mut self, secrets: &S) -> Result<Self, PrepareError> {
        let mut interacting = Vec::new();
        for (name, destination, credential) in &self.destinations {
            if let Some(found) = interacts(destination, credential, secrets).await? {
                interacting.push((name.clone(), found));
            }
        }
        self.interacting = interacting;
        Ok(self)
    }
}

async fn optional<S: Secrets>(
    secrets: &S,
    named: Option<&SecretRef>,
) -> Result<Option<Secret>, PrepareError> {
    match named {
        Some(named) => Ok(Some(secrets.secret(&named.secret).await?)),
        None => Ok(None),
    }
}

/// How `destination`, reached with `credential`, takes commands, where it
/// takes any, with its secrets read through `secrets`.
async fn interacts<S: Secrets>(
    destination: &Destination,
    credential: &Secret,
    secrets: &S,
) -> Result<Option<Interacting>, PrepareError> {
    Ok(match destination {
        Destination::Discord(options) => options.interactions.as_ref().map(|interactions| {
            let bot = options.token.is_some().then(|| credential.clone());
            Interacting::Discord(herald_dest_discord::Interacting::new(
                interactions.clone(),
                bot,
                options.api.as_deref(),
            ))
        }),
        Destination::Slack(options) => match &options.interactions {
            Some(interactions) => Some(Interacting::Slack {
                interacting: herald_dest_slack::Interacting::new(
                    interactions.clone(),
                    options.api.as_deref(),
                ),
                signing: optional(secrets, interactions.signing.as_ref()).await?,
                app: optional(secrets, interactions.app.as_ref()).await?,
            }),
            None => None,
        },
        Destination::Matrix(options) => match &options.interactions {
            Some(interactions) => {
                let acting = match interactions.via {
                    herald_dest_matrix::Via::Appservice => {
                        optional(secrets, interactions.appservice_token.as_ref()).await?
                    }
                    herald_dest_matrix::Via::Sync => None,
                };
                Some(Interacting::Matrix {
                    interacting: herald_dest_matrix::Interacting::new(
                        &options.homeserver,
                        interactions.clone(),
                        acting.unwrap_or_else(|| credential.clone()),
                    ),
                    homeserver_token: optional(secrets, interactions.homeserver_token.as_ref())
                        .await?,
                })
            }
            None => None,
        },
        Destination::Mastodon(_) | Destination::ActivityPub(_) => None,
    })
}
