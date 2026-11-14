//! A config's credentials and marker keys resolved, and each route and
//! source given the `Http` that reaches it.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use herald_config::{Config, Destination, Placed, RouteConfig, Uses};
use herald_core::{DestinationName, RouteId, SourceName, hex};
use herald_kinds::{Interacted, Poll, Posts, Reached};
use herald_ports::{Algorithm, Base, CryptoError, Http, Secret, SecretError, Secrets, Signer};
use herald_store_markers::{MarkedRoute, MarkersStore};

use crate::serving::Webhooks;

/// What a marker key is derived from a credential under (ADR-0011).
const DERIVATION: &[u8] = b"herald/marker-key/v1";

/// Why a config's credentials could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    /// Every secret the run needs that could not be read, each with the
    /// keys that name it.
    Unread(Vec<UnreadSecret>),
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
            Self::Unread(unread) => {
                for (index, one) in unread.iter().enumerate() {
                    if index > 0 {
                        f.write_str("\n")?;
                    }
                    one.fmt(f)?;
                }
                Ok(())
            }
            Self::Crypto(error) => error.fmt(f),
            Self::NoCredential { destination } => {
                write!(
                    f,
                    "the destination \"{destination}\" names no credential; give it the token or webhook it reaches its platform with, as [destinations.{destination}] token = {{ secret = \"NAME\" }}"
                )
            }
            Self::NoDestination { route } => {
                write!(
                    f,
                    "the route \"{route}\" names a destination the config does not have, or one of another kind; run herald doctor and fix what it names"
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
        Self::Unread(vec![UnreadSecret {
            error,
            keys: Vec::new(),
        }])
    }
}

/// A secret a run needs that could not be read, and the keys that name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadSecret {
    /// Why it could not be read.
    pub error: SecretError,
    /// The keys that name it, such as `destinations.discord.token`.
    pub keys: Vec<String>,
}

impl fmt::Display for UnreadSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.keys.is_empty() {
            write!(f, "{}: ", self.keys.join(", "))?;
        }
        self.error.fmt(f)
    }
}

/// The secrets a run needs, read before anything else: each it asks for
/// again is taken from here.
struct ReadSecrets(BTreeMap<String, Secret>);

impl Secrets for ReadSecrets {
    fn secret(
        &self,
        name: &str,
    ) -> impl std::future::Future<Output = Result<Secret, SecretError>> + Send {
        std::future::ready(
            self.0
                .get(name)
                .cloned()
                .ok_or_else(|| SecretError::Missing {
                    name: name.to_owned(),
                }),
        )
    }
}

