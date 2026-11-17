use std::collections::BTreeMap;

use herald_core::{Draft, Event, HeldEvent, Hold, Identity, Kind, Pending, SourceName, versioned};
use herald_ports::{StoreError, Version, Waiting, Written};

use super::{Checks, NOON};

/// An hour after [`NOON`], when the held events written are due.
const LATER: jiff::Timestamp = jiff::Timestamp::constant(1_793_451_600, 0);

pub(super) fn hold(identity: &str) -> Hold {
    Hold::Tag {
        source: SourceName::new("tagged"),
        identity: Identity::new(identity),
    }
}

pub(super) fn pending(identity: &str) -> Pending {
    let draft = Draft {
        identity: identity.to_owned(),
        kind: Kind::RELEASE,
        project: "tagged".to_owned(),
        title: "v1".to_owned(),
        body: "## Added".to_owned(),
        links: BTreeMap::new(),
        attributes: versioned("v1", false),
        occurred: NOON,
    };
    Pending {
        hold: hold(identity),
        events: Event::normalise(draft)
            .map(|event| HeldEvent {
                source: SourceName::new("tagged"),
                member: Some("7".to_owned()),
                event,
            })
            .into_iter()
            .collect(),
        opened: NOON,
        due: LATER,
        closed: None,
        landed: false,
    }
}

/// Whether events held back are kept by what holds them, written with
/// compare-and-set, listed as written, and forgotten one at a time.
pub(super) async fn waiting<S: Waiting>(store: &S, checks: &mut Checks) -> Result<(), StoreError> {
    let (one, other) = (pending("github.com/1/v1"), pending("github.com/1/v2"));
    checks.ensure(
        store.pending(&one.hold).await?.is_none() && store.every_pending().await?.is_empty(),
        "a new store holds events back",
    );
    let first = checks.written(store.put_pending(&one, None).await?);
    checks.ensure(
        store.put_pending(&one, None).await? == Written::Conflict
            && store.put_pending(&one, Some(Version(0))).await? == Written::Conflict,
        "held events were written over a version they did not expect",
    );
    let moved = Pending {
        hold: one.hold.clone(),
        events: one.events.clone(),
        opened: NOON,
        due: NOON,
        closed: Some(LATER),
        landed: true,
    };
    checks.written(store.put_pending(&moved, Some(first)).await?);
    checks.written(store.put_pending(&other, None).await?);
    checks.ensure(
        store
            .pending(&one.hold)
            .await?
            .is_some_and(|kept| kept.value == moved),
        "held events read back were not those written last under what holds them",
    );
    let mut every: Vec<Pending> = store
        .every_pending()
        .await?
        .into_iter()
        .map(|kept| kept.value)
        .collect();
    every.sort_by(|a, b| a.hold.cmp(&b.hold));
    checks.ensure(
        every == [moved, other.clone()],
        "the held events listed were not every record written, as written",
    );
    store.drop_pending(&one.hold).await?;
    checks.ensure(
        store.pending(&one.hold).await?.is_none()
            && store
                .pending(&other.hold)
                .await?
                .is_some_and(|kept| kept.value == other),
        "dropping held events did not forget them alone",
    );
    Ok(())
}
