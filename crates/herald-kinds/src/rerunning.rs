//! What applies a planned re-run on a forge: the operator's sign-in by the
//! forge's device flow, their permission on a repository, the job and its
//! run read right before the call, and the call, made with the source's
//! write credential alone (ADR-0068).

use std::fmt;

use herald_core::{Permission, Rerun, SecretRef};
use herald_ports::{Boxed, DynHttp, HttpError, OperatorToken, Response, Secret};
use jiff::SignedDuration;

use crate::Credential;
use crate::source::{Installed, Unpolled};

/// What an apply refuses where a source names no write credential.
pub const UNWRITTEN: &str = "the source names no write credential, so herald may not re-run anything it reads; give [sources.<name>] write";

/// What an apply refuses where a source names no sign-in.
pub const UNSIGNED: &str = "the source names no sign_in, so no operator can sign in to apply a re-run; give [sources.<name>] sign_in with the client id of a GitHub App or OAuth application, or of a GitLab application";

/// How an operator signs in at a source's forge: the device flow's web
/// root, the client id it is asked for, the scopes, and the least
/// permission they must hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignIn {
    /// The web root the device flow is asked at, such as
    /// `https://github.com`.
    pub web: String,
    /// The OAuth client id.
    pub client_id: String,
    /// The scopes asked for, read-only.
    pub scopes: Vec<String>,
    /// The least permission a signed-in operator must hold.
    pub least: Permission,
}

/// The credential a source re-runs with, as its config names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteCredential<'a> {
    /// A token the config names, sent to the source's API alone.
    Token {
        /// The secret holding it.
        secret: &'a SecretRef,
        /// The base URL of the source's API.
        api: String,
    },
    /// A GitHub App's installation, its tokens minted for the call.
    App(Installed<'a>),
}

impl WriteCredential<'_> {
    /// The credential's name, as herald records it: the secret's name, or
    /// the App's.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Token { secret, .. } => &secret.secret,
            Self::App(installed) => installed.app,
        }
    }
}

/// What the device flow answered a sign-in with: what the operator is
/// shown, and the code herald asks with until they finish.
pub struct Device {
    /// The code the operator enters.
    pub user_code: String,
    /// The page they enter it at.
    pub verification: String,
    /// The device code herald asks with.
    pub device_code: Secret,
    /// How long herald waits between asks.
    pub interval: SignedDuration,
    /// How long the code lasts.
    pub expires: SignedDuration,
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Device")
            .field("user_code", &self.user_code)
            .field("verification", &self.verification)
            .field("device_code", &self.device_code)
            .finish_non_exhaustive()
    }
}

/// What one ask of the device flow found.
#[derive(Debug)]
pub enum Polled {
    /// The operator has not finished yet.
    Pending,
    /// Ask less often: the interval grows by the forge's step.
    Slower,
    /// Signed in.
    Signed(OperatorToken),
    /// The sign-in ended without a token, for this reason in plain words.
    Ended(String),
}

/// What a signed-in operator may do on a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Granted {
    /// At least the permission asked for.
    Enough,
    /// Less, holding this, as the forge names it.
    Short(String),
}

/// A planned job as the forge describes it right before its re-run is
/// asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunNow {
    /// The head commit its run ran on.
    pub head: String,
    /// Whether it ended failed.
    pub failed: bool,
    /// How many times it, or its run, was re-run.
    pub reruns: u64,
}

/// What re-runs planned jobs of a source's repositories, with its write
/// credential alone, once an operator signed in through the forge.
pub trait Rerunner: fmt::Debug + Send + Sync {
    /// The write credential's name, as herald records it.
    fn credential(&self) -> &str;

    /// How an operator signs in, or why none can.
    ///
    /// # Errors
    ///
    /// Where the forge has no device flow, or the source names no
    /// sign-in, saying so.
    fn signing(&self) -> Result<&SignIn, &'static str>;

    /// The base URL the write credential is sent to: the source's API.
    fn api(&self) -> &str;

