use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::{SignedDuration, Timestamp};

use crate::{Clock, Request, Secret, SecretError, SecretText, Secrets, Segment};

/// Every secret value herald resolves or sends, shown as `«secret:NAME»`
/// wherever herald writes: what a secret's resolving or a request's
/// sending makes known, a token minted at run time among them, is known
/// to every clone (ADR-0007, ADR-0089).
#[derive(Debug, Clone)]
pub struct Scrubber(Arc<Mutex<Hidden>>);

struct Hidden {
    shortest: NonZeroUsize,
    clock: Option<Arc<dyn Clock + Send>>,
    kept: Vec<Secret>,
    sent: Vec<(Secret, Timestamp)>,
}

impl std::fmt::Debug for Hidden {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hidden")
            .field("shortest", &self.shortest)
            .field("timed", &self.clock.is_some())
            .field("kept", &self.kept)
            .field("sent", &self.sent)
            .finish()
    }
}

impl Hidden {
    fn kept(&self, value: &str) -> bool {
        self.kept.iter().any(|secret| secret.expose() == value)
    }
}

impl Scrubber {
    /// How long a value known only by its sending is held after it was
    /// last sent: longer than any token herald mints stays valid, so a
    /// value is forgotten only once no credential it was can be.
    pub const RETAINED: SignedDuration = SignedDuration::from_hours(24);

    /// A scrubber that knows no secret yet, leaves every value shorter
    /// than `shortest` characters as it is, and forgets no value it knows.
    #[must_use]
    pub fn new(shortest: NonZeroUsize) -> Self {
        Self(Arc::new(Mutex::new(Hidden {
            shortest,
            clock: None,
            kept: Vec::new(),
            sent: Vec::new(),
        })))
    }

    /// The same scrubber, forgetting a value known only by its sending
    /// [`Scrubber::RETAINED`] after `clock` says it was last sent.
    #[must_use]
    pub fn timed(self, clock: impl Clock + Send + 'static) -> Self {
        self.hidden().clock = Some(Arc::new(clock));
        self
    }

    fn hidden(&self) -> MutexGuard<'_, Hidden> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `secret` known for as long as the scrubber lives, and each line of
    /// a value that spans lines on its own as well.
    pub fn keep(&self, secret: &Secret) {
        let mut hidden = self.hidden();
        let value = secret.expose();
        let lines = value.lines().filter(|line| *line != value);
        for part in std::iter::once(value).chain(lines) {
            if part.trim().is_empty() || hidden.kept(part) {
                continue;
            }
            hidden.sent.retain(|(sent, _)| sent.expose() != part);
            hidden.kept.push(Secret::new(secret.name(), part));
        }
    }

    /// Every secret `request` carries in its URL or a header known, until
    /// [`Scrubber::RETAINED`] after it is last sent.
    pub fn sent(&self, request: &Request) {
        let texts =
            std::iter::once(&request.url).chain(request.headers.iter().map(|(_, text)| text));
        let mut hidden = self.hidden();
        let now = hidden
            .clock
            .as_ref()
            .map_or(Timestamp::UNIX_EPOCH, |clock| clock.now());
        if hidden.clock.is_some() {
            hidden
                .sent
                .retain(|(_, last)| now.duration_since(*last) <= Self::RETAINED);
        }
        for secret in texts
            .flat_map(SecretText::segments)
            .filter_map(|segment| match segment {
                Segment::Secret(secret) => Some(secret),
                Segment::Plain(_) => None,
            })
        {
            let value = secret.expose();
            if value.trim().is_empty() || hidden.kept(value) {
                continue;
            }
            match hidden
                .sent
                .iter_mut()
                .find(|(sent, _)| sent.expose() == value)
            {
                Some((_, last)) => *last = now,
                None => hidden.sent.push((secret.clone(), now)),
            }
        }
    }

    /// `text` with each value it knows of at least its shortest length
    /// written as `«secret:NAME»`, the longest matched first, in one pass.
    #[must_use]
    pub fn scrubbed(&self, text: &str) -> String {
        let hidden = self.hidden();
        let shortest = hidden.shortest.get();
        let mut known: Vec<&Secret> = hidden
            .kept
            .iter()
            .chain(hidden.sent.iter().map(|(secret, _)| secret))
            .filter(|secret| secret.expose().chars().count() >= shortest)
            .collect();
        known.sort_by_key(|secret| std::cmp::Reverse(secret.expose().len()));
        let mut scrubbed = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(first) = rest.chars().next() {
            if let Some(secret) = known
                .iter()
                .find(|secret| rest.starts_with(secret.expose()))
            {
                scrubbed.push_str(&secret.redacted());
                rest = rest.get(secret.expose().len()..).unwrap_or_default();
            } else {
                scrubbed.push(first);
                rest = rest.get(first.len_utf8()..).unwrap_or_default();
            }
        }
        scrubbed
    }
}

/// `secrets`, each secret read through them known to `scrubber`.
#[derive(Debug, Clone, Copy)]
pub struct Kept<'a, S> {
    secrets: &'a S,
    scrubber: &'a Scrubber,
}

impl<'a, S> Kept<'a, S> {
    /// `secrets`, each secret read known to `scrubber`.
    #[must_use]
    pub const fn new(secrets: &'a S, scrubber: &'a Scrubber) -> Self {
        Self { secrets, scrubber }
    }
}

impl<S: Secrets> Secrets for Kept<'_, S> {
    fn secret(&self, name: &str) -> impl Future<Output = Result<Secret, SecretError>> + Send {
        let read = self.secrets.secret(name);
        let scrubber = self.scrubber;
        async move {
            let secret = read.await?;
            scrubber.keep(&secret);
            Ok(secret)
        }
    }
}

#[cfg(test)]
mod tests;
