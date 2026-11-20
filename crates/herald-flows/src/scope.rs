//! The repositories of an owner source a view reads: each member its last
//! membership record holds and has not left (ADR-0062).

use herald_core::{Listed, Owner, SourceName};
use herald_ports::{Cursors, Store};

use crate::announce::Stop;
use crate::poll::stored;

/// Why a view reading an owner source could not: its repositories were
/// never listed.
pub(crate) const UNLISTED: &str =
    "the view reads an owner source whose repositories were never listed; its next poll lists them";

/// The owner source `source`'s repositories as `store` keeps them: what
/// kind of account the owner is, and each member that has not left.
///
/// # Errors
///
/// Where the store could not be read, or keeps no listing of the source.
pub(crate) async fn members<S: Store>(
    store: &S,
    source: &SourceName,
) -> Result<(Owner, Vec<Listed>), Stop> {
    let cursors = store.cursors();
    let listing = cursors
        .listing(source)
        .await
        .map_err(stored)?
        .ok_or_else(|| Stop::Source(UNLISTED.to_owned()))?;
    let members = cursors
        .members(source)
        .await
        .map_err(stored)?
        .into_iter()
        .filter(|member| member.value.left.is_none())
        .map(|member| member.value.repository)
        .collect();
    Ok((listing.value.owner, members))
}