    /// A device code for the operator, asked through `web`, which reaches
    /// the sign-in's web root alone.
    fn device<'a>(&'a self, web: &'a dyn DynHttp) -> Boxed<'a, Result<Device, Unpolled>>;

    /// Whether the operator finished signing in with `device`, asked
    /// through `web`.
    fn polled<'a>(
        &'a self,
        web: &'a dyn DynHttp,
        device: &'a Device,
    ) -> Boxed<'a, Result<Polled, Unpolled>>;

    /// The signed-in operator's login, asked with `token` through the
    /// first of `reached`, which reaches the API, once what the forge says
    /// `token` may do was found to read alone: the scopes it holds, asked
    /// through the second, which reaches the sign-in's web root, where the
    /// forge names them there; or where it names none, each installation of
    /// a GitHub App it reaches, none of them of the write App `writer`,
    /// named as its config names it.
    ///
    /// # Errors
    ///
    /// Where the token can do more than read or is the write App's, or the
    /// forge could not be asked.
    fn operator<'a>(
        &'a self,
        reached: (&'a dyn DynHttp, &'a dyn DynHttp),
        token: &'a OperatorToken,
        writer: Option<&'a str>,
    ) -> Boxed<'a, Result<String, Unpolled>>;

    /// `token` revoked through `web` where the forge lets a public client
    /// revoke it: whether it was asked to.
    ///
    /// # Errors
    ///
    /// Where the forge could not be asked, or refused.
    fn revoked<'a>(
        &'a self,
        web: &'a dyn DynHttp,
        token: &'a OperatorToken,
    ) -> Boxed<'a, Result<bool, Unpolled>>;

    /// What the operator holding `token` may do on `repository`, asked
    /// through `api` with that token.
    fn granted<'a>(
        &'a self,
        api: &'a dyn DynHttp,
        token: &'a OperatorToken,
        repository: &'a str,
    ) -> Boxed<'a, Result<Granted, Unpolled>>;

    /// The head of the open pull request `number` of `repository`, read
    /// with the source's read credential; none where it is no longer
    /// open.
    fn head<'a>(
        &'a self,
        api: &'a dyn DynHttp,
        pull: (&'a str, u64),
    ) -> Boxed<'a, Result<Option<String>, Unpolled>>;

    /// The planned job `rerun` of `repository` and its run, read with the
    /// source's read credential.
    fn run<'a>(
        &'a self,
        api: &'a dyn DynHttp,
        job: (&'a str, &'a Rerun),
    ) -> Boxed<'a, Result<RunNow, Unpolled>>;

    /// The answer to the call re-running the planned job `rerun` of
    /// `repository` alone, made through `write`, which reaches the
    /// source's API alone, with the write credential `credential`.
    fn rerun<'a>(
        &'a self,
        write: &'a dyn DynHttp,
        credential: Credential,
        job: (&'a str, &'a Rerun),
    ) -> Boxed<'a, Result<Result<Response, HttpError>, Unpolled>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_credential_is_named_as_its_config_names_it_and_a_device_code_never_shown() {
        let token = SecretRef {
            secret: "HERALD_RERUN".to_owned(),
        };
        let named = WriteCredential::Token {
            secret: &token,
            api: "https://api.github.com".to_owned(),
        };
        assert_eq!(named.name(), "HERALD_RERUN");
        let installed = Installed {
            app: "rerun",
            api: "https://api.github.com".to_owned(),
            installation: crate::Installation::Fixed(1),
        };
        assert_eq!(WriteCredential::App(installed).name(), "rerun");
        let device = Device {
            user_code: "WDJB-MJHT".to_owned(),
            verification: "https://github.com/login/device".to_owned(),
            device_code: Secret::new("the device code", "device-value"),
            interval: SignedDuration::from_secs(5),
            expires: SignedDuration::from_mins(15),
        };
        let shown = format!("{device:?}");
        assert!(
            shown.contains("WDJB-MJHT") && !shown.contains("device-value"),
            "{shown}"
        );
    }
}
