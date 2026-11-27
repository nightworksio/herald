//! The `herald.toml` a first run writes (ADR-0091 decision 5).

use herald_config::Preset;
use herald_core::Signal;
use herald_kinds::{Key, Lands, Written};
use herald_sources_forge::Forge;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value, value};

/// The line that names the schema the config is checked against.
const SCHEMA: &str =
    "#:schema https://raw.githubusercontent.com/nightworksio/herald/main/herald.schema.json";

/// The secret a records store is sealed with.
pub(crate) const STORE_KEY: &str = "HERALD_STORE_KEY";

/// The secret a Forgejo branch store's token is named as where none is
/// given.
pub(crate) const STORE_TOKEN: &str = "HERALD_STORE_TOKEN";

/// Where the records are kept that say what was sent once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kept {
    /// In the destination's own posts.
    Markers,
    /// In the GitHub Actions cache, sealed with [`STORE_KEY`].
    ActionsCache,
    /// On a branch of the Forgejo repository, sealed with [`STORE_KEY`],
    /// pushed with the token the secret names.
    ForgejoBranch {
        /// The secret holding a token that may push.
        token: String,
    },
}

impl Kept {
    /// The store a first run picks on `forge` for a way in that reads its
    /// posts back where `reads_back` (ADR-0091 decision 4): the markers
    /// store; else GitHub's Actions cache and Forgejo's branch; none on
    /// GitLab, where no store keeps herald's records in a repository.
    pub(crate) fn picked(forge: Forge, reads_back: bool) -> Option<Self> {
        match (reads_back, forge) {
            (true, _) => Some(Self::Markers),
            (false, Forge::GitHub) => Some(Self::ActionsCache),
            (false, Forge::Forgejo) => Some(Self::ForgejoBranch {
                token: STORE_TOKEN.to_owned(),
            }),
            (false, Forge::GitLab) => None,
        }
    }

    /// Its `kind`, as `[store]` writes it.
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Markers => "markers",
            Self::ActionsCache => "actions-cache",
            Self::ForgejoBranch { .. } => "forgejo-branch",
        }
    }

    /// Whether it is sealed with [`STORE_KEY`].
    pub(crate) const fn sealed(&self) -> bool {
        !matches!(self, Self::Markers)
    }
}

/// What a first run answered.
pub(crate) struct First<'a> {
    /// The forge the repository is on.
    pub(crate) forge: Forge,
    /// The repository's path there.
    pub(crate) repository: &'a str,
    /// The forge's API, where it is not the public instance's.
    pub(crate) api: Option<&'a str>,
    /// The destination's kind.
    pub(crate) kind: &'a str,
    /// The keys the kind's questions write.
    pub(crate) keys: &'a [Key],
    /// The presets it announces with.
    pub(crate) presets: &'a [&'static Preset],
    /// Where the records are kept.
    pub(crate) store: &'a Kept,
}

impl First<'_> {
    /// The source's name: the repository's own, as a name herald takes.
    pub(crate) fn source(&self) -> String {
        let last = self.repository.rsplit('/').next().unwrap_or_default();
        let named = last
            .to_ascii_lowercase()
            .split(|one: char| !one.is_ascii_alphanumeric())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        if named.is_empty() {
            "repository".to_owned()
        } else {
            named
        }
    }

    /// The config's text.
    pub(crate) fn text(&self) -> String {
        let mut document = DocumentMut::new();
        document.insert("version", value(1));
        document.insert("store", Item::Table(self.stored()));
        let mut destination = Table::new();
        destination.insert("kind", value(self.kind));
        let mut routed = Vec::new();
        for key in self.keys {
            let written = match &key.value {
                Written::Secret(name) => secret(name),
                Written::Text(text) => Value::from(text.as_str()),
            };
            match key.lands {
                Lands::Destination => {
                    destination.insert(key.key, value(written));
                }
                Lands::Route => routed.push((key.key, written)),
            }
        }
        document.insert(
            "destinations",
            Item::Table(implicit(self.kind, destination)),
        );
        let source = self.source();
        let mut sourced = Table::new();
        sourced.insert("kind", value(self.forge.kind()));
        sourced.insert("repository", value(self.repository));
        if let Some(api) = self.api {
            sourced.insert("api", value(api));
        }
        let signals = self.signals();
        if !signals.is_empty() && signals != Signal::DEFAULT.map(Signal::name) {
            sourced.insert("signals", value(Value::Array(Array::from_iter(signals))));
        }
        document.insert("sources", Item::Table(implicit(&source, sourced)));
        if !self.presets.is_empty() {
            let mut presets = Table::new();
            presets.set_implicit(true);
            for preset in self.presets {
                let mut adopted = Table::new();
                adopted.insert("use", value(preset.name));
                adopted.insert("destination", value(self.kind));
                adopted.insert(
                    "sources",
                    value(Value::Array(Array::from_iter([source.as_str()]))),
                );
                for (key, written) in &routed {
                    adopted.insert(key, value(written.clone()));
                }
                presets.insert(table_name(preset), Item::Table(adopted));
            }
            document.insert("presets", Item::Table(presets));
        }
        format!("{SCHEMA}\n{document}")
    }

    /// The signals the source announces: for each kind a route of a
    /// preset takes, the first signal of that kind.
    fn signals(&self) -> Vec<&'static str> {
        let kinds = self
            .presets
            .iter()
            .filter_map(|preset| preset.tables.parse::<toml::Table>().ok())
            .filter_map(|tables| {
                tables
                    .get("routes")
                    .and_then(toml::Value::as_array)
                    .cloned()
            })
            .flatten()
            .filter_map(|route| route.get("kinds").and_then(toml::Value::as_array).cloned())
            .flatten()
            .filter_map(|kind| kind.as_str().map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        let mut signals = kinds
            .iter()
            .filter_map(|kind| {
                Signal::ALL
                    .into_iter()
                    .find(|signal| signal.kind().as_str() == kind)
            })
            .collect::<Vec<_>>();
        signals.sort_unstable();
        signals.dedup();
        signals.into_iter().map(Signal::name).collect()
    }

    /// The `[store]` table.
    fn stored(&self) -> Table {
        let mut store = Table::new();
        store.insert("kind", value(self.store.kind()));
        if self.store.sealed() {
            store.insert("key", value(secret(STORE_KEY)));
        }
        if let Kept::ForgejoBranch { token } = self.store {
            store.insert("token", value(secret(token)));
        }
        store
    }
}

/// The `[presets.<name>]` table a shipped preset is adopted as: its name
/// without herald's prefix.
pub(crate) fn table_name(preset: &Preset) -> &'static str {
    preset
        .name
        .strip_prefix(herald_config::SHIPPED_PREFIX)
        .unwrap_or(preset.name)
}

/// `{ secret = "<name>" }`.
fn secret(name: &str) -> Value {
    Value::InlineTable(InlineTable::from_iter([("secret", Value::from(name))]))
}

/// A table holding `table` as `name`, written under `name`'s own header.
fn implicit(name: &str, table: Table) -> Table {
    let mut parent = Table::new();
    parent.set_implicit(true);
    parent.insert(name, Item::Table(table));
    parent
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_of(repository: &str) -> String {
        First {
            forge: Forge::GitLab,
            repository,
            api: None,
            kind: "slack",
            keys: &[],
            presets: &[],
            store: &Kept::Markers,
        }
        .source()
    }

    #[test]
    fn a_source_is_named_after_its_repository_as_a_name_herald_takes() {
        assert_eq!(source_of("group/sub/My_App.rs"), "my-app-rs");
        assert_eq!(source_of("owner/__"), "repository");
    }
}
