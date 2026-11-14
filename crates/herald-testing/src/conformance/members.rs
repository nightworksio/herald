use std::collections::BTreeMap;

use herald_core::{Cursor, Identity, Listed, Listing, Membership, Owner, SourceName, Visibility};
use herald_ports::{Cursors, StoreError, Written};

use super::{Checks, NOON};

pub(super) const OTHER: &str = "owner-other";

pub(super) fn membership(id: &str) -> Membership {
    Membership {
        repository: Listed {
            id: id.to_owned(),
            name: format!("repository-{id}"),
            path: format!("lemonfiber/repository-{id}"),
            topics: vec!["media".to_owned()],
            properties: BTreeMap::from([("team".to_owned(), vec!["media".to_owned()])]),
            visibility: Visibility::Public,
            archived: false,
            fork: false,
            template: false,
            language: Some("Rust".to_owned()),
        },
        discovered: NOON,
        left: None,
        cursor: None,
    }
}

pub(super) fn listing() -> Listing {
    Listing {
        owner: Owner::Organization,
        pages: vec![Some("e1".to_owned()), None],
        first: NOON,
        listed: NOON,
    }
}

/// Whether an owner source's listing is kept with compare-and-set, its
/// members are each kept, written with compare-and-set, listed under
/// their source alone, and forgotten one at a time.
pub(super) async fn members<S: Cursors>(store: &S, checks: &mut Checks) -> Result<(), StoreError> {
    let source = SourceName::new("owner");
    let other = SourceName::new(OTHER);
    checks.ensure(
        store.listing(&source).await?.is_none(),
        "an owner source never listed had a listing",
    );
    checks.ensure(
        store.members(&source).await?.is_empty(),
        "an owner source never listed had members",
    );
    let listing = listing();
    let v1 = checks.written(store.put_listing(&source, &listing, None).await?);
    let read = store.listing(&source).await?;
    checks.ensure(
        read.as_ref().map(|read| &read.value) == Some(&listing),
        "a listing written was not read back as written",
    );
    checks.ensure(
        read.map(|read| read.version) == Some(v1),
        "a listing was read back at another version than it was written at",
    );
    checks.ensure(
        store.listing(&other).await?.is_none(),
        "writing one listing made another appear",
    );
    checks.ensure(
        store.put_listing(&source, &listing, None).await? == Written::Conflict,
        "a listing was written as new over one that exists",
    );
    let first = membership("1");
    let second = membership("2");
    let joined = checks.written(store.put_member(&source, &first, None).await?);
    checks.written(store.put_member(&source, &second, None).await?);
    checks.written(store.put_member(&other, &membership("3"), None).await?);
    let polled = Membership {
        repository: first.repository.clone(),
        discovered: NOON,
        left: None,
        cursor: Some(Cursor {
            seen: vec![Identity::new("a")],
            etag: None,
            last_modified: None,
        }),
    };
    let moved = checks.written(store.put_member(&source, &polled, Some(joined)).await?);
    checks.ensure(moved != joined, "a member rewritten kept its version");
    checks.ensure(
        store.put_member(&source, &first, Some(joined)).await? == Written::Conflict,
        "a member was written over a version it did not expect",
    );
    let mut kept: Vec<Membership> = store
        .members(&source)
        .await?
        .into_iter()
        .map(|member| member.value)
        .collect();
    kept.sort_by(|one, another| one.repository.id.cmp(&another.repository.id));
    checks.ensure(
        kept == [polled, second.clone()],
        "the members listed were not every member written under the source, as written",
    );
    store.forget_member(&source, "1").await?;
    store.forget_member(&source, "absent").await?;
    let left: Vec<Membership> = store
        .members(&source)
        .await?
        .into_iter()
        .map(|member| member.value)
        .collect();
    checks.ensure(
        left == [second],
        "forgetting a member did not forget it alone",
    );
    checks.ensure(
        store.members(&other).await?.len() == 1,
        "forgetting a member forgot another source's",
    );
    Ok(())
}
