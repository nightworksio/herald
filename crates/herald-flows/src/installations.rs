//! The GitHub Apps sources read as: an installation token minted for each
//! installation they read, kept until shortly before it expires
//! (ADR-0041).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use herald_config::GithubApp;
use herald_core::KeptToken;
use herald_core::SourceName;
use herald_kinds::{Installation, Installed, Minted};
use herald_ports::{
    Algorithm, Clock, DynSecrets, Http, Sealing, Secret, SecretError, Signer, Store, Tokens,
    Version,
};
use jiff::{SignedDuration, Timestamp};

use crate::announce::{Ports, Stop};
use herald_wire::github_app::{Narrowed, Unminted};

/// What an installation token kept in the store is sealed for.
const PURPOSE: &str = "herald github-app installation token v1";

/// Sealing a token for its purpose, bound to `bound`.
const fn sealing(bound: &str) -> Sealing<'_> {
    Sealing {
        purpose: PURPOSE,
        bound: bound.as_bytes(),
    }
}

/// How long before a token expires it is minted again.
pub const AHEAD: SignedDuration = SignedDuration::from_mins(5);

/// The installation an App's tokens are minted for, as a source names it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Owner(String),
    Repository(String),
    Fixed(u64),
}

impl Target {
    fn of(installation: Installation<'_>) -> Self {
        match installation {
            Installation::Owner(owner) => Self::Owner(owner.to_owned()),
            Installation::Repository(repository) => Self::Repository(repository.to_owned()),
            Installation::Fixed(id) => Self::Fixed(id),
        }
    }

    fn installation(&self) -> Installation<'_> {
        match self {
            Self::Owner(owner) => Installation::Owner(owner),
            Self::Repository(repository) => Installation::Repository(repository),
            Self::Fixed(id) => Installation::Fixed(*id),
        }
    }
}

/// What the last minting learned: the installation's id, its App's id,
/// and when its token expires.
#[derive(Debug, Default)]
struct Held {
    id: Option<u64>,
    app: Option<u64>,
    expires: Option<Timestamp>,
}

/// One installation of a GitHub App, with the sources reading it and the
/// token they read with.
#[derive(Debug, Clone)]
pub struct Minting {
    app: String,
    issuer: String,
    pub(crate) api: String,
    key: Secret,
    target: Target,
    sources: Vec<SourceName>,
    minted: Arc<Minted>,
    held: Arc<Mutex<Held>>,
    stored: bool,
    narrowed: Option<Narrowed>,
}

/// Every installation the prepared sources read, as they are prepared.
#[derive(Debug, Default)]
pub(crate) struct Mintings(pub(crate) Vec<Minting>);

impl Mintings {
    /// The token the source `name`, reading as `installed`, reads with: the
    /// one minted for the installation another source reads, or a new one
    /// for its App of `apps`, its key read through `secrets`.
    pub(crate) async fn of(
        &mut self,
        (apps, secrets): (&BTreeMap<String, GithubApp>, &dyn DynSecrets),
        name: &SourceName,
        installed: Installed<'_>,
    ) -> Result<Option<Arc<Minted>>, SecretError> {
        let target = Target::of(installed.installation);
        if let Some(minting) = self
            .0
            .iter_mut()
            .find(|minting| minting.app == installed.app && minting.target == target)
        {
            minting.sources.push(name.clone());
            return Ok(Some(Arc::clone(&minting.minted)));
        }
        let Some(app) = apps.get(installed.app) else {
            return Ok(None);
        };
        let key = secrets.secret_boxed(&app.private_key.secret).await?;
        let minted = Arc::new(Minted::default());
        self.0.push(Minting {
            app: installed.app.to_owned(),
            issuer: app.issuer.clone(),
            api: app.api.clone(),
            key,
            target,
            sources: vec![name.clone()],
            minted: Arc::clone(&minted),
            held: Arc::new(Mutex::new(Held::default())),
            stored: app.store_token,
            narrowed: None,
        });
        Ok(Some(minted))
    }
}

impl Minting {
    /// The token the source `name` reads with, where this installation's.
    pub(crate) fn minted_for(&self, name: &SourceName) -> Option<Arc<Minted>> {
        self.sources
            .contains(name)
            .then(|| Arc::clone(&self.minted))
    }

    /// The App's private key.
    pub(crate) const fn key(&self) -> &Secret {
        &self.key
    }

    /// A minting of the same App's installation of its own, never kept in
    /// a store, whose token holds only what `narrowed` names (ADR-0068).
    pub(crate) fn narrowed(&self, narrowed: Narrowed) -> Self {
        Self {
            app: self.app.clone(),
            issuer: self.issuer.clone(),
            api: self.api.clone(),
            key: self.key.clone(),
            target: self.target.clone(),
            sources: self.sources.clone(),
            minted: Arc::new(Minted::default()),
            held: Arc::new(Mutex::new(Held::default())),
            stored: false,
            narrowed: Some(narrowed),
        }
    }

