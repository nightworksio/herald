//! The ID tokens apps take as credentials (ADR-0064): each entry's
//! issuer, audience, claims and keys checked, an issuer's keys read alike
//! by every entry naming it, and the names apps stand for checked against
//! `[peers]` and one another.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::{Address, ClaimPattern, KeySet, TokenAlgorithm, kind_name};
use herald_kinds::ConfiguredStore;
use jiff::SignedDuration;
use serde_json::{Value, json};

use super::Loader;
use super::apps::Named;
use crate::config::{Federation, Server};
use crate::file::OidcFile;
use crate::oidc::{Oidc, Taking, identified};
use crate::peers::Peer;

/// Where github.com is, whose default audience names an owner rather than
/// one herald.
const GITHUB: &str = "https://github.com";

/// What starts the host a GHE.com enterprise issues its ID tokens from.
const GHE_COM_ISSUER: &str = "token.actions.";

/// What ends a GHE.com enterprise's host.
const GHE_COM: &str = ".ghe.com";

/// What ends the issuer of GitHub Enterprise Server's ID tokens, after its
/// host.
const GHES_ISSUER: &str = "/_services/token";

/// An entry as written, its audience not yet given its default.
#[derive(Debug)]
pub(super) struct Written {
    oidc: Oidc,
    audience: Option<String>,
}

/// What an app stands for when a run holding one of its ID tokens pushes.
#[derive(Debug, Default)]
pub(super) struct Standing<'a> {
    /// Each name an app stands for, and the app.
    pub(super) names: BTreeMap<&'a str, &'a str>,
}

/// The audience an entry naming none names: `server`'s URL, else
/// `https://` and `federation`'s domain.
pub(super) fn audience(server: &Server, federation: &Federation) -> Option<String> {
    server.url.clone().or_else(|| {
        federation
            .domain
            .as_ref()
            .map(|domain| format!("https://{domain}"))
    })
}

