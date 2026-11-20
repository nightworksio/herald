//! A living post sent a view's advisories, deleted and posted anew in
//! place of an edit where its destination keeps what an edit replaced
//! (ADR-0067).

use herald_core::{
    Entry, Marker, Plan, Planned, Revision, SourceName, Step, plan, replaced, resumes,
};
use herald_ports::{Clock, Http, Signer, Store, Versioned};

use super::{Claim, Outcome, Progress, Run, stopped};

impl<S: Store, C: Clock, G: Signer, H: Http> Run<'_, S, C, G, H> {
    /// The post `record` keeps deleted, the post first, and `wanted` posted
    /// anew in its place showing `revision`, claimed for this run as the
    /// announcement of `source`: what a living post sent a view's
    /// advisories gets in place of an edit where its destination keeps what
    /// an edit replaced. A replacement under way of the same revision is
    /// resumed, each step it only attempted read back before it is sent
    /// again; a new one first reads back, from the posts marked `marker`,
    /// each post or chunk sent and never recorded, deletes each found and
    /// keeps the rest among the entry's strays, so it takes none for its
    /// own. The revision is recorded once every step landed (ADR-0067).
    pub(super) async fn replacing<R>(
        &self,
        record: Versioned<Entry>,
        (wanted, revision): (&[Step], &Revision),
        (source, marker): (Option<&SourceName>, &Marker),
    ) -> Result<(Vec<Planned>, Progress), Claim<R>> {
        let now = self.ports.clock.now();
        if let Plan::Held { by, until } = plan(Some(&record.value), &[], self.holder, now) {
            return Err(Claim::Done(Outcome::Held { by, until }));
        }
        let begun = if resumes(&record.value, revision) || record.value.unrecorded().is_empty() {
            record.value
        } else {
            let found = self
                .found(&record.value.key, marker)
                .await
                .map_err(|reason| Claim::Done(stopped(reason)))?;
            record.value.found(found.as_ref())
        };
        let replacing = replaced(&begun, revision, now);
        let mut planned = match plan(Some(&begun), &replacing.deletions(), self.holder, now) {
            Plan::Held { .. } | Plan::Complete => Vec::new(),
            Plan::Send(planned) => planned,
        };
        let entry = Entry {
            replacing: Some(replacing),
            ..begun
        };
        if let Plan::Send(fresh) = plan(Some(&entry.reposting()), wanted, self.holder, now) {
            planned.extend(fresh);
        }
        let claimed = entry.through(source).wanting(wanted);
        let progress = self
            .progressed(claimed, Some(record.version))
            .await
            .map_err(|reason| Claim::Done(stopped(reason)))?;
        Ok((planned, progress))
    }
}
