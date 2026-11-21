//! One source made ready to re-run: its write credential read and held
//! apart from the one it reads with, a connection to its API alone for the
//! call, and the operator signed in at its forge, their token revoked when
//! the apply ends whatever came of it; a write App's token is minted only
//! for a call, narrowed to that call (ADR-0068).

use std::sync::Arc;

use herald_config::Config;
use herald_core::{Permission, SourceName, same};
use herald_kinds::{Credential, Rerunner, SignIn, Source, UNWRITTEN, WriteCredential};
use herald_ports::{Base, Clock, Http, OperatorToken, Secret, Signer, Store, Timer};
use herald_wire::github_app::{Unminted, Wide};

use super::signing::{said, signed_in};
use super::{Applying, read_credential};
use crate::announce::Ports;
use crate::budget::Budgeted;
use crate::installations::{Minting, Mintings};
use crate::prepare::Connected;

/// Why a source's jobs are not re-run where its write credential holds the
/// value its read credential holds under another name.
pub const UNSEPARATED: &str = "the write credential holds the same secret the source reads with, under another name; give it a secret of its own";

/// Why a job is not re-run where the write App's token was minted for an
/// installation a source reads through, or by an App a source reads as.
pub const SHARED_INSTALLATION: &str = "the write App's token was minted for an installation a source reads through, or by an App a source reads as, so the write App is an App herald reads as; re-runs are made as another App, installed with actions: write alone";

/// What a source re-runs with: a token the config names, or a GitHub App
/// whose token is minted for each call, with the installation and the App
/// of every minting a source reads through.
pub(super) enum Writing {
    Token(Credential),
    App {
        minting: Box<Minting>,
        reads: Vec<(u64, u64)>,
    },
}

/// The operator's token at one source and what revokes it, held until the
/// apply ends, whatever came of the sign-in's checks.
pub(super) struct Revoking<H> {
    rerunner: Arc<dyn Rerunner>,
    web: H,
    pub(super) token: OperatorToken,
}

impl<H: Http> Revoking<H> {
    /// The token revoked where its forge lets a public client revoke it,
    /// whatever the budget holds, and dropped.
    pub(super) async fn end(self) {
        let _ = self.rerunner.revoked(&self.web, &self.token).await;
    }
}

/// One source ready to re-run: what re-runs, the API it reads, the
/// connection to its API for the write credential, the write credential,
/// and the operator signed in, with the least role they must hold.
pub(super) struct Signed<'a, H> {
    pub(super) rerunner: Arc<dyn Rerunner>,
    pub(super) api: &'a H,
    pub(super) written: H,
    pub(super) write: Writing,
    pub(super) operator: String,
    pub(super) least: Permission,
}

