use std::collections::BTreeMap;
use std::fmt;

use herald_config::{Config, Destination, RouteConfig, RouteOptions};
use herald_core::{DestinationName, RouteId, hex};
use herald_dest_discord::{Credential, Discord};
use herald_ports::{Algorithm, CryptoError, Secret, SecretError, Secrets, Signer};

use crate::reached::Reached;

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

/// A config's routes, each with its destination's credential resolved.
#[derive(Debug, Clone)]
pub struct Prepared {
    routes: Vec<PreparedRoute>,
}

/// The prepared routes, each with the `Http` its destination may reach.
#[derive(Debug)]
pub struct Connected<H> {
    pub(crate) routes: Vec<(PreparedRoute, H)>,
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

    /// The routes, each with an `Http` made by `http` for its destination's
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
        let mut routes = Vec::new();
        for route in self.routes {
            let base = route.reached.api().to_owned();
            let reaching = if let Some(reaching) = made.get(&base) {
                reaching.clone()
            } else {
                let reaching = http(&base)?;
                made.insert(base, reaching.clone());
                reaching
            };
            routes.push((route, reaching));
        }
        Ok(Connected { routes })
    }
}

struct Resolved {
    credential: Credential,
    api: Option<String>,
    marker_key: Secret,
    derived: bool,
}

/// `config`'s routes with every credential and marker key resolved through
/// `secrets` and `signer`.
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
    Ok(Prepared { routes })
}
