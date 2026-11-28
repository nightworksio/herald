use herald_config::Kinds;

/// Every kind herald ships, as each host reads its config with them.
#[must_use]
pub fn kinds() -> Kinds {
    Kinds::new()
        .destination::<herald_dest_discord::DiscordKind>()
        .destination::<herald_dest_fediverse::MastodonKind>()
        .destination::<herald_dest_matrix::MatrixKind>()
        .destination::<herald_dest_slack::SlackKind>()
        .destination::<herald_dest_teams::TeamsKind>()
        .destination::<herald_dest_email::EmailKind>()
        .destination::<herald_dest_fediverse::ActorKind>()
        .destination::<herald_dest_herald::HeraldKind>()
        .destination::<herald_dest_herald::PushKind>()
        .source::<herald_sources_forge::GitHubKind>()
        .source::<herald_sources_forge::GitLabKind>()
        .source::<herald_sources_forge::ForgejoKind>()
        .source::<herald_sources_forge::GhcrKind>()
        .source::<herald_sources_registry::PackagistKind>()
        .source::<herald_sources_registry::CratesIoKind>()
        .source::<herald_sources_registry::NpmKind>()
        .source::<herald_sources_registry::DockerHubKind>()
        .source::<herald_sources_registry::OciKind>()
        .source::<herald_sources_feed::FeedKind>()
        .source::<herald_sources_herald::HeraldKind>()
        .store::<herald_store_sqlite::SqliteKind>()
        .store::<herald_store_postgres::PostgresKind>()
        .store::<herald_store_d1::D1Kind>()
        .store::<herald_store_durable_object::DurableObjectKind>()
        .store::<herald_store_deno_kv::DenoKvKind>()
        .store::<herald_store_fastly_kv::FastlyKvKind>()
        .store::<herald_store_dynamodb::DynamodbKind>()
        .store::<herald_store_git_branch::GitBranchKind>()
        .store::<herald_store_forgejo_branch::ForgejoBranchKind>()
        .store::<herald_store_markers::MarkersKind>()
        .store::<herald_store_actions_cache::ActionsCacheKind>()
        .store::<herald_store_repo_variables::RepoVariablesKind>()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    #[test]
    fn every_route_option_of_every_kind_is_written_or_refused_by_a_local_preset_on_purpose() {
        for kind in super::kinds().preset_route_options() {
            let taken: BTreeSet<&str> = kind.taken.iter().map(String::as_str).collect();
            let writes: BTreeSet<&str> = kind.writes.iter().copied().collect();
            let refuses: BTreeSet<&str> = kind.refuses.iter().copied().collect();
            assert!(
                writes.is_disjoint(&refuses),
                "{}: a key both written and refused",
                kind.kind
            );
            assert_eq!(
                taken,
                &writes | &refuses,
                "{}: say of each route option whether a local preset writes it (PRESET_WRITES) or not (PRESET_REFUSES)",
                kind.kind
            );
            assert!(
                kind.addressing.iter().all(|key| writes.contains(key)),
                "{}: an addressing key a local preset does not write",
                kind.kind
            );
            let marks: BTreeSet<&str> = ["private", "vouch_unchecked", "viewers"].into();
            assert!(
                writes.is_disjoint(&marks),
                "{}: a local preset writes a mark of what a route may show",
                kind.kind
            );
        }
    }
}
