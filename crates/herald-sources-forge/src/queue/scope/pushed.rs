//! An owner's capacity overview: the runs of the repositories a view
//! shows a pull request of, or of its newest pushed members (ADR-0066).

use herald_core::{Capacity, Listed, Overviews, Owner};
use herald_kinds::{Asked, Most, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _};
use herald_wire::answered;
use serde::Deserialize;

use super::Scoped;
use crate::Forge;
use crate::queue::capacity::counted;

/// What an owner's repositories, the newest pushed first, are read as.
const PUSHED: &str = "an owner's repositories, the newest pushed first";

#[derive(Deserialize)]
struct Repository {
    #[serde(alias = "path_with_namespace")]
    full_name: String,
}

impl Scoped {
    /// The path, under its forge's API, of the newest pushed `most` of
    /// the repositories of the owner, of the kind `owner`; none on
    /// Forgejo, which lists no push order.
    fn pushed_path(&self, owner: Owner, most: usize) -> Option<String> {
        let name = herald_core::unreserved(&self.owner);
        match (self.forge(), owner) {
            (Forge::GitHub, Owner::Organization) => Some(format!(
                "/orgs/{name}/repos?sort=pushed&direction=desc&per_page={most}"
            )),
            (Forge::GitHub, Owner::User) => Some(format!(
                "/users/{name}/repos?sort=pushed&direction=desc&per_page={most}"
            )),
            (Forge::GitLab, Owner::Organization) => Some(format!(
                "/groups/{name}/projects?order_by=last_activity_at&sort=desc&include_subgroups=true&per_page={most}"
            )),
            (Forge::GitLab, Owner::User) => Some(format!(
                "/users/{name}/projects?order_by=last_activity_at&sort=desc&per_page={most}"
            )),
            (Forge::Forgejo, _) => None,
        }
    }

    /// The members of `asked.scope` `capacity` names: those of `shown`, or
    /// the newest pushed of them asked through `http`, at most `most`; on
    /// Forgejo the first `most` its membership record keeps.
    async fn counted_members<'a>(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (asked, shown): (Asked<'a>, &[&str]),
        capacity: Capacity,
    ) -> Result<Vec<&'a Listed>, Unpolled> {
        let Some(scope) = asked.scope else {
            return Ok(Vec::new());
        };
        let held = |path: &str| {
            scope
                .members
                .iter()
                .find(|member| member.path.eq_ignore_ascii_case(path))
        };
        match capacity {
            Capacity::Shown => Ok(shown.iter().filter_map(|path| held(path)).collect()),
            Capacity::Members { most } => {
                let Some(path) = self.pushed_path(scope.owner, most) else {
                    return Ok(scope.members.iter().take(most).collect());
                };
                let answer = http
                    .send(self.root.rooted(&path, self.token.as_ref()))
                    .await
                    .map_err(Unpolled::unreachable)?;
                answered(PUSHED, &answer).map_err(Unpolled::unreadable)?;
                let pushed: Vec<Repository> =
                    herald_wire::read(PUSHED, self.root.reading().parse, &answer.body)
                        .map_err(Unpolled::unreadable)?;
                Ok(pushed
                    .iter()
                    .filter_map(|repository| held(&repository.full_name))
                    .take(most)
                    .collect())
            }
        }
    }

    /// The capacity overview of the members `capacity` names, each read
    /// through `http`.
    pub(super) async fn capacity_of(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (asked, shown): (Asked<'_>, &[&str]),
        capacity: Capacity,
    ) -> Result<Overviews, Unpolled> {
        let members = self.counted_members(http, (asked, shown), capacity).await?;
        let mut all = Vec::new();
        for member in members {
            let origin = self.origin(&member.path);
            let opened = self.reading(&origin, ("", member.visibility), (Most::default(), false));
            all.push(opened.capacity(http, member.visibility).await?);
        }
        Ok(counted(all))
    }
}