impl Loader<'_> {
    /// Whether the apps `listed` read each issuer's keys alike and stand
    /// for names of their own, each grant of `sources` naming a peer of
    /// `peers` or such a name; and how many instances, peers and names,
    /// may make what a herald source takes.
    pub(super) fn credentials(
        &mut self,
        listed: &BTreeMap<String, Named>,
        (peers, federation): (&BTreeMap<String, Peer>, &Federation),
        sources: (
            &BTreeMap<herald_core::SourceName, herald_kinds::Source>,
            &BTreeMap<herald_core::SourceName, Range<usize>>,
        ),
    ) -> usize {
        self.issuers(
            listed
                .iter()
                .map(|(name, app)| (name.as_str(), &app.span, app.oidc.as_slice())),
        );
        let standing = self.standing(
            listed.iter().map(|(name, app)| {
                let oidc = !app.oidc.is_empty();
                (name.as_str(), &app.span, app.peers.as_slice(), oidc)
            }),
            peers,
            federation,
        );
        self.granted(sources, peers, &standing);
        peers.len().saturating_add(standing.names.len())
    }

    /// The entries `written` at `path`, each checked, or none where one is
    /// not sound; an entry whose keys are read needs a store that keeps
    /// them.
    pub(super) fn oidc(
        &mut self,
        (span, path): (&Range<usize>, &str),
        written: Vec<OidcFile>,
        (store, does): (Option<&ConfiguredStore>, &BTreeSet<Taking>),
    ) -> Option<Vec<Written>> {
        let mut entries = Vec::new();
        let mut sound = true;
        for (index, file) in written.into_iter().enumerate() {
            let at = format!("{path}.oidc[{index}]");
            match self.entry((span, &at), file, (store, does)) {
                Some(entry) => entries.push(entry),
                None => sound = false,
            }
        }
        sound.then_some(entries)
    }

    /// The entry `file` at `at` of an app that does `does`, where every
    /// key of it is sound.
    fn entry(
        &mut self,
        (span, at): (&Range<usize>, &str),
        file: OidcFile,
        (store, does): (Option<&ConfiguredStore>, &BTreeSet<Taking>),
    ) -> Option<Written> {
        let errors = self.errors.len();
        if !https(&file.issuer) {
            self.error(
                span,
                &format!("{at}.issuer"),
                format!(
                    "\"{}\" is not an issuer herald reads; give it as its tokens' iss claim names it, https:// with no user, query or fragment, such as \"https://token.actions.githubusercontent.com\"",
                    file.issuer
                ),
            );
        }
        if let Some(audience) = &file.audience {
            self.audience((span, at), audience, &file.issuer);
        }
        let (keys_for, keys_retry, lifetime, skew) = self.lengths((span, at), &file);
        let claims = self.claims((span, at), file.claims);
        if !identified(&claims) {
            self.error(
                span,
                &format!("{at}.claims"),
                format!(
                    "the entry binds no claim naming who a run is to one id or one owner, and this would accept a token of any repository at this issuer, as any run may ask for any audience and a GitHub repository shapes its own sub; bind {} with no wildcard, best such as repository_id = \"123456\", or {} with none before its first /, or sub beginning {} and an owner with no wildcard, then /",
                    Oidc::IMMUTABLE.join(", "),
                    Oidc::NAMED.join(", "),
                    Oidc::SUBJECTS.join(" or ")
                ),
            );
        }
        let algorithms = file
            .algorithms
            .unwrap_or_else(|| TokenAlgorithm::ALL.to_vec());
        if algorithms.is_empty() {
            self.error(
                span,
                &format!("{at}.algorithms"),
                format!(
                    "no algorithm is allowed, so no token is taken; allow one, such as [\"{}\"], or leave algorithms out",
                    TokenAlgorithm::Rs256.name()
                ),
            );
        }
        if let Some(jwks) = file.jwks.as_deref().filter(|jwks| !https(jwks)) {
            self.error(
                span,
                &format!("{at}.jwks"),
                format!(
                    "\"{jwks}\" is not where herald reads keys; give an https:// address with no user, query or fragment"
                ),
            );
        }
        let keys = file
            .keys
            .map(|keys| self.pinned((span, at), &keys, file.jwks.is_some()));
        if keys.is_none()
            && let Some(store) = store.filter(|store| !store.keeps_tokens())
        {
            self.error(
                span,
                at,
                format!(
                    "the {} store keeps no records, so it keeps no issuer's keys between requests; choose a store that keeps records, or pin the issuer's keys",
                    store.kind()
                ),
            );
        }
        let takes = self.takes((span, at), file.takes, does);
        (self.errors.len() == errors).then(|| Written {
            oidc: Oidc {
                issuer: file.issuer,
                audience: String::new(),
                claims,
                algorithms,
                keys_for,
                keys_retry,
                jwks: file.jwks,
                keys: keys.flatten(),
                takes,
                lifetime,
                skew,
            },
            audience: file.audience,
        })
    }

    /// How long the entry `file` at `at` uses its issuer's keys, waits to
    /// fetch them again, lets a token live and lets its times stand from
    /// the clock, each within its bounds.
    fn lengths(
        &mut self,
        (span, at): (&Range<usize>, &str),
        file: &OidcFile,
    ) -> (
        SignedDuration,
        SignedDuration,
        SignedDuration,
        SignedDuration,
    ) {
        let keys_for = file.keys_for.map_or(Oidc::KEYS_FOR, |keys_for| keys_for.0);
        if keys_for > Oidc::KEYS_FOR_ALLOWED {
            self.error(
                span,
                &format!("{at}.keys_for"),
                "keys_for is at most \"1d\"; shorten it",
            );
        }
        let keys_retry = file
            .keys_retry
            .map_or(Oidc::KEYS_RETRY, |keys_retry| keys_retry.0);
        if keys_retry < Oidc::KEYS_RETRY_LEAST || keys_retry > keys_for {
            self.error(
                span,
                &format!("{at}.keys_retry"),
                "keys_retry is from \"10s\" to keys_for; give it within them",
            );
        }
        let lifetime = file.lifetime.map_or(Oidc::LIFETIME, |lifetime| lifetime.0);
        if lifetime > Oidc::LIFETIME_ALLOWED {
            self.error(
                span,
                &format!("{at}.lifetime"),
                "lifetime is at most \"1d\"; shorten it",
            );
        }
        let skew = file.skew.map_or(Oidc::SKEW, |skew| skew.0);
        if skew > Oidc::SKEW_ALLOWED {
            self.error(
                span,
                &format!("{at}.skew"),
                "skew is at most \"5m\"; shorten it",
            );
        }
        (keys_for, keys_retry, lifetime, skew)
    }

    /// What an entry at `at` of an app that does `does` lets its tokens
    /// do: `written`, where the app does it all, else what the app does
    /// where it does one thing.
    fn takes(
        &mut self,
        (span, at): (&Range<usize>, &str),
        written: Option<Vec<Taking>>,
        does: &BTreeSet<Taking>,
    ) -> BTreeSet<Taking> {
        let why = match written {
            None if does.len() > 1 => {
                "the app announces and stands for peers; say what this entry's tokens may do, as takes = [\"announce\"], [\"push\"] or both"
            }
            None => return does.clone(),
            Some(written) => {
                let takes: BTreeSet<Taking> = written.into_iter().collect();
                if takes.is_empty() {
                    "the entry's tokens may do nothing, so none is taken; give takes = [\"announce\"], [\"push\"] or both, or leave it out"
                } else if !takes.is_subset(does) {
                    "the entry's tokens may do what the app does not; an app announces only where it names projects, and pushes only where it stands for peers"
                } else {
                    return takes;
                }
            }
        };
        self.error(span, &format!("{at}.takes"), why);
        BTreeSet::new()
    }

    /// Whether `audience`, written at `at` beside `issuer`, names one
    /// herald alone: never an issuer, nor the default audience of a GitHub
    /// that issues tokens, which names an owner.
    fn audience(&mut self, (span, at): (&Range<usize>, &str), audience: &str, issuer: &str) {
        let why = if audience.is_empty() {
            "the audience is empty; give this herald's own URL, or leave audience out"
        } else if audience == issuer {
            "the audience is the issuer; give this herald's own URL, or leave audience out"
        } else if owners(issuer).any(|forge| owner_of(audience, &forge)) {
            "the audience is GitHub's default, which names the owner, so every herald of the owner would take its tokens; give this herald's own URL, or leave audience out"
        } else {
            return;
        };
        self.error(span, &format!("{at}.audience"), why);
    }

    /// Each claim of `written` at `at`, its pattern read; a claim's name
    /// and pattern are never empty.
    fn claims(
        &mut self,
        (span, at): (&Range<usize>, &str),
        written: BTreeMap<String, String>,
    ) -> BTreeMap<String, ClaimPattern> {
        let mut claims = BTreeMap::new();
        for (name, pattern) in written {
            if name.is_empty() || pattern.is_empty() {
                self.error(
                    span,
                    &format!("{at}.claims"),
                    "a claim is named, and its pattern says what it must be, such as repository_id = \"123456\"",
                );
                continue;
            }
            claims.insert(name, ClaimPattern::new(pattern));
        }
        claims
    }

    /// The keys `written` at `at` pins, where it pins at least one RSA
    /// signing key and names no address they would be read from too.
    fn pinned(
        &mut self,
        (span, at): (&Range<usize>, &str),
        written: &[Value],
        read: bool,
    ) -> Option<KeySet> {
        if read {
            self.error(
                span,
                &format!("{at}.jwks"),
                "pinned keys are never read; leave jwks out, or the keys",
            );
            return None;
        }
        let body = serde_json::to_vec(&json!({ "keys": written })).unwrap_or_default();
        match KeySet::read(&body) {
            Ok(keys) if !keys.is_empty() => Some(keys),
            _ => {
                self.error(
                    span,
                    &format!("{at}.keys"),
                    "no key pinned is an RSA signing key; pin the issuer's keys as its JSON Web Key Set lists them, each with kty = \"RSA\", n and e",
                );
                None
            }
        }
    }

    /// The entries `written` for the app `name`, each naming `default`
    /// where it names no audience of its own.
    pub(super) fn audienced(
        &mut self,
        (span, name): (&Range<usize>, &str),
        written: Vec<Written>,
        (default, peers): (Option<&str>, &BTreeMap<String, Peer>),
    ) -> Option<Vec<Oidc>> {
        let mut entries = Vec::new();
        for (index, entry) in written.into_iter().enumerate() {
            let at = format!("apps.{name}.oidc[{index}].audience");
            let Some(audience) = entry.audience.or_else(|| default.map(str::to_owned)) else {
                self.error(
                    span,
                    &at,
                    "a token must name this herald as its audience, and herald knows no URL of its own; give audience, [server] url or [federation] domain",
                );
                return None;
            };
            let bare = audience.trim_end_matches('/');
            if let Some(peer) = peers
                .iter()
                .find(|(_, peer)| peer.url.trim_end_matches('/') == bare)
                .map(|(peer, _)| peer)
            {
                self.error(
                    span,
                    &at,
                    format!("the audience is the URL of peers.{peer}, another herald, which could present a token meant for it here; give this herald's own URL"),
                );
                return None;
            }
            entries.push(Oidc {
                audience,
                ..entry.oidc
            });
        }
        Some(entries)
    }

    /// Whether every entry of `apps` naming one issuer reads its keys
    /// alike: from the same address, pinned the same, for as long.
    fn issuers<'a>(
        &mut self,
        apps: impl Iterator<Item = (&'a str, &'a Range<usize>, &'a [Written])>,
    ) {
        let mut first: BTreeMap<&str, (&str, &Oidc)> = BTreeMap::new();
        for (name, span, entries) in apps {
            for (index, entry) in entries.iter().enumerate() {
                let oidc = &entry.oidc;
                let Some((before, kept)) = first.get(oidc.issuer.as_str()) else {
                    first.insert(&oidc.issuer, (name, oidc));
                    continue;
                };
                if (&kept.jwks, &kept.keys, kept.keys_for, kept.keys_retry)
                    != (&oidc.jwks, &oidc.keys, oidc.keys_for, oidc.keys_retry)
                {
                    self.error(
                        span,
                        &format!("apps.{name}.oidc[{index}]"),
                        format!(
                            "apps.{before} reads the keys of {} otherwise; give every entry naming an issuer the same jwks, keys, keys_for and keys_retry",
                            oidc.issuer
                        ),
                    );
                }
            }
        }
    }

    /// Each name an app of `apps` stands for, and the app: a peer's name
    /// that no table of `peers` and no other app has, standing only for
    /// an app that takes ID tokens, on a herald whose `federation` proves
    /// what is pushed in its name.
    fn standing<'a>(
        &mut self,
        apps: impl Iterator<Item = (&'a str, &'a Range<usize>, &'a [String], bool)>,
        peers: &BTreeMap<String, Peer>,
        federation: &Federation,
    ) -> Standing<'a> {
        let mut standing = Standing::default();
        for (app, span, names, oidc) in apps {
            let path = format!("apps.{app}.peers");
            if !names.is_empty() && !oidc {
                self.error(
                    span,
                    &path,
                    "an app stands for peers only where a run proves who it is with an ID token; give the app oidc entries, or leave peers out",
                );
            }
            if !names.is_empty() && (federation.domain.is_none() || federation.keys.is_empty()) {
                self.error(
                    span,
                    &path,
                    "what an app pushes as a peer is proved by [federation]'s newest key on its domain; give domain and keys in [federation]",
                );
            }
            for name in names {
                let why = if !kind_name(name) {
                    format!("\"{name}\" is not a peer's name; give letters, digits, '-' and '_'")
                } else if peers.contains_key(name) {
                    format!(
                        "\"{name}\" is a table of [peers]; give the name an app stands for one no peer has"
                    )
                } else if let Some(other) = standing.names.insert(name, app) {
                    format!("apps.{other} stands for \"{name}\" too; give each name one app")
                } else {
                    continue;
                };
                self.error(span, &path, why);
            }
        }
        standing
    }
}

