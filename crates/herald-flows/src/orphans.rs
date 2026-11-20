//! The possibly orphaned parts of a report's living posts, as `herald
//! orphans clear` lists them, and one route's dropped once the operator
//! says the parts they were shown were deleted by hand (ADR-0067).

use herald_core::{AdvisedPost, Entry, RouteId, SourceName, Stray, digest};
use herald_ports::{Cursors, Ledger, Store, StoreError, Versioned, Written};

use crate::recall::living_key;

/// How many hex digits of the digest of a route's strays the operator is
/// shown and names back.
const SEEN_DIGITS: usize = 12;

/// The possibly orphaned parts of a report's living post on one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphaned {
    /// The route.
    pub route: RouteId,
    /// Each part, 0 for the post, else the chunk, once for each send
    /// under a key of its own, in order.
    pub parts: Vec<u32>,
    /// What the operator names back to clear exactly these: a short
    /// digest of each part and the key it was sent under.
    pub seen: String,
}

impl Orphaned {
    fn of(route: &RouteId, strays: &[Stray]) -> Self {
        let mut parts: Vec<u32> = strays.iter().map(|stray| stray.part).collect();
        parts.sort_unstable();
        let listed = strays
            .iter()
            .map(|stray| format!("{}:{}", stray.part, stray.token))
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            route: route.clone(),
            parts,
            seen: digest(&listed).chars().take(SEEN_DIGITS).collect(),
        }
    }
}

/// Each living post the cursor of the report `report` records, of the
/// route `route` alone where it names one, each route once.
async fn posts<S: Store>(
    store: &S,
    report: &str,
    route: Option<&RouteId>,
) -> Result<Vec<AdvisedPost>, StoreError> {
    let record = store.cursors().cursor(&SourceName::new(report)).await?;
    let mut posts: Vec<AdvisedPost> = Vec::new();
    for post in record.into_iter().flat_map(|record| record.value.advised) {
        if route.is_none_or(|route| post.route == *route)
            && !posts.iter().any(|kept| kept.route == post.route)
        {
            posts.push(post);
        }
    }
    Ok(posts)
}

/// The ledger entry of `post` of the report `report`, where it holds a
/// possibly orphaned part.
async fn strayed<S: Store>(
    store: &S,
    report: &str,
    post: &AdvisedPost,
) -> Result<Option<Versioned<Entry>>, StoreError> {
    let entry = store.ledger().entry(&living_key(report, post)).await?;
    Ok(entry.filter(|entry| !entry.value.strays.is_empty()))
}

/// Why nothing was cleared, or listed, for the report `report`, of the
/// route `route` where it names one: no living post holds such a part.
fn none(report: &str, route: Option<&RouteId>) -> String {
    format!(
        "no living post of the report {report} holds a possibly orphaned part{}, so nothing was cleared",
        route.map_or_else(String::new, |route| format!(
            " on the route {}",
            route.as_str()
        ))
    )
}

/// Why the records of the report `report`'s living posts could not be
/// read.
fn unread(report: &str, error: &StoreError) -> String {
    format!(
        "the records of the report {report}'s living posts could not be read, so nothing was cleared: {error}"
    )
}

/// Each possibly orphaned part of the living posts the cursor of the
/// report `report` records, of the route `route` alone where it names
/// one, by route, with what the operator names back to clear them
/// (ADR-0067).
///
/// # Errors
///
/// Where no such part is held, or the cursor or a ledger entry cannot be
/// read, in plain words.
pub async fn orphans<S: Store>(
    store: &S,
    report: &str,
    route: Option<&RouteId>,
) -> Result<Vec<Orphaned>, String> {
    let mut found = Vec::new();
    for post in posts(store, report, route)
        .await
        .map_err(|error| unread(report, &error))?
    {
        if let Some(entry) = strayed(store, report, &post)
            .await
            .map_err(|error| unread(report, &error))?
        {
            found.push(Orphaned::of(&post.route, &entry.value.strays));
        }
    }
    if found.is_empty() {
        return Err(none(report, route));
    }
    Ok(found)
}

/// The possibly orphaned parts of the report `report`'s living post on
/// the route `route` dropped from its ledger entry, as the operator says
/// they were deleted by hand, where they are still the ones `seen` names:
/// its recall record and the requests kept for it go at the next round
/// (ADR-0067).
///
/// # Errors
///
/// Where the route's post holds no such part, holds others than `seen`
/// names, or its records cannot be read or written, in plain words that
/// say whether anything was cleared.
pub async fn orphans_cleared<S: Store>(
    store: &S,
    report: &str,
    (route, seen): (&RouteId, &str),
) -> Result<Orphaned, String> {
    let named = route.as_str();
    let posts = posts(store, report, Some(route))
        .await
        .map_err(|error| unread(report, &error))?;
    let Some(post) = posts.first() else {
        return Err(none(report, Some(route)));
    };
    let Some(entry) = strayed(store, report, post)
        .await
        .map_err(|error| unread(report, &error))?
    else {
        return Err(none(report, Some(route)));
    };
    let orphaned = Orphaned::of(route, &entry.value.strays);
    if orphaned.seen != seen {
        return Err(format!(
            "the possibly orphaned parts of the report {report}'s living post on the route {named} changed since they were listed, so nothing was cleared; list them again with `herald orphans clear {report} --route {named}`"
        ));
    }
    let kept = Entry {
        strays: Vec::new(),
        ..entry.value
    };
    match store.ledger().put_entry(&kept, Some(entry.version)).await {
        Ok(Written::Written(_)) => Ok(orphaned),
        Ok(Written::Conflict) => Err(format!(
            "a run wrote the ledger entry of the report {report}'s living post on the route {named} meanwhile, so nothing was cleared; list them again with `herald orphans clear {report} --route {named}`"
        )),
        Err(error) => Err(format!(
            "the ledger entry of the report {report}'s living post on the route {named} could not be written, so its possibly orphaned parts may or may not be cleared; list them again with `herald orphans clear {report} --route {named}`: {error}"
        )),
    }
}
