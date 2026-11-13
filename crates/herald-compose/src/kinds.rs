use herald_config::Kinds;

/// Every kind herald ships, as each host reads its config with them.
#[must_use]
pub fn kinds() -> Kinds {
    Kinds::new()
        .destination::<herald_dest_discord::DiscordKind>()
        .destination::<herald_dest_fediverse::MastodonKind>()
        .destination::<herald_dest_matrix::MatrixKind>()
        .destination::<herald_dest_slack::SlackKind>()
        .destination::<herald_dest_fediverse::ActorKind>()
        .source::<herald_sources_forge::GitHubKind>()
        .source::<herald_sources_forge::GitLabKind>()
        .source::<herald_sources_forge::ForgejoKind>()
        .source::<herald_sources_registry::PackagistKind>()
        .source::<herald_sources_registry::CratesIoKind>()
        .source::<herald_sources_registry::NpmKind>()
        .source::<herald_sources_registry::DockerHubKind>()
        .source::<herald_sources_feed::FeedKind>()
        .store::<herald_store_sqlite::SqliteKind>()
        .store::<herald_store_postgres::PostgresKind>()
        .store::<herald_store_d1::D1Kind>()
        .store::<herald_store_durable_object::DurableObjectKind>()
        .store::<herald_store_deno_kv::DenoKvKind>()
        .store::<herald_store_fastly_kv::FastlyKvKind>()
        .store::<herald_store_dynamodb::DynamodbKind>()
        .store::<herald_store_git_branch::GitBranchKind>()
        .store::<herald_store_markers::MarkersKind>()
        .store::<herald_store_actions_cache::ActionsCacheKind>()
        .store::<herald_store_repo_variables::RepoVariablesKind>()
}
