//! A release's base branches read for a view: the head's checks of each
//! branch the pull requests it picks merge into, read once a round, each
//! request asked on the validators kept from the last read where the store
//! keeps pull requests (ADR-0083).

use herald_core::{BaseKey, BaseRead, Based, KeptBase, MOST_BASES, Pull, SourceName, View};
use herald_kinds::Queue;
use herald_ports::{Clock, Http, Pulls};

use crate::budget::Budgeted;
use crate::codeowned::Keeping;

/// The base branches of `view`'s release, where it reads them, read
/// through `queue`, the source `name`, and `http`: each branch a pull
/// request of `pulls` the release picks merges into, once per repository
/// and branch, seen as its rows are, kept as `keeping` says; each named
/// unread with why where it could not be read, and where `asked`, the
/// branches the view's release has read this round, reached
/// [`MOST_BASES`].
pub(crate) async fn bases<P: Pulls, H: Http, C: Clock>(
    view: &View,
    (name, queue, http): (&SourceName, &dyn Queue, &Budgeted<'_, H, C>),
    (pulls, (store, at)): (&[Pull], Keeping<'_, P>),
    asked: &mut usize,
) -> Vec<Based> {
    let Some(release) = view
        .release
        .as_ref()
        .filter(|release| release.reads_bases())
    else {
        return Vec::new();
    };
    let mut branches: Vec<(String, &str, &str, _)> = pulls
        .iter()
        .filter(|pull| release.picks(pull) && view.holds(&pull.repository))
        .filter_map(|pull| {
            let branch = pull.base.as_deref()?;
            let path = pull.repository.as_str();
            Some((path.to_ascii_lowercase(), branch, path, pull.visibility))
        })
        .collect();
    branches.sort_unstable_by(|left, right| {
        (&left.0, left.1, left.3).cmp(&(&right.0, right.1, right.3))
    });
    branches.dedup_by(|later, first| later.0 == first.0 && later.1 == first.1);
    let mut bases = Vec::with_capacity(branches.len());
    for (lowered, branch, path, visibility) in branches {
        let seen = |read| Based {
            repository: path.to_owned(),
            visibility,
            branch: branch.to_owned(),
            read,
        };
        if *asked >= MOST_BASES {
            bases.push(seen(BaseRead::Unread(format!(
                "a release reads the checks of at most {MOST_BASES} base branches"
            ))));
            continue;
        }
        *asked += 1;
        let key = BaseKey {
            source: name.clone(),
            repository: lowered,
            branch: branch.to_owned(),
        };
        let kept = match store {
            Some(store) => store.base(&key).await.ok().flatten(),
            None => None,
        };
        let found = match queue
            .based(http, (path, branch), kept.as_ref().map(|kept| &kept.value))
            .await
        {
            Ok(found) => found,
            Err(unpolled) => {
                bases.push(seen(BaseRead::Unread(http.unread_base(unpolled))));
                continue;
            }
        };
        let changed = kept
            .as_ref()
            .is_none_or(|kept| kept.value.parts != found.parts);
        if let (Some(store), true) = (store, changed)
            && let Some(record) = KeptBase::of(key, found.parts, at)
        {
            store
                .put_base(&record, kept.map(|kept| kept.version))
                .await
                .ok();
        }
        bases.push(seen(found.read));
    }
    bases
}
