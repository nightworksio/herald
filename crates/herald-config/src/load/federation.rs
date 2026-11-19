//! `[federation]`: this herald among other instances, and the keys its
//! ActivityPub destinations take from it where they leave them out.

use herald_core::follows::followed_by;
use herald_core::{Duration, Size};
use herald_kinds::ConfiguredStore;
use herald_peers::multibase::ed25519_key;
use jiff::SignedDuration;
use toml::de::{DeTable, DeValue};

use super::{Loader, Value};
use crate::Peer;
use crate::config::Federation;
use crate::file::FederationFile;

impl Loader<'_> {
    /// `[federation]` read, and its table as written, which destinations
    /// take keys from.
    pub(super) fn federation<'i>(
        &mut self,
        value: Option<Value<'i>>,
        store: Option<&ConfiguredStore>,
    ) -> (Federation, DeTable<'i>) {
        let Some(value) = value else {
            return (Federation::default(), DeTable::default());
        };
        let span = value.span();
        let written = match value.get_ref() {
            DeValue::Table(table) => table.clone(),
            _ => DeTable::default(),
        };
        let Some(file) = self.typed::<FederationFile>(value, "federation") else {
            return (Federation::default(), written);
        };
        if let Some(domain) = file
            .domain
            .as_deref()
            .filter(|domain| !herald_core::domain(domain))
        {
            self.error(
                &span,
                "federation.domain",
                format!(
                    "\"{domain}\" is not a domain; give the host herald serves on, such as \"herald.example\", with no scheme, port or path"
                ),
            );
        }
        if file.domain.is_none() && !file.keys.is_empty() {
            self.error(
                &span,
                "federation.domain",
                "give the domain the keys are published on: domain = \"herald.example\"",
            );
        }
        let [refresh, signed_for, keep_audit, grants_ending] = self.lastings(&span, &file);
        let ([stranger_rate, replay_rate], refused_nonces) = self.rates(&span, &file);
        let fields = file.fields.map_or(Federation::FIELDS, |fields| fields.0);
        if fields > Federation::FIELDS_ALLOWED {
            self.error(
                &span,
                "federation.fields",
                format!(
                    "fields is at most {}; lower it",
                    Size::written(Federation::FIELDS_ALLOWED)
                ),
            );
        }
        let mut revoked = Vec::with_capacity(file.revoked.len());
        for written in &file.revoked {
            match ed25519_key(written) {
                Some(key) => revoked.push(key),
                None => self.error(
                    &span,
                    "federation.revoked",
                    format!("\"{written}\" is not an Ed25519 Multikey; give each revoked key as `herald peer revoke` writes it, such as \"z6Mk…\""),
                ),
            }
        }
        let pages = file.pages.as_ref().and_then(|pages| {
            self.pages(
                &span,
                pages,
                (file.domain.as_deref(), !file.keys.is_empty()),
                store,
            )
        });
        let block = file.block.unwrap_or_default();
        let allow = file.allow.unwrap_or_default();
        if let Err(refused) = followed_by(&block, &allow, file.followers) {
            self.error(&span, "federation", refused);
        }
        (
            Federation {
                domain: file.domain,
                keys: file.keys,
                refresh,
                signed_for,
                fields,
                revoked,
                keep_audit,
                grants_ending,
                stranger_rate,
                replay_rate,
                refused_nonces,
                pages,
            },
            written,
        )
    }

    /// `file`'s `stranger_rate` and `replay_rate`, and its `refused_nonces`,
    /// each within its bound.
    fn rates(
        &mut self,
        span: &std::ops::Range<usize>,
        file: &FederationFile,
    ) -> ([herald_core::Rate; 2], u32) {
        let rates = [
            ("stranger_rate", file.stranger_rate),
            ("replay_rate", file.replay_rate),
        ]
        .map(|(key, rate)| {
            let rate = rate.unwrap_or(Peer::RATE);
            if rate.exceeds(&Peer::RATE_ALLOWED) {
                self.error(
                    span,
                    &format!("federation.{key}"),
                    format!("{key} is at most \"{}\"; lower it", Peer::RATE_ALLOWED),
                );
            }
            rate
        });
        let refused_nonces = file.refused_nonces.unwrap_or(Federation::REFUSED_NONCES);
        if refused_nonces > Federation::REFUSED_NONCES_ALLOWED {
            self.error(
                span,
                "federation.refused_nonces",
                format!(
                    "refused_nonces is at most {}; lower it",
                    Federation::REFUSED_NONCES_ALLOWED
                ),
            );
        }
        (rates, refused_nonces)
    }

    /// How long `file`'s `refresh`, `signed_for`, `keep_audit` and
    /// `grants_ending` last, each within its bounds.
    fn lastings(
        &mut self,
        span: &std::ops::Range<usize>,
        file: &FederationFile,
    ) -> [SignedDuration; 4] {
        let span_at = |key: &str| (span.clone(), key.to_owned());
        let refresh = self.lasting(
            span_at("refresh"),
            file.refresh,
            (Federation::REFRESH, Federation::REFRESH_ALLOWED, "7d"),
        );
        let signed_for = self.lasting(
            span_at("signed_for"),
            file.signed_for,
            (
                Federation::SIGNED_FOR,
                Federation::SIGNED_FOR_ALLOWED,
                "20m",
            ),
        );
        let keep_audit = self.lasting(
            span_at("keep_audit"),
            file.keep_audit,
            (
                Federation::KEEP_AUDIT,
                Federation::KEEP_AUDIT_ALLOWED,
                "365d",
            ),
        );
        let grants_ending = self.lasting(
            span_at("grants_ending"),
            file.grants_ending,
            (
                Federation::GRANTS_ENDING,
                Federation::GRANTS_ENDING_ALLOWED,
                "90d",
            ),
        );
        [refresh, signed_for, keep_audit, grants_ending]
    }

    /// How long `[federation]`'s `key` lasts, as `written` or else its
    /// default, refused past its longest, written as `most`.
    pub(super) fn lasting(
        &mut self,
        (span, key): (std::ops::Range<usize>, String),
        written: Option<Duration>,
        (default, longest, most): (SignedDuration, SignedDuration, &str),
    ) -> SignedDuration {
        let lasting = written.map_or(default, |written| written.0);
        if lasting > longest {
            self.error(
                &span,
                &format!("federation.{key}"),
                format!("{key} is at most \"{most}\"; shorten it"),
            );
        }
        lasting
    }
}
