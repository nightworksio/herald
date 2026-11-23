//! A release's header read for a view: the open milestones its selector
//! matches in each repository it shows a pull request of the release in,
//! read once a round (ADR-0082).

use herald_core::{Header, MOST_HEADER_REPOSITORIES, Pull, Unread, View, Visibility};
use herald_kinds::Queue;
use herald_ports::{Clock, Http};
use jiff::Timestamp;

use crate::budget::Budgeted;

/// The header of `view`'s release, where it shows one, read through
/// `queue` and `http` at `now`: each repository of `pulls` the release
/// picks, and the one repository `queue` reads, seen as its rows are or
/// as private where it has none; each named unread with why where it could
/// not be read, and where `asked`, the repositories the view's header has
/// read this round, reached [`MOST_HEADER_REPOSITORIES`].
pub(crate) async fn header<H: Http, C: Clock>(
    view: &View,
    (queue, http): (&dyn Queue, &Budgeted<'_, H, C>),
    (pulls, now): (&[Pull], Timestamp),
    asked: &mut usize,
) -> Option<Header> {
    let release = view.release.as_ref()?;
    let glob = release.headed()?;
    let mut repositories: Vec<(String, &str, Visibility)> = pulls
        .iter()
        .filter(|pull| release.picks(pull))
        .map(|pull| (pull.repository.as_str(), pull.visibility))
        .chain(queue.repository().map(|path| (path, Visibility::Private)))
        .filter(|(path, _)| view.holds(path))
        .map(|(path, visibility)| (path.to_ascii_lowercase(), path, visibility))
        .collect();
    repositories.sort_unstable_by(|left, right| (&left.0, left.2).cmp(&(&right.0, right.2)));
    repositories.dedup_by(|later, first| later.0 == first.0);
    let mut header = Header::default();
    for (_, path, visibility) in repositories {
        let unread = |reason: String| Unread {
            repository: path.to_owned(),
            visibility,
            reason,
        };
        if *asked >= MOST_HEADER_REPOSITORIES {
            header.unread.push(unread(format!(
                "a release's header reads the milestones of at most {MOST_HEADER_REPOSITORIES} repositories"
            )));
            continue;
        }
        *asked += 1;
        match queue.milestoned(http, (path, glob, visibility), now).await {
            Ok(read) => header.merge(read),
            Err(unpolled) => header.unread.push(unread(http.unread_milestones(unpolled))),
        }
    }
    Some(header)
}