/// Whether `url` is an `https://` address with a host and no user, query
/// or fragment.
fn https(url: &str) -> bool {
    Address::parse(url).is_some_and(|parsed| parsed.bare_https())
}

/// Each GitHub whose default audience, `<GitHub>/<owner>`, a token of
/// `issuer` may carry: github.com always; the enterprise's own on GHE.com,
/// where `issuer` is `https://token.actions.<enterprise>.ghe.com`; and the
/// server's own on GitHub Enterprise Server, where `issuer` is
/// `https://<host>/_services/token`.
fn owners(issuer: &str) -> impl Iterator<Item = String> {
    let parsed = Address::parse(issuer);
    let ghe_com = parsed
        .as_ref()
        .and_then(|parsed| parsed.host().strip_prefix(GHE_COM_ISSUER))
        .filter(|rest| rest.ends_with(GHE_COM))
        .map(|enterprise| format!("https://{enterprise}"));
    let server = parsed
        .as_ref()
        .filter(|parsed| parsed.path().trim_end_matches('/') == GHES_ISSUER)
        .map(Address::origin);
    std::iter::once(GITHUB.to_owned())
        .chain(ghe_com)
        .chain(server)
}

/// Whether `audience` is `forge`, then `/` and one name: an owner there.
fn owner_of(audience: &str, forge: &str) -> bool {
    audience
        .strip_prefix(forge)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|owner| !owner.trim_end_matches('/').contains('/'))
}