    /// The App's id or client id, as its config names it.
    pub(crate) fn issuer(&self) -> &str {
        &self.issuer
    }

    /// The token it mints.
    pub(crate) fn minted(&self) -> Arc<Minted> {
        Arc::clone(&self.minted)
    }

    /// The id of its installation, where it is named or a token was minted
    /// for it.
    pub(crate) fn installation(&self) -> Option<u64> {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner).id;
        match self.target {
            Target::Fixed(id) => Some(id),
            Target::Owner(_) | Target::Repository(_) => held,
        }
    }

    /// The id of its installation and of its App, each looked up at its API
    /// through `http`, signed by `signer`, where neither named nor found
    /// before: the installation's lookup names its App, and an installation
    /// named by its id is asked for its App.
    ///
    /// # Errors
    ///
    /// Where the App signs nothing or a lookup fails.
    pub(crate) async fn identified<G: Signer, H: Http>(
        &self,
        (signer, http): (&G, &H),
        now: Timestamp,
    ) -> Result<(u64, u64), Unminted> {
        let app = self.held.lock().unwrap_or_else(PoisonError::into_inner).app;
        if let (Some(id), Some(app)) = (self.installation(), app) {
            return Ok((id, app));
        }
        let jwt = self.jwt(signer, now).await?;
        let erased = herald_ports::Erased(http as &dyn herald_ports::DynHttp);
        let found = herald_wire::github_app::installation_id(
            &erased,
            (&self.api, self.target.installation(), self.installation()),
            &jwt,
        )
        .await?;
        let app = match found.app.or(app) {
            Some(app) => app,
            None => herald_wire::github_app::app_of(&erased, (&self.api, found.id), &jwt).await?,
        };
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.id = Some(found.id);
        held.app = Some(app);
        Ok((found.id, app))
    }

    /// The App's assertion of itself, signed with its key.
    async fn jwt<G: Signer>(&self, signer: &G, now: Timestamp) -> Result<Secret, Unminted> {
        let assertion = herald_wire::github_app::assertion(&self.issuer, now);
        let signature = signer
            .sign(Algorithm::RsaSha256, &self.key, assertion.as_bytes())
            .await
            .map_err(|error| {
                Unminted::Unreadable(format!(
                    "the GitHub App {}'s private key signs nothing: {error}",
                    self.app
                ))
            })?;
        Ok(herald_wire::github_app::jwt(
            &self.app, &assertion, &signature,
        ))
    }

    /// The name its token is kept in the store as.
    fn kept_name(&self) -> String {
        let target = match &self.target {
            Target::Owner(owner) => format!("owner/{owner}"),
            Target::Repository(repository) => format!("repository/{repository}"),
            Target::Fixed(id) => format!("installation/{id}"),
        };
        format!("github_apps.{}/{target}", self.app)
    }

    /// What a token kept for the installation, expiring at `expires`, is
    /// sealed bound to: its record's name and its expiry, so a sealed token
    /// moved to another record, or given a later expiry, does not open.
    fn bound(&self, expires: Timestamp) -> String {
        format!("{}|{expires}", self.kept_name())
    }

    /// The token `tokens` keeps for the installation, opened by `signer`,
    /// with when it expires, where it is good at `now` for longer than
    /// [`AHEAD`]; and the version of the record either way.
    async fn kept<G: Signer, T: Tokens>(
        &self,
        (signer, tokens): (&G, &T),
        now: Timestamp,
    ) -> (Option<(Secret, Timestamp)>, Option<Version>) {
        let Ok(Some(record)) = tokens.token(&self.kept_name()).await else {
            return (None, None);
        };
        let opened = async {
            let sealed = record.value.sealed_bytes()?;
            let bound = self.bound(record.value.expires);
            let plain = signer
                .open(&self.key, sealing(&bound), &sealed)
                .await
                .ok()?;
            let token = String::from_utf8(plain).ok()?;
            Some((
                Secret::new(format!("github_apps.{}", self.app), token),
                record.value.expires,
            ))
        };
        let good = opened.await.filter(|(_, expires)| now < *expires - AHEAD);
        (good, Some(record.version))
    }

    /// `token`, expiring at `expires`, sealed by `signer` and kept in the
    /// store `kind`'s `tokens` over the record at `version`; or why it is
    /// not, so a later run mints it again.
    async fn keep<G: Signer, T: Tokens>(
        &self,
        (signer, (tokens, kind)): (&G, Kept<'_, T>),
        (token, expires): (&Secret, Timestamp),
        version: Option<Version>,
    ) -> Result<(), String> {
        let bound = self.bound(expires);
        let sealed = signer
            .seal(&self.key, sealing(&bound), token.expose().as_bytes())
            .await
            .map_err(|error| format!("it could not be sealed: {error}"))?;
        let kept = KeptToken::new(self.kept_name(), &sealed, expires);
        tokens
            .put_token(&kept, version)
            .await
            .map(drop)
            .map_err(|error| format!("the {kind} store refused it: {error}"))
    }

    /// A token for the sources reading the installation at `now`: the one
    /// held in memory, or the one `tokens` keeps where the App keeps them,
    /// while good for longer than [`AHEAD`]; else one minted through `http`,
    /// signed by `signer`, and kept, with why it is not kept where it is
    /// not.
    async fn refreshed<G: Signer, H: Http, T: Tokens>(
        &self,
        (signer, http, tokens): (&G, &H, Option<Kept<'_, T>>),
        now: Timestamp,
    ) -> Result<Option<String>, Unminted> {
        let (known, app) = {
            let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
            if held.expires.is_some_and(|expires| now < expires - AHEAD) {
                return Ok(None);
            }
            (held.id, held.app)
        };
        let tokens = tokens.filter(|_| self.stored);
        let version = match tokens {
            Some((tokens, _)) => match self.kept((signer, tokens), now).await {
                (Some((token, expires)), _) => {
                    self.minted.refresh(Some(token));
                    self.held
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .expires = Some(expires);
                    return Ok(None);
                }
                (None, version) => version,
            },
            None => None,
        };
        let jwt = self.jwt(signer, now).await?;
        let minted = herald_wire::github_app::minted(
            http,
            (&self.api, self.target.installation(), known),
            (&format!("github_apps.{}", self.app), &jwt),
            self.narrowed.as_ref(),
        )
        .await?;
        let unkept = match tokens {
            Some(tokens) => self
                .keep((signer, tokens), (&minted.token, minted.expires), version)
                .await
                .err(),
            None => None,
        };
        self.minted.refresh(Some(minted.token));
        *self.held.lock().unwrap_or_else(PoisonError::into_inner) = Held {
            id: Some(minted.installation),
            app: minted.app.or(app),
            expires: Some(minted.expires),
        };
        Ok(unkept)
    }
}

/// A store's tokens, with the name of its kind.
pub(crate) type Kept<'a, T> = (&'a T, &'a str);

/// A token minted for an App's installation but not kept in the store, the
/// sources that read with it, and why.
#[derive(Debug)]
pub(crate) struct Unkept {
    pub(crate) app: String,
    pub(crate) sources: Vec<SourceName>,
    pub(crate) why: String,
}

/// What giving each installation a token did: why the sources reading one
/// that could not be minted are not read, and each token not kept.
#[derive(Debug, Default)]
pub(crate) struct Refreshed {
    pub(crate) unminted: BTreeMap<SourceName, Unminted>,
    pub(crate) unkept: Vec<Unkept>,
}

/// Each installation of `mintings` given a token good for the round, kept
/// in the store `kind`; why the sources reading one that could not be
/// minted are not polled; and each token not kept.
pub(crate) async fn refreshed<S: Store, C: Clock, G: Signer, H: Http>(
    mintings: &[(Minting, H)],
    ports: &Ports<'_, S, C, G>,
    kind: &str,
) -> (BTreeMap<SourceName, Stop>, Vec<Unkept>) {
    let refreshed = refreshed_by(
        mintings,
        (
            ports.signer,
            ports.store.tokens().map(|tokens| (tokens, kind)),
        ),
        ports.clock.now(),
    )
    .await;
    let stops = refreshed
        .unminted
        .into_iter()
        .map(|(source, unminted)| {
            let stop = match unminted {
                Unminted::Unreachable(reason) => Stop::Unreachable(reason),
                Unminted::Unreadable(reason) => Stop::Source(reason),
                wide @ Unminted::Wide(_) => Stop::Source(wide.to_string()),
            };
            (source, stop)
        })
        .collect();
    (stops, refreshed.unkept)
}

/// Each installation of `mintings` given a token good at `now`, signed by
/// `signer` and kept in `tokens` where its App keeps them.
pub(crate) async fn refreshed_by<G: Signer, H: Http, T: Tokens>(
    mintings: &[(Minting, H)],
    (signer, tokens): (&G, Option<Kept<'_, T>>),
    now: Timestamp,
) -> Refreshed {
    let mut refreshed = Refreshed::default();
    for (minting, http) in mintings {
        match minting.refreshed((signer, http, tokens), now).await {
            Ok(None) => {}
            Ok(Some(why)) => refreshed.unkept.push(Unkept {
                app: minting.app.clone(),
                sources: minting.sources.clone(),
                why,
            }),
            Err(stop) => {
                minting.minted.refresh(None);
                *minting.held.lock().unwrap_or_else(PoisonError::into_inner) = Held::default();
                for source in &minting.sources {
                    refreshed.unminted.insert(source.clone(), stop.clone());
                }
            }
        }
    }
    refreshed
}
