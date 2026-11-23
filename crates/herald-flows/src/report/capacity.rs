//! The capacity overview a view reads of one source: its queued and
//! running CI, each run and unread repository seen as its rows are
//! (ADR-0066).

use herald_config::ViewConfig;
use herald_core::{Listed, Overviews, Pull, SourceName, Unread, Visibility};
use herald_kinds::{Asked, Queue, Scope};
use herald_ports::{Clock, Http, Signer, Store};

use crate::announce::Ports;
use crate::budget::Budgeted;

/// The capacity overview `capacity` of `view` shows of what `queue`, the
/// source `name`, read as `read` through `http`, asked as `asked` of its
/// scope's `members`: each run and unread repository seen as its rows are,
/// and the source named unread with why where it could not be read.
pub(super) async fn capacity_of<S: Store, C: Clock, G: Signer, H: Http>(
    (view, capacity): (&ViewConfig, herald_core::Capacity),
    (name, queue, http): (&SourceName, &dyn Queue, &Budgeted<'_, H, C>),
    (asked, members): (Asked<'_>, Option<&(herald_core::Owner, Vec<Listed>)>),
    read: &[Pull],
    ports: &Ports<'_, S, C, G>,
) -> Overviews {
    let shown = herald_core::rows(&view.view, read, ports.clock.now());
    let mut shown: Vec<&str> = shown
        .iter()
        .map(|row| row.pull.repository.as_str())
        .collect();
    shown.sort_unstable();
    shown.dedup();
    let held: Vec<Listed> = members
        .iter()
        .flat_map(|(_, members)| members)
        .filter(|member| view.view.holds(&member.path))
        .cloned()
        .collect();
    let asked = Asked {
        scope: members.map(|(owner, _)| Scope {
            owner: *owner,
            members: &held,
            narrowed: !view.view.holds_every(),
        }),
        ..asked
    };
    let counted = match queue.capacity(http, (asked, &shown), capacity).await {
        Ok(counted) => counted,
        Err(unpolled) => unread_source(name, http.unread(unpolled)),
    };
    seen_as_rows(counted, read)
}

/// What a capacity read of `source` that failed for `reason` shows: the
/// source named unread with why, seen as private.
fn unread_source(source: &SourceName, reason: String) -> Overviews {
    Overviews {
        unread: vec![Unread {
            repository: source.as_str().to_owned(),
            visibility: Visibility::Private,
            reason,
        }],
        ..Overviews::default()
    }
}

/// `counted` with each run and unread repository seen as the rows of
/// `pulls` of its repository are, and as the forge said where none is:
/// as its membership record holds it, or as private.
fn seen_as_rows(mut counted: Overviews, pulls: &[Pull]) -> Overviews {
    let seen = |repository: &str| {
        pulls
            .iter()
            .find(|pull| pull.repository.eq_ignore_ascii_case(repository))
            .map(|pull| pull.visibility)
    };
    for run in counted.capacity.iter_mut().flatten() {
        if let Some(visibility) = seen(&run.repository) {
            run.visibility = visibility;
        }
    }
    for unread in &mut counted.unread {
        if let Some(visibility) = seen(&unread.repository) {
            unread.visibility = visibility;
        }
    }
    counted
}
