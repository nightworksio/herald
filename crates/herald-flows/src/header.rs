//! A release's header read for a view: the open milestones its selector
//! matches in each repository it reads, and the GitLab groups above them,
//! and the open issues they carry where those are its rows, read once a
//! round (ADR-0082, ADR-0084, ADR-0086).

use herald_core::{Headed, Header, Unread, View};
use herald_kinds::Queue;
use herald_ports::{Clock, Http};
use jiff::Timestamp;

use crate::budget::Budgeted;

/// What one source reads a release's header through: its queue and its
/// budgeted HTTP.
pub(crate) type Reaching<'r, H, C> = (&'r dyn Queue, &'r Budgeted<'r, H, C>);

/// The header of `view`'s release, where it shows one or its rows are
/// issues: each of `asked` [`herald_core::heading`] reads, read at `now`
/// through the source of `reached` it names, in that order, a GitLab
/// group's issues kept where the view holds their projects; each named unread
/// with why where it could not be read or lay past the bound.
pub(crate) async fn header<H: Http, C: Clock>(
    view: &View,
    reached: &[Reaching<'_, H, C>],
    (asked, now): (Vec<Headed>, Timestamp),
) -> Option<Header> {
    let release = view.release.as_ref()?;
    let (glob, heading) = release.heading()?;
    let (read, unread) = herald_core::heading(view, asked);
    let mut header = Header {
        issues: heading.issues.then(Vec::new),
        unread,
        ..Header::default()
    };
    let sourced = read.iter().filter_map(|headed| {
        reached
            .get(headed.source)
            .map(|reaching| (headed, reaching))
    });
    for (headed, &(queue, http)) in sourced {
        match queue.milestoned(http, (headed, glob), (heading, now)).await {
            Ok(found) => header.merge(found.read_as(view, headed)),
            Err(unpolled) => header.unread.push(Unread {
                repository: headed.path.clone(),
                visibility: headed.visibility,
                reason: http.unread_milestones(unpolled),
            }),
        }
    }
    Some(header)
}
