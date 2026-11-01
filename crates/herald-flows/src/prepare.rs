use std::collections::BTreeMap;
use std::fmt;

use herald_config::{Config, Destination, RouteConfig, RouteOptions, Source};
use herald_core::{DestinationName, RouteId, SourceName, hex};
use herald_dest_discord::{Credential, Discord};
use herald_ports::{Algorithm, CryptoError, Http, Secret, SecretError, Secrets, Signer};
use herald_sources_feed::Feed;
use herald_sources_forge::{Origin, Token};
use herald_sources_registry::Package;
use herald_store_markers::{MarkedRoute, MarkersStore};

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
    /// A route names a destination the config does not have.
    NoDestination {
        /// The route.
        route: String,
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
                    "the route \"{route}\" names a destination the config does not have"
                )
            }
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
}

/// The prepared routes and sources, each with the `Http` that reaches it.
#[derive(Debug)]
pub struct Connected<H> {
    pub(crate) routes: Vec<(PreparedRoute, H)>,
    pub(crate) sources: Vec<(PreparedSource, H)>,
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
        Ok(Connected { routes, sources })
    }
}

struct Resolved {
    credential: Credential,
    api: Option<String>,
    marker_key: Secret,
    derived: bool,
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
    let installation = match &config.marker_key {
        Some(key) => Some(secrets.secret(&key.secret).await?),
        None => None,
    };
    let mut resolved: BTreeMap<&DestinationName, Resolved> = BTreeMap::new();
    for (name, destination) in &config.destinations {
        let Destination::Discord(options) = destination;
        let Some(reference) = options.token.as_ref().or(options.webhook.as_ref()) else {
            return Err(PrepareError::NoCredential {
                destination: name.as_str().to_owned(),
            });
        };
        let secret = secrets.secret(&reference.secret).await?;
        let (marker_key, derived) = if let Some(key) = &installation {
            (key.clone(), false)
        } else {
            let mac = signer
                .sign(Algorithm::HmacSha256, &secret, DERIVATION)
                .await?;
            let name = format!("the marker key of {}", name.as_str());
            (Secret::new(name, hex(&mac)), true)
        };
        let credential = if options.token.is_some() {
            Credential::Bot(secret)
        } else {
            Credential::Webhook(secret)
        };
        resolved.insert(
            name,
            Resolved {
                credential,
                api: options.api.clone(),
                marker_key,
                derived,
            },
        );
    }
    let mut routes = Vec::new();
    for route in &config.routes {
        let RouteOptions::Discord(options) = &route.options;
        let Some(destination) = resolved.get(&route.route.destination) else {
            return Err(PrepareError::NoDestination {
                route: route.route.id.as_str().to_owned(),
            });
        };
        routes.push(PreparedRoute {
            config: route.clone(),
            reached: Reached::Discord(Discord::new(
                destination.credential.clone(),
                options.clone(),
                destination.api.as_deref(),
            )),
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
    Ok(Prepared { routes, sources })
}