/// The source `name`'s write credential, as its config names it, read
/// through `applying` with no request made: a token's secret, or its App's
/// minting and key.
async fn write_credential<H, T>(
    config: &Config,
    applying: &Applying<'_, H, T>,
    (name, write): (&SourceName, WriteCredential<'_>),
) -> Result<(Writing, Secret), String> {
    match write {
        WriteCredential::Token { secret, .. } => {
            let read = applying.secrets.secret_boxed(&secret.secret).await;
            let secret =
                read.map_err(|error| format!("the write credential could not be read: {error}"))?;
            Ok((Writing::Token(Credential::Named(secret.clone())), secret))
        }
        WriteCredential::App(installed) => {
            let mut mintings = Mintings::default();
            mintings
                .of((&config.github_apps, applying.secrets), name, installed)
                .await
                .map_err(|error| format!("the write App's key could not be read: {error}"))?;
            let minting = mintings.0.pop().ok_or(UNWRITTEN)?;
            let key = minting.key().clone();
            Ok((
                Writing::App {
                    minting: Box::new(minting),
                    reads: Vec::new(),
                },
                key,
            ))
        }
    }
}

/// Why `wide`, minted by `minting` holding more than it was asked for,
/// was not used, once it is revoked with itself through `signed`'s
/// connection to the source's API, outside the budget as the operator's
/// token is; and whether its revocation failed.
async fn revoked<H: Http>(signed: &Signed<'_, H>, minting: &Minting, wide: Wide) -> String {
    let said = wide.to_string();
    match wide.revoked(&signed.written, &minting.api).await {
        Ok(()) => format!("{said}; it was revoked"),
        Err(failed) => format!("{said}, and its revocation failed: {failed}"),
    }
}

/// The token `signed` calls with on `repository`, minted through its
/// connection to the source's API where it is an App's: the token the
/// config names, or its App's minted now for that repository alone with
/// `actions: write` alone, refused where its installation or its App is
/// one a source reads through.
///
/// # Errors
///
/// Where the App's token cannot be minted or its App found, or it is one a
/// source reads through, saying why.
pub(super) async fn credential<S: Store, C: Clock, G: Signer, H: Http>(
    signed: &Signed<'_, H>,
    ports: &Ports<'_, S, C, G>,
    repository: &str,
) -> Result<Credential, String> {
    let (minting, reads) = match &signed.write {
        Writing::Token(credential) => return Ok(credential.clone()),
        Writing::App { minting, reads } => (minting, reads),
    };
    let name = repository
        .rsplit_once('/')
        .map_or(repository, |(_, name)| name);
    let narrowed = minting.narrowed(herald_wire::github_app::rerunning(name));
    let written = Budgeted::new(&signed.written, ports.budget, ports.clock);
    let unminted = |reason: String| format!("the write App's token could not be minted: {reason}");
    let minted = match narrowed
        .minted((ports.signer, &written), ports.clock.now())
        .await
    {
        Ok(Ok(minted)) => minted,
        Ok(Err(wide)) => return Err(unminted(revoked(signed, minting, wide).await)),
        Err(Unminted::Unreachable(reason) | Unminted::Unreadable(reason)) => {
            return Err(unminted(reason));
        }
    };
    let (installation, app) = narrowed
        .identified((ports.signer, &written), ports.clock.now())
        .await
        .map_err(|error| unminted(error.to_string()))?;
    if reads
        .iter()
        .any(|(read, reading)| *read == installation || *reading == app)
    {
        return Err(SHARED_INSTALLATION.to_owned());
    }
    Ok(Credential::Minted(minted))
}

/// What the source `name`, `source`, re-runs with, before anyone signs in:
/// its write credential held apart from its read one, its rerunner and
/// sign-in, and the installation and App of every minting a source reads
/// through where it writes as an App. No request carries the write
/// credential.
async fn prepared<S: Store, C: Clock, G: Signer, H: Http, T: Timer>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    applying: &Applying<'_, H, T>,
    (name, source): (&SourceName, &Source),
) -> Result<(Writing, Arc<dyn Rerunner>, SignIn), String> {
    let write = source.write().ok_or(UNWRITTEN)?;
    let read = read_credential((source, connected), applying.secrets, name)
        .await
        .map_err(|error| format!("the source's read credential could not be read: {error}"))?;
    let (mut writing, secret) = write_credential(config, applying, (name, write)).await?;
    let held = read.as_ref().and_then(|(_, held)| held.as_ref());
    if held.is_some_and(|held| same(held.expose().as_bytes(), secret.expose().as_bytes())) {
        return Err(UNSEPARATED.to_owned());
    }
    let (rerunner, signing): (Arc<dyn Rerunner>, SignIn) = source
        .rerunner(read.map(|(read, _)| read))
        .and_then(|rerunner| Ok((rerunner.clone(), rerunner.signing()?.clone())))?;
    if let Writing::App { reads, .. } = &mut writing {
        for (minting, http) in &connected.mintings {
            let budgeted = Budgeted::new(http, ports.budget, ports.clock);
            let found = minting
                .identified((ports.signer, &budgeted), ports.clock.now())
                .await;
            reads.push(found.map_err(|unminted| {
                format!("an installation a source reads through could not be found: {unminted}")
            })?);
        }
    }
    Ok((writing, rerunner, signing))
}

/// The source `name`, `source`, read through `api`, made ready to re-run as
/// `applying` reaches: its operator signed in, beside the guard revoking
/// their token, given wherever a token arrived, whatever came after. No
/// request carries the write credential.
///
/// The result's errors are where it names no write credential or sign-in,
/// a credential cannot be read or is the read one, an installation a
/// source reads through cannot be found, or the sign-in ends without the
/// operator, saying why.
pub(super) async fn signed<'a, S: Store, C: Clock, G: Signer, H: Http, T: Timer>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    applying: &Applying<'_, H, T>,
    (name, source, api): (&SourceName, &Source, &'a H),
) -> (Option<Revoking<H>>, Result<Signed<'a, H>, String>) {
    let ports = flow.2;
    let ready = async {
        let (writing, rerunner, signing) = prepared(flow, applying, (name, source)).await?;
        let web = (applying.connect)(Base::At(&signing.web))?;
        let written = (applying.connect)(Base::At(rerunner.api()))?;
        let told = (name.as_str(), applying.told);
        let budgeted = Budgeted::new(&web, ports.budget, ports.clock);
        let token = signed_in(&*rerunner, (&budgeted, applying.timer), told).await?;
        Ok::<_, String>((writing, rerunner, signing, web, written, token))
    };
    let (writing, rerunner, signing, web, written, token) = match ready.await {
        Ok(ready) => ready,
        Err(why) => return (None, Err(why)),
    };
    let revoking = Revoking {
        rerunner: Arc::clone(&rerunner),
        web,
        token,
    };
    let writer = match &writing {
        Writing::App { minting, .. } => Some(minting.issuer()),
        Writing::Token(_) => None,
    };
    let budgeted = |http| Budgeted::new(http, ports.budget, ports.clock);
    let operator = rerunner
        .operator(
            (&budgeted(api), &budgeted(&revoking.web)),
            &revoking.token,
            writer,
        )
        .await
        .map_err(said);
    let signed = operator.map(|operator| Signed {
        rerunner,
        api,
        written,
        write: writing,
        operator,
        least: signing.least,
    });
    (Some(revoking), signed)
}
