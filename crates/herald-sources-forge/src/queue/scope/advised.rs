//! An owner's advisories overview: its organization's or group's own
//! listings where its forge has them, else its members one by one, each
//! advisory kept only where its membership record holds the repository
//! (ADR-0067).

use herald_core::{Advising, Advisories, AdvisoriesFrom, Owner, Unread, Visibility};
use herald_kinds::{Asked, Unpolled};
use herald_ports::{DynHttp, Erased};

use super::Scoped;
use super::pushed::Chosen;
use crate::Forge;
use crate::queue::advisory::{Holder, Reader};

/// Why an owner's advisories were not read where its members were never
/// listed.
pub(crate) const UNLISTED: &str = "its members were never listed, so none of their advisories were read; its next poll lists them";

/// What `unread` members of an owner `chosen` passed over: how many, and
/// which were read instead.
fn passed_over(unread: usize, chosen: Chosen) -> String {
    match chosen {
        Chosen::Shown => format!(
            "{unread} of its member repositories were not read: only those the view shows an open pull request of are"
        ),
        Chosen::Pushed { most } => format!(
            "{unread} of its member repositories were not read: only the {most} newest pushed are, as advisory_repositories says"
        ),
        Chosen::Held { most } => format!(
            "{unread} of the member repositories the view holds were not read: only the first {most} by name are, as advisory_repositories says"
        ),
    }
}

impl Scoped {
    /// The members `advising` reads one by one, where it does not read the
    /// owner's own listings: those shown, or the newest pushed.
    fn advised_members(&self, advising: Advising, owner: Owner) -> Option<Chosen> {
        let listed = self.forge() == Forge::Forgejo
            || (advising.from == AdvisoriesFrom::Owner && owner == Owner::Organization);
        match advising.from {
            _ if listed => None,
            AdvisoriesFrom::Shown => Some(Chosen::Shown),
            AdvisoriesFrom::Owner | AdvisoriesFrom::Members => Some(Chosen::Pushed {
                most: advising.repositories,
            }),
        }
    }

    /// The advisories overview `advising` asks of the owner's members at
    /// `at`, `asked` holding them and `shown` those the view shows a pull
    /// request of, each listing read through `http`.
    pub(super) async fn advisories_of(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (asked, shown): (Asked<'_>, &[&str]),
        (advising, at): (Advising, jiff::Timestamp),
    ) -> Result<Advisories, Unpolled> {
        let unread = |reason: String| Unread {
            repository: self.owner.clone(),
            visibility: Visibility::Private,
            reason,
        };
        let Some(scope) = asked.scope else {
            let mut found = Advisories::none(at);
            found.unread.push(unread(UNLISTED.to_owned()));
            return Ok(found);
        };
        let Some(chosen) = self.advised_members(advising, scope.owner) else {
            let reader = Reader {
                origin: &self.root,
                token: self.token.as_ref(),
            };
            let member = |path: &str| {
                scope
                    .members
                    .iter()
                    .find(|member| member.path.eq_ignore_ascii_case(path))
                    .map(|member| member.path.clone())
            };
            let holder = Holder::Owner(&self.owner);
            return reader
                .read(http, (holder, &member), (advising.most, at))
                .await;
        };
        let mut found = Advisories::none(at);
        let chosen = chosen.within(scope.narrowed);
        let members = self.chosen_members(http, (asked, shown), chosen).await?;
        let skipped = scope.members.len().saturating_sub(members.len());
        if skipped > 0 {
            found.more |= !matches!(chosen, Chosen::Shown);
            found.unread.push(unread(passed_over(skipped, chosen)));
        }
        for member in members {
            let left = advising.most.saturating_sub(found.open.len());
            if left == 0 {
                found.more = true;
                break;
            }
            let origin = self.origin(&member.path);
            let reader = Reader {
                origin: &origin,
                token: self.token.as_ref(),
            };
            let holder = Holder::Repository(&member.path);
            found.merge(reader.read(http, (holder, &|_| None), (left, at)).await?);
        }
        Ok(found)
    }
}
