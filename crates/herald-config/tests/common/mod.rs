//! What the config's tests share.

use herald_config::Kinds;

/// Every destination kind herald ships, in the order herald lists them.
pub fn kinds() -> Kinds {
    Kinds::new()
        .destination::<herald_dest_discord::DiscordKind>()
        .destination::<herald_dest_fediverse::MastodonKind>()
        .destination::<herald_dest_matrix::MatrixKind>()
        .destination::<herald_dest_slack::SlackKind>()
        .destination::<herald_dest_fediverse::ActorKind>()
}
