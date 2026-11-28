//! A config's credentials and marker keys resolved, and each route and
//! source given the `Http` that reaches it.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use herald_config::{Config, Destination, Pages, Placed, RouteConfig, Uses};
use herald_core::{DestinationName, Edits, RouteId, SourceName, hex, itself};
use herald_kinds::{
    Credential, Expand, Interacted, Polling, Posts, Queue, Reached, Recorded, Registering, Vouching,
};
use herald_peers::discovery::Instance;
use herald_ports::{
    Algorithm, Base, Binding, CryptoError, DynInfer, Http, Secret, SecretError, Secrets, Signer,
};
use herald_store_markers::{MarkedRoute, MarkersStore};

use herald_models::{KeyHeader, Model, bedrock};

use crate::acting::Projects;
use crate::installations::{Minting, Mintings};
use crate::serving::{HOOKS, Webhooks};

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
                "the destination \"{destination}\" signs with {secret}, which is not an RSA private key of 2048 to 8192 bits in a PKCS#8 PEM; make one with `herald activitypub keygen <file>`"
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
    pub(crate) secret: Secret,
    pub(crate) marker_key: Secret,
    pub(crate) derived: bool,
    /// The instance's domain, which a feed is served on; empty where
    /// `[federation]` names none.
    pub(crate) domain: String,
    /// The path every endpoint is served under.
    pub(crate) base: String,
    /// How many times a record is written before another run's writes
    /// stop it.
    pub(crate) attempts: usize,
    /// How long a request the instance signs stays signed.
    pub(crate) signed_for: jiff::SignedDuration,
}

impl PreparedRoute {
    /// The instance as a feed's envelopes and pages name it.
    pub(crate) fn instance(&self) -> Instance<'_> {
        Instance {
            domain: &self.domain,
            base: &self.base,
            software: itself::NAME,
            version: itself::VERSION,
            repository: herald_core::repository!(),
        }
    }
}

/// A merge queue, with what reads its repository.
#[derive(Debug, Clone)]
pub(crate) struct MergeReading {
    /// The queue's name, as `[merge_queues.<name>]` names it.
    pub(crate) name: String,
    /// The base branch it merges into.
    pub(crate) branch: String,
    /// Who runs it.
    pub(crate) runs: herald_core::merging::Runs,
    /// How it merges, where it says.
    pub(crate) method: Option<herald_core::merging::Method>,
    /// Its forge, as a source's kind names it.
    pub(crate) forge: &'static str,
    /// Whether its forge never has a merge queue of its own, which doctor
    /// says without asking.
    pub(crate) queueless: bool,
    /// What reads its repository with the source's read credential.
    pub(crate) reader: Arc<dyn herald_kinds::MergeReader>,
}

/// A source ready to poll.
#[derive(Debug, Clone)]
pub struct PreparedSource {
    pub(crate) name: SourceName,
    pub(crate) poll: Polling,
    pub(crate) backfill: usize,
    /// What reads its repository's open pull requests for a view, where it
    /// names one repository on a forge (ADR-0055).
    pub(crate) queue: Option<Arc<dyn Queue>>,
    /// What reads the repository of each merge queue naming it (ADR-0087).
    pub(crate) merging: Vec<MergeReading>,
    /// Where each member's webhook points, where some registers one, and
    /// the secret it is signed with, where each round registers one on a
    /// member that has none.
    pub(crate) hook: Option<(String, Option<Secret>)>,
}

/// The owner source `name` of `connected` expands to, where it is one the
/// run polls.
pub(crate) fn expanded<'a, H>(
    connected: &'a Connected<H>,
    name: &SourceName,
) -> Option<&'a Arc<dyn Expand>> {
    connected
        .sources
        .iter()
        .find(|(prepared, _)| prepared.name == *name)
        .and_then(|(prepared, _)| match &prepared.poll {
            Polling::Many(expand) => Some(expand),
            Polling::One(_) | Polling::Pull(_) | Polling::Unpolled => None,
        })
}

/// A config's routes, each with its destination's credential resolved, and
/// its sources, each with its token.
#[derive(Debug, Clone)]
pub struct Prepared {
    models: Vec<PreparedModel>,
    routes: Vec<PreparedRoute>,
    projects: BTreeMap<RouteId, Projects>,
    sources: Vec<PreparedSource>,
    mintings: Vec<Minting>,
    interacting: Vec<(DestinationName, Interacted)>,
    webhooks: Webhooks,
    instance: Vec<Secret>,
    pages: Option<PreparedPages>,
    issuers: Vec<(String, Vec<String>)>,
    recorded: Recorded,
}

