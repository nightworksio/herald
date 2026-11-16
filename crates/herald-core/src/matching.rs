use std::collections::BTreeMap;

use crate::event::Event;
use crate::glob::Glob;
use crate::origin::Origin;

/// What of an event a route narrows its sources' events by: each list
/// takes any one of what it holds, and every one where empty; every
/// property must be held.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Match {
    /// The projects taken.
    pub projects: Vec<Glob>,
    /// The repositories or packages taken, by their full path.
    pub repositories: Vec<Glob>,
    /// The owners taken.
    pub owners: Vec<Glob>,
    /// The forges and registries taken, as a source's `kind` names them.
    pub forges: Vec<String>,
    /// The hosts taken.
    pub hosts: Vec<String>,
    /// The topics, any one of which an origin holds.
    pub topics: Vec<String>,
    /// The properties, every one of which an origin holds with one of the
    /// values given for it.
    pub properties: BTreeMap<String, Vec<String>>,
}

/// Whether `globs` is empty or one of it matches `name`.
fn named(globs: &[Glob], name: &str) -> bool {
    globs.is_empty() || globs.iter().any(|glob| glob.matches(name))
}

/// Whether `names` is empty or holds `name`, ignoring case.
fn listed(names: &[String], name: &str) -> bool {
    names.is_empty() || names.iter().any(|listed| listed.eq_ignore_ascii_case(name))
}

impl Match {
    /// Whether it narrows by the origin as well as the project.
    #[must_use]
    pub fn reads_origin(&self) -> bool {
        !(self.repositories.is_empty()
            && self.owners.is_empty()
            && self.forges.is_empty()
            && self.hosts.is_empty()
            && self.topics.is_empty()
            && self.properties.is_empty())
    }

    /// Whether it takes `origin` by every field it narrows by.
    fn holds(&self, origin: &Origin) -> bool {
        named(&self.repositories, &origin.repository)
            && named(&self.owners, &origin.owner)
            && listed(&self.forges, &origin.forge)
            && listed(&self.hosts, &origin.host)
            && origin.holds_topics(&self.topics)
            && origin.holds_properties(&self.properties)
    }

    /// Whether it takes `event`: one with no origin only where it narrows
    /// by the project alone.
    #[must_use]
    pub fn takes(&self, event: &Event) -> bool {
        named(&self.projects, event.project())
            && event
                .origin()
                .map_or(!self.reads_origin(), |origin| self.holds(origin))
    }

    /// Whether it may take an event of `project` from `origin`, every field
    /// of an origin not known taken.
    #[must_use]
    pub fn admits(&self, project: &str, origin: Option<&Origin>) -> bool {
        named(&self.projects, project) && origin.is_none_or(|origin| self.holds(origin))
    }
}

#[cfg(test)]
mod tests;
