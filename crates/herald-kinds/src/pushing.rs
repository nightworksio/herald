use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use herald_core::Waits;
use herald_ports::Secret;

/// Where a destination that pushes to a peer pushes, as its options name
/// it: the peer of `[peers]` and the herald source of the peer's taking
/// the pushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pushing<'a> {
    /// The peer of `[peers]`.
    pub peer: &'a str,
    /// The peer's source whose inbox takes the pushes.
    pub inbox: &'a str,
    /// Whether a CI run's ID token stands for the sender, in place of the
    /// instance's key (ADR-0064).
    pub id_token: bool,
    /// What the run's ID token must be meant for, where the push names it.
    pub audience: Option<&'a str>,
}

/// What proves who pushes.
#[derive(Debug, Clone, Copy)]
pub enum Proving<'a> {
    /// The instance's newest key, which proves each envelope and signs
    /// each request.
    Key(&'a Secret),
    /// A CI run's ID token, a fresh one for each request where the CI
    /// gives one, presented only where it is meant for `audience`; the
    /// peer proves each envelope itself (ADR-0064).
    IdToken {
        /// Where the tokens come from.
        tokens: IdTokens<'a>,
        /// What a token's `aud` must name.
        audience: &'a str,
    },
}

/// Where a run's ID tokens come from.
#[derive(Debug, Clone, Copy)]
pub enum IdTokens<'a> {
    /// Asked of the CI for each push, at `url` with `token`, as GitHub
    /// Actions gives a job with the `id-token: write` permission.
    Requested {
        /// Where a token is asked for, before its audience.
        url: &'a Secret,
        /// What asks.
        token: &'a Secret,
    },
    /// Read from the run's variables, each presented for one push in turn.
    Read {
        /// The tokens.
        tokens: &'a [Secret],
        /// How many were presented.
        presented: &'a Presented,
    },
}

/// Which of a run's ID tokens read from its variables its pushes
/// presented, by the variable each was read from, shared by every
/// destination of one config, so no two present one token. Two are alike
/// whatever they hold.
#[derive(Debug, Clone, Default)]
pub struct Presented(Arc<Mutex<BTreeSet<String>>>);

impl Presented {
    /// The first of `tokens` no push presented, now held as presented.
    #[must_use]
    pub fn next(&self, tokens: &[Secret]) -> Option<Secret> {
        let mut presented = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let token = tokens
            .iter()
            .find(|token| !presented.contains(token.name()))?;
        presented.insert(token.name().to_owned());
        Some(token.clone())
    }

    /// `token` held as never presented, as its push sent nothing.
    pub fn unspent(&self, token: &Secret) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(token.name());
    }
}

impl PartialEq for Presented {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Presented {}

/// The peer a destination pushes to, as `[peers]` has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peered {
    /// Where the peer is served: `https://`, its domain and the path its
    /// endpoints are under.
    pub url: String,
    /// Whether private events may go to it.
    pub private: bool,
    /// Which of the run's ID tokens read from its variables were
    /// presented, shared by every destination of the config.
    pub presented: Presented,
}

/// A route's push to a peer, as the flows push through it: where the
/// envelopes go, the most bytes one takes, whether private events may go,
/// how long it waits, and what proves who pushes.
#[derive(Debug, Clone, Copy)]
pub struct Pushed<'a> {
    /// The peer of `[peers]`.
    pub peer: &'a str,
    /// The URL of the peer's inbox.
    pub inbox: &'a str,
    /// The most bytes one envelope takes, its proof counted.
    pub envelope: usize,
    /// Whether private events may go to the peer.
    pub private: bool,
    /// How long it waits where the peer's inbox names no wait.
    pub waits: Waits,
    /// What proves who pushes.
    pub proving: Proving<'a>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_token_is_presented_once_across_clones_unless_it_was_unspent() {
        let presented = Presented::default();
        let shared = presented.clone();
        let tokens = [Secret::new("FIRST", "a"), Secret::new("SECOND", "b")];
        assert_eq!(presented.next(&tokens), Some(tokens[0].clone()));
        assert_eq!(shared.next(&tokens[..1]), None);
        assert_eq!(shared.next(&tokens), Some(tokens[1].clone()));
        assert_eq!(presented.next(&tokens), None);
        presented.unspent(&tokens[0]);
        assert_eq!(shared.next(&tokens), Some(tokens[0].clone()));
        assert_eq!(presented, Presented::default());
    }
}
