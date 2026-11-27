use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use aho_corasick::{AhoCorasick, MatchKind};
use jiff::{SignedDuration, Timestamp};

use crate::{Clock, REDACTED, Request, Secret, SecretError, SecretText, Secrets, Segment};

/// Every secret value herald resolves or sends, shown as `«secret:NAME»`
/// wherever herald writes: what a secret's resolving or a request's
/// sending makes known, a token minted at run time among them, is known
/// to every clone (ADR-0007, ADR-0089).
#[derive(Debug, Clone)]
pub struct Scrubber(Arc<Mutex<Hidden>>);

/// How long a value is held: for as long as the scrubber lives, or until
/// a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Until {
    /// Until the time.
    At(Timestamp),
    /// For as long as the scrubber lives.
    Ever,
}

/// One value held, by the name it is shown as.
#[derive(Debug)]
struct Held {
    secret: Secret,
    until: Until,
}

/// What the scrubber matches each text against, built from the values held
/// whenever they change.
#[derive(Debug)]
struct Matcher {
    patterns: AhoCorasick,
    shown: Vec<String>,
}

struct Hidden {
    shortest: NonZeroUsize,
    many: NonZeroUsize,
    clock: Option<Arc<dyn Clock + Send>>,
    held: HashMap<String, Held>,
    next: Option<Timestamp>,
    matcher: Option<Matcher>,
    crowded: Crowded,
}

/// Whether more values are held than are many, and whether that was said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Crowded {
    /// No more are held.
    Not,
    /// More are held, not yet said.
    Unsaid,
    /// More are held, and that was said once.
    Said,
}

impl std::fmt::Debug for Hidden {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut held: Vec<_> = self
            .held
            .values()
            .map(|held| (&held.secret, held.until))
            .collect();
        held.sort_by_key(|(secret, _)| secret.name().to_owned());
        f.debug_struct("Hidden")
            .field("shortest", &self.shortest)
            .field("many", &self.many)
            .field("timed", &self.clock.is_some())
            .field("held", &held)
            .finish_non_exhaustive()
    }
}

impl Hidden {
    fn now(&self) -> Option<Timestamp> {
        self.clock.as_ref().map(|clock| clock.now())
    }

    /// `secret` held until `until`, or longer where it is held longer, and
    /// shown by the name of the one held longest.
    fn hold(&mut self, secret: &Secret, until: Until) {
        let value = secret.expose();
        if value.trim().is_empty() {
            return;
        }
        match self.held.get_mut(value) {
            Some(held) if held.until >= until => return,
            Some(held) => {
                *held = Held {
                    secret: secret.clone(),
                    until,
                };
                self.matcher = None;
            }
            None => {
                self.held.insert(
                    value.to_owned(),
                    Held {
                        secret: secret.clone(),
                        until,
                    },
                );
                self.matcher = None;
            }
        }
        if let Until::At(at) = until {
            self.next = Some(self.next.map_or(at, |next| next.min(at)));
        }
        if self.held.len() > self.many.get() && self.crowded == Crowded::Not {
            self.crowded = Crowded::Unsaid;
        }
    }

    /// Every value held until a time already past forgotten, where the
    /// scrubber has a clock and one is: a value is held through its time.
    fn prune(&mut self) {
        let (Some(now), Some(next)) = (self.now(), self.next) else {
            return;
        };
        if next >= now {
            return;
        }
        let before = self.held.len();
        self.held
            .retain(|_, held| !matches!(held.until, Until::At(at) if at < now));
        if self.held.len() != before {
            self.matcher = None;
        }
        self.next = self
            .held
            .values()
            .filter_map(|held| match held.until {
                Until::At(at) => Some(at),
                Until::Ever => None,
            })
            .min();
        if self.held.len() <= self.many.get() {
            self.crowded = Crowded::Not;
        }
    }

