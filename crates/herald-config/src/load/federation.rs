//! `[federation]`: this herald among other instances, and the keys its
//! ActivityPub destinations take from it where they leave them out.

use herald_core::follows::followed_by;
use toml::de::{DeTable, DeValue};

use super::{Loader, Value};
use crate::config::Federation;
use crate::file::FederationFile;

impl Loader<'_> {
    /// `[federation]` read, and its table as written, which destinations
    /// take keys from.
    pub(super) fn federation<'i>(&mut self, value: Option<Value<'i>>) -> (Federation, DeTable<'i>) {
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
        let refresh = file
            .refresh
            .map_or(Federation::REFRESH, |refresh| refresh.0);
        if refresh > Federation::REFRESH_ALLOWED {
            self.error(
                &span,
                "federation.refresh",
                "refresh is at most \"7d\"; shorten it",
            );
        }
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
            },
            written,
        )
    }
}
