//! The members an owner's overview reads one by one: those a view shows a
//! pull request of, its newest pushed, or for a view that holds only some
//! of them the first by name (ADR-0066, ADR-0067); and its capacity
//! overview over them.

use herald_core::{Capacity, Listed, Overviews, Owner};
use herald_kinds::{Asked, Most, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _};
use serde::Deserialize;

use super::Scoped;
use crate::Forge;
use crate::forge::{OWNER, whole};
use crate::limited::checked;
use crate::queue::capacity::counted;

/// What an owner's repositories, the newest pushed first, are read as.
const PUSHED: &str = "an owner's repositories, the newest pushed first";

/// Which of an owner's members an overview reads one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Chosen {
    /// Those the view shows a pull request of.
    Shown,
    /// The newest pushed, at most `most`.
    Pushed {
        /// The most read.
        most: usize,
    },
    /// The first by name of those the view holds, at most `most`: what it
    /// reads in place of the newest pushed where it holds only some, so the
    /// push times of those it hides choose nothing.
    Held {
        /// The most read.
        most: usize,
    },
}

impl Chosen {
    /// What it reads for a view that holds only some members where
    /// `narrowed` says so.
    pub(super) const fn within(self, narrowed: bool) -> Self {
        match self {
            Self::Pushed { most } if narrowed => Self::Held { most },
            _ => self,
        }
    }
}

#[derive(Deserialize)]
struct Repository {
    #[serde(alias = "path_with_namespace")]
    full_name: String,
}

impl Scoped {
    /// The path, under its forge's API, of the newest pushed `most` of
    /// the repositories of the owner, of the kind `owner`; none on
    /// Forgejo, which lists no push order.
    ///
    /// # Errors
    ///
    /// Where the owner's name is empty or a dot segment.
    fn pushed_path(&self, owner: Owner, most: usize) -> Result<Option<String>, Unpolled> {
        let name = whole(&self.owner, OWNER)?;
        Ok(match (self.forge(), owner) {
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
        })
    }

    /// The members of `asked.scope` `chosen` names: those of `shown`, the
    /// newest pushed of them asked through `http`, at most `most`, or for a
    /// narrowed scope the first `most` by name with nothing asked; on
    /// Forgejo the first `most` its membership record keeps.
    pub(super) async fn chosen_members<'a>(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (asked, shown): (Asked<'a>, &[&str]),
        chosen: Chosen,
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
        match chosen.within(scope.narrowed) {
            Chosen::Shown => Ok(shown.iter().filter_map(|path| held(path)).collect()),
            Chosen::Held { most } => {
                let mut named: Vec<&Listed> = scope.members.iter().collect();
                named.sort_by_cached_key(|member| member.path.to_ascii_lowercase());
                Ok(named.into_iter().take(most).collect())
            }
            Chosen::Pushed { most } => {
                let Some(path) = self.pushed_path(scope.owner, most)? else {
                    return Ok(scope.members.iter().take(most).collect());
                };
                let answer = http
                    .send(self.root.rooted(&path, self.token.as_ref()))
                    .await
                    .map_err(Unpolled::unreachable)?;
                checked(self.forge(), PUSHED, &answer)?;
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
        let chosen = match capacity {
            Capacity::Shown => Chosen::Shown,
            Capacity::Members { most } => Chosen::Pushed { most },
        };
        let members = self.chosen_members(http, (asked, shown), chosen).await?;
        let mut all = Vec::new();
        for member in members {
            let origin = self.origin(&member.path);
            let opened = self.reading(&origin, ("", member.visibility), (Most::default(), false));
            all.push(opened.capacity(http, member.visibility).await?);
        }
        Ok(counted(all))
    }
}
