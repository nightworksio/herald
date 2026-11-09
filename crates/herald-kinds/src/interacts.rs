use std::any::Any;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use herald_core::{Answering, Asked, Dialect, Room, SecretRef, Text};
use herald_ports::{DynSecrets, Request, Response, Secret, SecretError};

/// A destination that takes commands, with the secrets it needs resolved:
/// what every kind's commands are to the flows that answer them
/// (ADR-0016).
pub trait Interacts: fmt::Debug + Send + Sync + Any {
    /// The base URL every answer it gives is sent to.
    fn api(&self) -> &str;

    /// The base URL a session herald holds is opened at, where it holds one
    /// beside the answers it sends.
    fn sessions(&self) -> Option<&str> {
        None
    }

    /// How it answers, whatever carries the commands.
    fn answering(&self) -> Answering;

    /// The dialect its answers are written in, and the room each part of
    /// one fits.
    fn room(&self) -> (Dialect, Room);

    /// The requests that answer the command `replying` names with `parts`,
    /// seen by the asker alone unless `public`; none where `replying` is of
    /// another kind.
    fn answered(&self, replying: &Replying, parts: &[Text], public: bool) -> Vec<Request>;

    /// Whether it took a request answering a command, or else why not.
    ///
    /// # Errors
    ///
    /// What it said.
    fn took(&self, response: &Response) -> Result<(), String>;

    /// The request saying `asked` is being worked, where it waits for that
    /// on a command `replying` names that no endpoint answered.
    fn deferred(&self, _replying: &Replying, _asked: &Asked) -> Option<Request> {
        None
    }

    /// Itself, for code that reads one kind's own capabilities.
    fn as_any(&self) -> &dyn Any;

    /// Whether `other` is the same, taking commands the same way.
    fn same(&self, other: &dyn Any) -> bool;
}

/// The secret `named` names, read through `secrets`, where it names one.
///
/// # Errors
///
/// Where the secret it names cannot be read.
pub async fn optional(
    secrets: &dyn DynSecrets,
    named: Option<&SecretRef>,
) -> Result<Option<Secret>, SecretError> {
    match named {
        Some(named) => secrets.secret_boxed(&named.secret).await.map(Some),
        None => Ok(None),
    }
}

/// A destination that takes commands, of any kind.
#[derive(Clone)]
pub struct Interacted(Arc<dyn Interacts>);

impl Interacted {
    /// `interacting`, taking commands.
    #[must_use]
    pub fn new(interacting: impl Interacts) -> Self {
        Self(Arc::new(interacting))
    }

    /// It as its own kind, where it is a `T`.
    #[must_use]
    pub fn of<T: Interacts>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref()
    }
}

impl Deref for Interacted {
    type Target = dyn Interacts;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl fmt::Debug for Interacted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for Interacted {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(other.0.as_any())
    }
}

impl Eq for Interacted {}

/// What a kind keeps of where an answer goes: compared and shown as itself.
trait Kept: fmt::Debug + Send + Sync + Any {
    fn as_any(&self) -> &dyn Any;

    fn same(&self, other: &dyn Any) -> bool;
}

impl<T: fmt::Debug + PartialEq + Send + Sync + 'static> Kept for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        other.downcast_ref::<T>() == Some(self)
    }
}

/// Where an answer to a command goes, as the command's kind writes it.
#[derive(Clone)]
pub struct Replying(Arc<dyn Kept>);

impl Replying {
    /// `replying`, as a kind writes where its answers go.
    #[must_use]
    pub fn new<T: fmt::Debug + PartialEq + Send + Sync + 'static>(replying: T) -> Self {
        Self(Arc::new(replying))
    }

    /// Where the answer goes, as its own kind writes it, where it is a `T`.
    #[must_use]
    pub fn of<T: 'static>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref()
    }
}

impl fmt::Debug for Replying {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for Replying {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(other.0.as_any())
    }
}

impl Eq for Replying {}

#[cfg(test)]
mod tests;