/// Where the static signed log is written, with the token that pushes to
/// its branch read, where it is committed to one (ADR-0060).
#[derive(Debug, Clone)]
pub struct PreparedPages {
    pub(crate) pages: Pages,
    pub(crate) token: Option<Secret>,
    pub(crate) repository: Option<String>,
    pub(crate) api: String,
}

impl Prepared {
    /// It with the static signed log's branch in `repository`, the run's
    /// own, where `[federation.pages]` names none.
    #[must_use]
    pub fn in_repository(mut self, repository: Option<&str>) -> Self {
        if let Some(pages) = &mut self.pages {
            pages.repository = pages
                .repository
                .take()
                .or_else(|| repository.map(str::to_owned));
        }
        self
    }

    /// It as a build that asks models where `asks` says so, and else with
    /// no model, so every task answers unknown (ADR-0057).
    #[must_use]
    pub fn asking(self, asks: bool) -> Self {
        Self {
            models: if asks { self.models } else { Vec::new() },
            ..self
        }
    }
}

/// A model a task may ask, its key read, and whether it may be given a
/// private repository's data (ADR-0057).
#[derive(Debug, Clone)]
pub struct PreparedModel {
    pub(crate) name: String,
    pub(crate) model: Model,
    pub(crate) private: bool,
}

/// How a run reaches a model a task may ask (ADR-0059).
#[derive(Debug)]
pub enum Asking<H> {
    /// Through its API, over this `Http`.
    Http(H),
    /// Through its host's binding.
    Bound(Arc<dyn DynInfer>),
    /// Through a binding the run was given nothing for, so it is not asked.
    Unbound,
}

/// A destination that takes commands, by name, with the `Http` its
/// answers go through, the one its sessions open through and the one the
/// keys its requests are vouched for with are read through, where it has
/// them.
pub(crate) type Interacting<H> = (DestinationName, Interacted, H, Option<H>, Option<H>);

/// The prepared routes and sources, each with the `Http` that reaches it.
#[derive(Debug)]
pub struct Connected<H> {
    pub(crate) models: Vec<(PreparedModel, Asking<H>)>,
    pub(crate) routes: Vec<(PreparedRoute, H)>,
    /// The projects each route announcing each project as an actor of its
    /// own names its actors from, by the route's id (ADR-0036).
    pub(crate) projects: BTreeMap<RouteId, Projects>,
    pub(crate) sources: Vec<(PreparedSource, H)>,
    pub(crate) mintings: Vec<(Minting, H)>,
    pub(crate) interacting: Vec<Interacting<H>>,
    webhooks: Webhooks,
    pub(crate) instance: Vec<Secret>,
    pub(crate) pages: Option<(PreparedPages, H)>,
    pub(crate) issuers: Vec<(String, H)>,
    pub(crate) recorded: Recorded,
}

impl<H> Connected<H> {
    /// It with each model its host runs reached through the binding `bind`
    /// gives for the model's name and binding (ADR-0059).
    ///
    /// # Errors
    ///
    /// The first error `bind` gives.
    pub fn bind<E>(
        mut self,
        mut bind: impl FnMut(&str, Binding<'_>) -> Result<Arc<dyn DynInfer>, E>,
    ) -> Result<Self, E> {
        for (prepared, asking) in &mut self.models {
            if matches!(asking, Asking::Unbound)
                && let Some(binding) = prepared.model.binding()
            {
                *asking = Asking::Bound(bind(&prepared.name, binding)?);
            }
        }
        Ok(self)
    }

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
            .map(|(_, interacting, http, ..)| (interacting, http))
    }

    /// What the bearer token each request posted for the destination
    /// named `name` carries must hold, with the `Http` the keys it is
    /// checked with are read through, where its requests are vouched for so.
    #[must_use]
    pub fn vouching(&self, name: &DestinationName) -> Option<(&Vouching, &H)> {
        self.interacting
            .iter()
            .find(|(named, ..)| named == name)
            .and_then(|(_, interacting, .., vouches)| interacting.vouching().zip(vouches.as_ref()))
    }

