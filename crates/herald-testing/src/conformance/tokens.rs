use herald_core::KeptToken;
use herald_ports::{StoreError, Tokens, Version, Written};

use super::{Checks, NOON};

fn kept(name: &str, sealed: &str) -> KeptToken {
    KeptToken {
        name: name.to_owned(),
        sealed: sealed.to_owned(),
        expires: NOON,
    }
}

/// Whether minted tokens are kept by name, written with compare-and-set,
/// and read back as written.
pub(super) async fn tokens<S: Tokens>(store: &S, checks: &mut Checks) -> Result<(), StoreError> {
    let (one, other) = (
        kept("github_apps.herald/1", "c2VhbGVk"),
        kept("github_apps.herald/2", "b3RoZXI="),
    );
    checks.ensure(
        store.token(&one.name).await?.is_none(),
        "a new store keeps a token",
    );
    let first = checks.written(store.put_token(&one, None).await?);
    checks.ensure(
        store.put_token(&one, None).await? == Written::Conflict
            && store.put_token(&one, Some(Version(0))).await? == Written::Conflict,
        "a token was written over a version it did not expect",
    );
    let moved = kept(&one.name, "bW92ZWQ=");
    checks.written(store.put_token(&moved, Some(first)).await?);
    checks.written(store.put_token(&other, None).await?);
    checks.ensure(
        store
            .token(&one.name)
            .await?
            .is_some_and(|kept| kept.value == moved)
            && store
                .token(&other.name)
                .await?
                .is_some_and(|kept| kept.value == other),
        "a token read back was not the one written last under its name",
    );
    Ok(())
}
