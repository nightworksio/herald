use std::collections::VecDeque;
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::{Request, Secret, SecretError, SecretText, Secrets, Segment};

/// Every secret value herald resolves or sends, shown as `«secret:NAME»`
/// wherever herald writes: what a secret's resolving or a request's
/// sending makes known, a token minted at run time among them, is known
/// to every clone (ADR-0007, ADR-0089).
#[derive(Debug, Clone)]
pub struct Scrubber(Arc<Mutex<Hidden>>);

#[derive(Debug)]
struct Hidden {
    shortest: NonZeroUsize,
    kept: Vec<Secret>,
    sent: VecDeque<Secret>,
}

impl Hidden {
    fn holds(&self, value: &str) -> bool {
        self.kept
            .iter()
            .chain(&self.sent)
            .any(|secret| secret.expose() == value)
    }
}

impl Scrubber {
    /// How many values known only by their sending are held, the one sent
    /// longest ago forgotten first, so a server minting a token for each
    /// request holds a bounded set.
    pub const SENT: usize = 1024;

    /// A scrubber that knows no secret yet and leaves every value shorter
    /// than `shortest` characters as it is.
    #[must_use]
    pub fn new(shortest: NonZeroUsize) -> Self {
        Self(Arc::new(Mutex::new(Hidden {
            shortest,
            kept: Vec::new(),
            sent: VecDeque::new(),
        })))
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
            if !part.trim().is_empty() && !hidden.holds(part) {
                hidden.kept.push(Secret::new(secret.name(), part));
            }
        }
    }

    /// Every secret `request` carries in its URL or a header known, as
    /// [`Scrubber::SENT`] bounds.
    pub fn sent(&self, request: &Request) {
        let texts =
            std::iter::once(&request.url).chain(request.headers.iter().map(|(_, text)| text));
        let mut hidden = self.hidden();
        for secret in texts
            .flat_map(SecretText::segments)
            .filter_map(|segment| match segment {
                Segment::Secret(secret) => Some(secret),
                Segment::Plain(_) => None,
            })
        {
            if secret.expose().trim().is_empty() || hidden.holds(secret.expose()) {
                continue;
            }
            if hidden.sent.len() == Self::SENT {
                hidden.sent.pop_front();
            }
            hidden.sent.push_back(secret.clone());
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
            .chain(&hidden.sent)
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