    /// Every destination that takes commands, by name, with the `Http` its
    /// answers go through and the one its sessions open through, where it
    /// holds any.
    pub fn interactions(
        &self,
    ) -> impl Iterator<Item = (&DestinationName, &Interacted, &H, Option<&H>)> {
        self.interacting
            .iter()
            .map(|(name, interacting, http, sessions, _)| {
                (name, interacting, http, sessions.as_ref())
            })
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
        let mut made: BTreeMap<Vec<String>, H> = BTreeMap::new();
        let mut public: Option<H> = None;
        let mut reach = |base: Base<'_>| -> Result<H, E> {
            let bases = base.bases();
            let known = match &bases {
                Some(bases) => made.get(bases),
                None => public.as_ref(),
            };
            if let Some(reaching) = known {
                return Ok(reaching.clone());
            }
            let reaching = http(base)?;
            match bases {
                Some(bases) => made.insert(bases, reaching.clone()),
                None => public.replace(reaching.clone()),
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
            let reaching = reach(source.poll.base())?;
            sources.push((source, reaching));
        }
        let mut interacting = Vec::new();
        for (name, destination) in self.interacting {
            let reaching = reach(Base::At(destination.api()))?;
            let sessions = destination
                .sessions()
                .map(|base| reach(Base::At(base)))
                .transpose()?;
            let vouches = destination
                .vouching()
                .map(|vouching| reach(Base::At(&vouching.metadata)))
                .transpose()?;
            interacting.push((name, destination, reaching, sessions, vouches));
        }
        let mintings = self
            .mintings
            .into_iter()
            .map(|minting| reach(Base::At(&minting.api)).map(|reaching| (minting, reaching)))
            .collect::<Result<Vec<_>, E>>()?;
        let models = self
            .models
            .into_iter()
            .map(|model| {
                if model.model.binding().is_some() {
                    return Ok((model, Asking::Unbound));
                }
                reach(Base::At(&model.model.base)).map(|reaching| (model, Asking::Http(reaching)))
            })
            .collect::<Result<Vec<_>, E>>()?;
        let pages = self
            .pages
            .map(|pages| reach(Base::At(&pages.api)).map(|reaching| (pages, reaching)))
            .transpose()?;
        let issuers = self
            .issuers
            .into_iter()
            .map(|(issuer, bases)| reach(Base::Among(&bases)).map(|reached| (issuer, reached)))
            .collect::<Result<Vec<_>, E>>()?;
        Ok(Connected {
            issuers,
            pages,
            models,
            routes,
            projects: self.projects,
            sources,
            mintings,
            interacting,
            webhooks: self.webhooks,
            instance: self.instance,
            recorded: self.recorded,
        })
    }
}

struct Resolved<'a> {
    destination: &'a Destination,
    base: &'a str,
    secret: Secret,
    read: Vec<Secret>,
    marker_key: Secret,
    derived: bool,
}

impl Resolved<'_> {
    /// The destination reached for a route with `options`, where they are
    /// options of the destination's kind.
    fn reached(&self, options: &Placed) -> Option<Reached> {
        self.destination
            .reach_reading(options, self.base, (self.secret.clone(), self.read.clone()))
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
        let Some(reference) = config.credential(destination) else {
            return Err(PrepareError::NoCredential {
                destination: name.as_str().to_owned(),
            });
        };
        let secret = secrets.secret(&reference.secret).await?;
        let mut read = Vec::new();
        for reference in destination.reads() {
            read.push(secrets.secret(&reference.secret).await?);
        }
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
                read,
                marker_key,
                derived,
            },
        );
    }
    Ok(resolved)
}

/// Each model of `config` a run that `uses` it asks, its key or the access
/// key a request to Amazon Bedrock is signed with read from `read`, and a
/// session token from `secrets` where it is set (ADR-0057, ADR-0059).
async fn prepared_models<S: Secrets>(
    config: &Config,
    (read, secrets): (&ReadSecrets, &S),
    uses: Uses,
) -> Result<Vec<PreparedModel>, PrepareError> {
    let mut models = Vec::new();
    for (name, model) in config.models.iter().filter(|_| uses >= Uses::Polls) {
        let key = herald_kinds::optional(read, model.key.as_ref()).await?;
        let aws = match (&model.bedrock, &key) {
            (Some(reached), None) => Some(bedrock::Keys {
                region: reached.region.clone(),
                access_key_id: read.secret(&reached.access_key_id.secret).await?,
                secret_access_key: read.secret(&reached.secret_access_key.secret).await?,
                session_token: secrets.secret(&reached.session_token.secret).await.ok(),
            }),
            _ => None,
        };
        models.push(PreparedModel {
            name: name.clone(),
            model: Model {
                speaks: model.speaks,
                base: model.base.clone(),
                model: model.model.clone(),
                key,
                header: model
                    .header
                    .clone()
                    .map_or(KeyHeader::Bearer, KeyHeader::Named),
                most: model.most,
                tokens: model.tokens,
                aws,
                binding: model.binding.clone(),
            },
            private: model.private,
        });
    }
    Ok(models)
}

