/// A kind of record a store may keep beside its ledger, cursors, schedules
/// and deliveries, which every store that keeps records keeps.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum RecordKind {
    /// Who follows or subscribes to a project.
    Subscriptions,
    /// Each route's pinned post.
    Pins,
    /// The Notes herald's ActivityPub actors published.
    Notes,
    /// What each project last announced, for commands to answer with.
    Latest,
    /// The keys inbound activities are checked with, under
    /// `[server.keys] cache = "store"`.
    Keys,
}

impl RecordKind {
    /// The kind as a config spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Subscriptions => "subscriptions",
            Self::Pins => "pins",
            Self::Notes => "notes",
            Self::Latest => "latest",
            Self::Keys => "keys",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_is_named_as_a_config_spells_it() {
        for (kind, name) in [
            (RecordKind::Subscriptions, "subscriptions"),
            (RecordKind::Pins, "pins"),
            (RecordKind::Notes, "notes"),
            (RecordKind::Latest, "latest"),
            (RecordKind::Keys, "keys"),
        ] {
            assert_eq!(kind.name(), name);
            let read: RecordKind = serde_json::from_value(serde_json::json!(name)).unwrap();
            assert_eq!(read, kind);
        }
    }
}
