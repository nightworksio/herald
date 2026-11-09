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
}