/// The `[federation]` keys a run that `uses` `config` signs with, and where
/// it writes the static signed log, each secret read from `read`
/// (ADR-0060).
async fn prepared_instance(
    config: &Config,
    read: &ReadSecrets,
    uses: Uses,
) -> Result<(Vec<Secret>, Option<PreparedPages>), PrepareError> {
    let mut instance = Vec::new();
    if uses == Uses::Serves || config.federation.pages.is_some() {
        for key in &config.federation.keys {
            instance.push(read.secret(&key.secret).await?);
        }
    }
    let pages = match config
        .federation
        .pages
        .as_ref()
        .filter(|_| uses != Uses::Serves)
    {
        Some(pages) => Some(PreparedPages {
            token: match pages.branch {
                Some(_) => Some(read.secret(pages.token_name()).await?),
                None => None,
            },
            repository: pages.repository.clone(),
            api: herald_wire::github::api(pages.api.as_deref()),
            pages: pages.clone(),
        }),
        None => None,
    };
    Ok((instance, pages))
}

/// Each route of `config`, its destination reached as `resolved` reaches
/// it.
///
/// # Errors
///
/// A route whose destination is not among them.
fn prepared_routes(
    config: &Config,
    resolved: &BTreeMap<&DestinationName, Resolved<'_>>,
) -> Result<Vec<PreparedRoute>, PrepareError> {
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
            secret: destination.secret.clone(),
            marker_key: destination.marker_key.clone(),
            derived: destination.derived,
            domain: config.federation.domain.clone().unwrap_or_default(),
            base: config.instance_base().to_owned(),
            attempts: config.attempts.get(),
            signed_for: config.federation.signed_for,
        });
    }
    Ok(routes)
}

/// What reads the repository of each of `config`'s merge queues on the
/// source `name`, `source`, with its read credential `token`.
fn merge_readings(
    config: &Config,
    (name, source): (&SourceName, &herald_kinds::Source),
    token: Option<&Credential>,
) -> Vec<MergeReading> {
    config
        .merge_queues
        .iter()
        .filter(|(_, queue)| queue.source == *name)
        .filter_map(|(queue, merging)| {
            Some(MergeReading {
                name: queue.clone(),
                branch: merging.branch.clone(),
                runs: merging.runs,
                method: merging.method,
                forge: merging.forge.forge,
                queueless: merging.forge.queueless.is_some(),
                reader: source.merge_reader(&merging.forge, token.cloned())?,
            })
        })
        .collect()
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
    let routes = prepared_routes(config, &resolved)?;
    let mut sources = Vec::new();
    let mut mintings = Mintings::default();
    for (name, source) in config.sources.iter().filter(|_| uses >= Uses::Polls) {
        let token = match source.installed() {
            Some(installed) => mintings
                .of((&config.github_apps, &read), name, installed)
                .await?
                .map(Credential::Minted),
            None => herald_kinds::optional(&read, source.token())
                .await?
                .map(Credential::Named),
        };
        let edits = config.routes.iter().any(|route| {
            route.route.delivery.edits == Edits::Release
                && (route.route.filter.sources.contains(name)
                    || source.names_route(&route.route.id))
        });
        let hook = match (source.registering(), &config.server.url) {
            (Registering::Never, _) | (_, None) => None,
            (registering, Some(url)) => Some((
                format!("{}{HOOKS}{}", url.trim_end_matches('/'), name.as_str()),
                match (registering, source.webhook()) {
                    (Registering::Round, Some(named)) => Some(read.secret(&named.secret).await?),
                    _ => None,
                },
            )),
        };
        sources.push(PreparedSource {
            name: name.clone(),
            queue: source.queue(token.clone()),
            merging: merge_readings(config, (name, source), token.as_ref()),
            poll: source.poller(token, edits),
            backfill: source.backfill(),
            hook,
        });
    }
    let mut interacting = Vec::new();
    let mut webhooks = Webhooks::default();
    let (instance, pages) = prepared_instance(config, &read, uses).await?;
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
                webhooks
                    .sources
                    .insert(name.clone(), read.secret(&named.secret).await?);
            }
        }
        for (name, app) in &config.github_apps {
            if let Some(named) = &app.webhook {
                webhooks
                    .apps
                    .insert(name.clone(), read.secret(&named.secret).await?);
            }
        }
    }
    Ok(Prepared {
        models: prepared_models(config, (&read, secrets), uses).await?,
        routes,
        projects: config
            .routes
            .iter()
            .filter_map(|route| Some((route.route.id.clone(), Projects::of(config, route)?)))
            .collect(),
        sources,
        mintings: mintings.0,
        interacting,
        webhooks,
        instance,
        pages,
        issuers: if uses == Uses::Serves {
            crate::oidc::issuers(config)
        } else {
            Vec::new()
        },
        recorded: if config.store.reads_posts() {
            Recorded::Posts
        } else {
            Recorded::Kept
        },
    })
}

#[cfg(test)]
mod tests;