    /// What matches every value held of at least the shortest length,
    /// built where the values changed since it was last built.
    fn matcher(&mut self) -> Option<&Matcher> {
        if self.matcher.is_none() {
            let shortest = self.shortest.get();
            let (values, shown): (Vec<&str>, Vec<String>) = self
                .held
                .iter()
                .filter(|(value, _)| value.chars().count() >= shortest)
                .map(|(value, held)| (value.as_str(), held.secret.redacted()))
                .unzip();
            self.matcher = AhoCorasick::builder()
                .match_kind(MatchKind::LeftmostLongest)
                .build(values)
                .ok()
                .map(|patterns| Matcher { patterns, shown });
        }
        self.matcher.as_ref()
    }
}

impl Scrubber {
    /// How long after its expiry a minted value is still held, for a clock
    /// that runs behind the one that minted it.
    pub const MARGIN: SignedDuration = SignedDuration::from_mins(10);
    /// How long a value of no known expiry, known only by its sending, is
    /// held after it was last sent.
    pub const RETAINED: SignedDuration = SignedDuration::from_hours(24);
    /// How many values held are many where a config says nothing: past
    /// it, every value is still held and scrubbed, and the log says so once
    /// (ADR-0089).
    pub const CROWDED: NonZeroUsize = NonZeroUsize::new(4096).unwrap();

    /// A scrubber that knows no secret yet, leaves every value shorter
    /// than `shortest` characters as it is, forgets no value it knows, and
    /// takes [`Scrubber::CROWDED`] values for many.
    #[must_use]
    pub fn new(shortest: NonZeroUsize) -> Self {
        Self(Arc::new(Mutex::new(Hidden {
            shortest,
            many: Self::CROWDED,
            clock: None,
            held: HashMap::new(),
            next: None,
            matcher: None,
            crowded: Crowded::Not,
        })))
    }

    /// The same scrubber, forgetting by `clock` a value known only by its
    /// sending once it can no longer be valid: [`Scrubber::MARGIN`] after
    /// its expiry where it was minted with one, and else
    /// [`Scrubber::RETAINED`] after it was last sent.
    #[must_use]
    pub fn timed(self, clock: impl Clock + Send + 'static) -> Self {
        self.hidden().clock = Some(Arc::new(clock));
        self
    }

    /// The same scrubber, taking `many` values for many: past it, every
    /// value is still held and scrubbed, and the log says so once.
    #[must_use]
    pub fn crowded_past(self, many: NonZeroUsize) -> Self {
        self.hidden().many = many;
        self
    }

    /// How many values it takes for many.
    #[must_use]
    pub fn many(&self) -> NonZeroUsize {
        self.hidden().many
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
            hidden.hold(&Secret::new(secret.name(), part), Until::Ever);
        }
    }

    /// Every secret `request` carries in its URL or a header known until it
    /// can no longer be valid.
    pub fn sent(&self, request: &Request) {
        let texts =
            std::iter::once(&request.url).chain(request.headers.iter().map(|(_, text)| text));
        let mut hidden = self.hidden();
        hidden.prune();
        let now = hidden.now();
        for secret in texts
            .flat_map(SecretText::segments)
            .filter_map(|segment| match segment {
                Segment::Secret(secret) => Some(secret),
                Segment::Plain(_) => None,
            })
        {
            let until = match (secret.expires(), now) {
                (Some(expires), Some(_)) => Until::At(expires + Self::MARGIN),
                (None, Some(now)) => Until::At(now + Self::RETAINED),
                (_, None) => Until::Ever,
            };
            hidden.hold(secret, until);
        }
    }

    /// How many values are held, where more are held than it takes for
    /// many, told once each time it becomes so.
    #[must_use]
    pub fn crowded(&self) -> Option<usize> {
        let mut hidden = self.hidden();
        (hidden.crowded == Crowded::Unsaid).then(|| {
            hidden.crowded = Crowded::Said;
            hidden.held.len()
        })
    }

    /// `text` with each value it holds of at least its shortest length
    /// written as `«secret:NAME»`, the longest matched first, in one pass;
    /// the whole text withheld where no matcher could be built of them.
    #[must_use]
    pub fn scrubbed(&self, text: &str) -> String {
        let mut hidden = self.hidden();
        hidden.prune();
        let unscrubbed = format!("{REDACTED}unscrubbed»");
        hidden.matcher().map_or(unscrubbed, |matcher| {
            matcher.patterns.replace_all(text, &matcher.shown)
        })
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