/// Every secret a run that `uses` `config` needs, read through `secrets`
/// (ADR-0006 decision 7).
///
/// # Errors
///
/// Every one that cannot be read, each with the keys that name it.
async fn read<S: Secrets>(
    config: &Config,
    secrets: &S,
    uses: Uses,
) -> Result<ReadSecrets, PrepareError> {
    let mut read = BTreeMap::new();
    let mut unread = Vec::new();
    for name in config.secrets_used(uses) {
        match secrets.secret(name).await {
            Ok(secret) => {
                read.insert(name.to_owned(), secret);
            }
            Err(error) => unread.push(UnreadSecret {
                error,
                keys: config.keys_naming(name).to_vec(),
            }),
        }
    }
    if unread.is_empty() {
        Ok(ReadSecrets(read))
    } else {
        Err(PrepareError::Unread(unread))
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
pub struct PreparedRoute {
    pub(crate) config: RouteConfig,
    pub(crate) reached: Reached,
    pub(crate) marker_key: Secret,
    pub(crate) derived: bool,
}

/// A source ready to poll.
#[derive(Debug, Clone)]
pub struct PreparedSource {
    pub(crate) name: SourceName,
    pub(crate) poll: Arc<dyn Poll>,
    pub(crate) backfill: usize,
}

/// A config's routes, each with its destination's credential resolved, and
/// its sources, each with its token.
#[derive(Debug, Clone)]
pub struct Prepared {
    routes: Vec<PreparedRoute>,
    sources: Vec<PreparedSource>,
    interacting: Vec<(DestinationName, Interacted)>,
    webhooks: Webhooks,
}

/// The prepared routes and sources, each with the `Http` that reaches it.
#[derive(Debug)]
pub struct Connected<H> {
    pub(crate) routes: Vec<(PreparedRoute, H)>,
    pub(crate) sources: Vec<(PreparedSource, H)>,
    pub(crate) interacting: Vec<(DestinationName, Interacted, H, Option<H>)>,
    webhooks: Webhooks,
}

impl<H> Connected<H> {
    /// The secret each source's webhooks are signed with, where the run
    /// serves them.
    #[must_use]
    pub fn webhooks(&self) -> &Webhooks {
        &self.webhooks
    }

    /// The destination named `name`, where it takes commands, with the
    /// `Http` its answers go through.
    #[must_use]
    pub fn interacting(&self, name: &DestinationName) -> Option<(&Interacted, &H)> {
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
    ) -> impl Iterator<Item = (&DestinationName, &Interacted, &H, Option<&H>)> {
        self.interacting
            .iter()
            .map(|(name, interacting, http, sessions)| (name, interacting, http, sessions.as_ref()))
    }
}

impl<H: Http> Connected<H> {
    /// The store the posts of every route that reads them back make, its
    /// markers computed by `signer` (ADR-0005).
    pub fn marked<G: Signer>(&self, signer: G) -> MarkersStore<Posts<'_>, G> {
        let routes = self
            .routes
            .iter()
            .filter(|(route, _)| route.reached.profile().features.reads_back)
            .map(|(route, http)| MarkedRoute {
                route: route.config.route.id.clone(),
                thread: route.config.route.delivery.thread.clone(),
                markers: route.reached.read(http),
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

    /// The routes and sources, each with an `Http` made by `http` for what
    /// it reaches, one per base and one for every public address.
    ///
    /// # Errors
    ///
    /// The first error `http` gives.
    pub fn connect<H: Clone, E>(
        self,
        mut http: impl FnMut(Base<'_>) -> Result<H, E>,
    ) -> Result<Connected<H>, E> {
        let mut made: BTreeMap<String, H> = BTreeMap::new();
        let mut public: Option<H> = None;
        let mut reach = |base: Base<'_>| -> Result<H, E> {
            let known = match base {
                Base::At(url) => made.get(url),
                Base::Public => public.as_ref(),
            };
            if let Some(reaching) = known {
                return Ok(reaching.clone());
            }
            let reaching = http(base)?;
            match base {
                Base::At(url) => made.insert(url.to_owned(), reaching.clone()),
                Base::Public => public.replace(reaching.clone()),
            };
            Ok(reaching)
        };
        let mut routes = Vec::new();
        for route in self.routes {
            let reaching = reach(route.reached.base())?;
            routes.push((route, reaching));
        }
        let mut sources = Vec::new();
        for source in self.sources {
            let reaching = reach(Base::At(source.poll.api()))?;
            sources.push((source, reaching));
        }
        let mut interacting = Vec::new();
        for (name, destination) in self.interacting {
            let reaching = reach(Base::At(destination.api()))?;
            let sessions = match destination.sessions() {
                Some(base) => Some(reach(Base::At(base))?),
                None => None,
            };
            interacting.push((name, destination, reaching, sessions));
        }
        Ok(Connected {
            routes,
            sources,
            interacting,
            webhooks: self.webhooks,
        })
    }
}

struct Resolved<'a> {
    destination: &'a Destination,
    base: &'a str,
    secret: Secret,
    marker_key: Secret,
    derived: bool,
}

impl Resolved<'_> {
    /// The destination reached for a route with `options`, where they are
    /// options of the destination's kind.
    fn reached(&self, options: &Placed) -> Option<Reached> {
        self.destination
            .reach(options, self.base, self.secret.clone())
    }
}

/// Whether `key` signs as the actor of the ActivityPub destination
/// `destination`.
///
/// # Errors
///
/// When it is no key `signer` can sign with.
pub(crate) async fn signs_as_actor<G: Signer>(
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
async fn destination_secrets<'c, G: Signer>(
    config: &'c Config,
    secrets: &ReadSecrets,
    signer: &G,
) -> Result<BTreeMap<&'c DestinationName, Resolved<'c>>, PrepareError> {
    let installation = match &config.marker_key {
        Some(key) => Some(secrets.secret(&key.secret).await?),
        None => None,
    };
    let mut resolved: BTreeMap<&DestinationName, Resolved> = BTreeMap::new();
    for (name, destination) in &config.destinations {
        let Some(reference) = destination.credential() else {
            return Err(PrepareError::NoCredential {
                destination: name.as_str().to_owned(),
            });
        };
        let secret = secrets.secret(&reference.secret).await?;
        if destination.acts() {
            signs_as_actor(signer, name, &secret).await?;
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
                base: &config.server.base,
                secret,
                marker_key,
                derived,
            },
        );
    }
    Ok(resolved)
}

/// `config`'s routes with every credential and marker key resolved through
/// `secrets` and `signer`, with what else a run that `uses` it needs: each
/// source's token where it polls, and where it serves, each destination's
/// commands and each source's webhook secret. Every secret the run needs
/// beside its store's is read first, and every one that cannot be is named
/// at once.
///
/// # Errors
///
/// Every secret that cannot be read, or the first marker key that cannot
/// be derived.
pub async fn prepare<S: Secrets, G: Signer>(
    config: &Config,
    secrets: &S,
    signer: &G,
    uses: Uses,
) -> Result<Prepared, PrepareError> {
    let read = read(config, secrets, uses).await?;
    let resolved = destination_secrets(config, &read, signer).await?;
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
    for (name, source) in config.sources.iter().filter(|_| uses >= Uses::Polls) {
        let token = herald_kinds::optional(&read, source.token()).await?;
        sources.push(PreparedSource {
            name: name.clone(),
            poll: source.poller(token),
            backfill: source.backfill(),
        });
    }
    let mut interacting = Vec::new();
    let mut webhooks = BTreeMap::new();
    if uses == Uses::Serves {
        for (name, resolved) in &resolved {
            if let Some(found) = resolved
                .destination
                .interacts(&resolved.secret, &read)
                .await?
            {
                interacting.push(((*name).clone(), found));
            }
        }
        for (name, source) in &config.sources {
            if let Some(named) = source.webhook() {
                webhooks.insert(name.clone(), read.secret(&named.secret).await?);
            }
        }
    }
    Ok(Prepared {
        routes,
        sources,
        interacting,
        webhooks: Webhooks(webhooks),
    })
}

#[cfg(test)]
mod tests {
    use herald_core::{Phrase, ReadBack, Wording};
    use herald_dest_discord::{Credential, Discord};
    use herald_dest_fediverse::{ActivityPub, Mastodon};
    use herald_dest_matrix::Matrix;
    use herald_dest_slack::Slack;
    use serde_json::json;

    use super::*;

    #[test]
    fn a_secret_read_first_is_taken_again_and_one_not_read_is_missing() {
        let read = ReadSecrets([("TOKEN".to_owned(), Secret::new("TOKEN", "t"))].into());
        assert_eq!(
            pollster::block_on(read.secret("TOKEN")),
            Ok(Secret::new("TOKEN", "t"))
        );
        let missing = pollster::block_on(read.secret("OTHER")).unwrap_err();
        assert_eq!(
            PrepareError::from(missing.clone()).to_string(),
            "no secret is named OTHER; set it where this platform keeps secrets"
        );
        let named = UnreadSecret {
            error: missing,
            keys: vec!["a.token".to_owned(), "b.token".to_owned()],
        };
        assert_eq!(
            PrepareError::Unread(vec![named.clone(), named]).to_string(),
            [
                "a.token, b.token: no secret is named OTHER; set it where this platform keeps secrets";
                2
            ]
            .join("\n")
        );
    }

    fn reached(destination: &Destination, options: &Placed) -> Option<Reached> {
        Resolved {
            destination,
            base: "",
            secret: Secret::new("HERALD_TOKEN", "token"),
            marker_key: Secret::new("HERALD_MARKERS_KEY", "key"),
            derived: false,
        }
        .reached(options)
    }

    #[test]
    fn each_destination_reads_back_and_looks_as_its_config_says() {
        let pages = ReadBack::new(9).unwrap();
        let token = json!({ "secret": "HERALD_TOKEN" });
        let secret = || Secret::new("HERALD_TOKEN", "token");
        let discord = Destination::new(
            serde_json::from_value::<herald_dest_discord::DestinationOptions>(json!({
                "token": token, "read_back": 9,
                "colours": { "release": "#112233" }, "archive_after": "1d",
                "interactions": { "application": "1566184281406177000", "wording": { "release_link": "Uitgave" } }
            }))
            .unwrap(),
        );
        let slack = Destination::new(
            serde_json::from_value::<herald_dest_slack::DestinationOptions>(
                json!({ "token": token, "read_back": 9, "domain": "slack-gov.com", "team": "T0123456789" }),
            )
            .unwrap(),
        );
        let matrix = Destination::new(
            serde_json::from_value::<herald_dest_matrix::DestinationOptions>(json!({
                "homeserver": "https://matrix.example", "token": token, "read_back": 9
            }))
            .unwrap(),
        );
        let mastodon = Destination::new(
            serde_json::from_value::<herald_dest_fediverse::DestinationOptions>(json!({
                "server": "https://mastodon.example", "token": token, "read_back": 9
            }))
            .unwrap(),
        );
        let room = herald_dest_matrix::RouteOptions {
            room: "!abcdefg:matrix.example".to_owned(),
        };
        assert_eq!(
            reached(
                &discord,
                &Placed::new(herald_dest_discord::RouteOptions::default())
            ),
            Some(Reached::new(
                Discord::new(
                    Credential::Bot(secret()),
                    herald_dest_discord::RouteOptions::default(),
                    None
                )
                .reading_back(pages)
                .looking(herald_dest_discord::Look {
                    release: herald_dest_discord::Colour(0x0011_2233),
                    archive_after: herald_dest_discord::ArchiveAfter::Day,
                    ..herald_dest_discord::Look::DEFAULT
                })
                .worded(Wording::of(Some(
                    &[(Phrase::ReleaseLink, "Uitgave".to_owned())].into()
                )))
            ))
        );
        assert_eq!(
            reached(
                &slack,
                &Placed::new(herald_dest_slack::RouteOptions::default())
            ),
            Some(Reached::new(
                Slack::new(
                    herald_dest_slack::Credential::Bot(secret()),
                    herald_dest_slack::RouteOptions::default(),
                    None
                )
                .on(herald_dest_slack::Domain::Gov)
                .in_team(Some("T0123456789"))
                .reading_back(pages)
            ))
        );
        assert_eq!(
            reached(&matrix, &Placed::new(room.clone())),
            Some(Reached::new(
                Matrix::new("https://matrix.example", secret(), room).reading_back(pages)
            ))
        );
        assert_eq!(
            reached(
                &mastodon,
                &Placed::new(herald_dest_fediverse::RouteOptions::default())
            ),
            Some(Reached::new(
                Mastodon::new(
                    "https://mastodon.example",
                    secret(),
                    herald_dest_fediverse::RouteOptions::default()
                )
                .reading_back(pages)
            ))
        );
    }

    #[test]
    fn each_destination_waits_as_its_config_says_and_its_platform_elsewhere() {
        let token = json!({ "secret": "HERALD_TOKEN" });
        let secret = || Secret::new("HERALD_TOKEN", "token");
        let outage = jiff::SignedDuration::from_secs(40);
        let waits = json!({ "outage": "40s" });
        let matrix = Destination::new(
            serde_json::from_value::<herald_dest_matrix::DestinationOptions>(
                json!({ "homeserver": "https://matrix.example", "token": token, "waits": waits }),
            )
            .unwrap(),
        );
        let mastodon = Destination::new(
            serde_json::from_value::<herald_dest_fediverse::DestinationOptions>(
                json!({ "server": "https://mastodon.example", "token": token, "waits": waits }),
            )
            .unwrap(),
        );
        let actor = Destination::new(
            serde_json::from_value::<herald_dest_fediverse::ActorOptions>(
                json!({ "domain": "herald.example", "key": token, "waits": waits }),
            )
            .unwrap(),
        );
        let room = herald_dest_matrix::RouteOptions {
            room: "!abcdefg:matrix.example".to_owned(),
        };
        let toot = herald_dest_fediverse::RouteOptions::default();
        let named: herald_dest_fediverse::ActorRouteOptions =
            serde_json::from_value(json!({ "actor": "herald" })).unwrap();
        assert_eq!(
            reached(&matrix, &Placed::new(room.clone())),
            Some(Reached::new(
                Matrix::new("https://matrix.example", secret(), room).waiting(herald_core::Waits {
                    outage,
                    ..herald_core::Waits::DEFAULT
                })
            ))
        );
        assert_eq!(
            reached(&mastodon, &Placed::new(toot.clone())),
            Some(Reached::new(
                Mastodon::new("https://mastodon.example", secret(), toot).waiting(
                    herald_core::Waits {
                        outage,
                        ..Mastodon::WAITS
                    }
                )
            ))
        );
        assert_eq!(
            reached(&actor, &Placed::new(named.clone())),
            Some(Reached::new(
                ActivityPub::new("herald.example", "", secret(), named).waiting(
                    herald_core::Waits {
                        outage,
                        ..herald_core::Waits::DEFAULT
                    }
                )
            ))
        );
    }

    #[test]
    fn a_slack_on_its_government_cloud_takes_commands_there() {
        let slack = Destination::new(
            serde_json::from_value::<herald_dest_slack::DestinationOptions>(json!({
                "token": { "secret": "HERALD_TOKEN" },
                "domain": "slack-gov.com",
                "interactions": { "via": "socket", "app": { "secret": "HERALD_SLACK_APP" } },
            }))
            .unwrap(),
        );
        let secrets = herald_testing::FakeSecrets::default().with("HERALD_SLACK_APP", "xapp");
        let interacting =
            pollster::block_on(slack.interacts(&Secret::new("HERALD_TOKEN", "token"), &secrets))
                .unwrap();
        let interacting = interacting.unwrap();
        let interacting = interacting.of::<herald_dest_slack::Interacting>().unwrap();
        assert_eq!(
            (interacting.api(), interacting.hooks()),
            ("https://slack-gov.com/api", "https://hooks.slack-gov.com")
        );
    }
}
