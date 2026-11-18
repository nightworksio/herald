use std::collections::BTreeMap;

use crate::event::Event;
use crate::glob::Glob;
use crate::kind::Attribute;
use crate::origin::Origin;

/// What of an event a route narrows its sources' events by: each list
/// takes any one of what it holds, and every one where empty; every
/// property and attribute must be held; and nothing `not` takes is taken.
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
    /// The attributes, every one of which an event carries with one of the
    /// values given for it, as text.
    pub attributes: BTreeMap<String, Vec<String>>,
    /// What is never taken, whatever else takes it.
    pub not: Option<Box<Self>>,
}

/// Whether `globs` is empty or one of it matches `name`.
fn named(globs: &[Glob], name: &str) -> bool {
    globs.is_empty() || globs.iter().any(|glob| glob.matches(name))
}

/// Whether `names` is empty or holds `name`, ignoring case.
fn listed(names: &[String], name: &str) -> bool {
    names.is_empty() || names.iter().any(|listed| listed.eq_ignore_ascii_case(name))
}

/// `attribute` as a route's values name it: text as it is, a number as
/// written, a flag as `true` or `false`.
fn written(attribute: &Attribute) -> String {
    match attribute {
        Attribute::Text(text) => text.clone(),
        Attribute::Number(number) => number.to_string(),
        Attribute::Flag(flag) => flag.to_string(),
    }
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

    /// Whether `event` carries every attribute it narrows by with one of
    /// the values given for it, ignoring case.
    fn carried(&self, event: &Event) -> bool {
        self.attributes.iter().all(|(name, values)| {
            event
                .attributes()
                .get(name)
                .is_some_and(|attribute| listed(values, &written(attribute)))
        })
    }

    /// Whether it takes `event` by its own fields, leaving `not` aside.
    fn takes_itself(&self, event: &Event) -> bool {
        named(&self.projects, event.project())
            && event
                .origin()
                .map_or(!self.reads_origin(), |origin| self.holds(origin))
            && self.carried(event)
    }

    /// Whether it takes `event`: one with no origin only where it narrows
    /// by the project alone, and none `not` takes.
    #[must_use]
    pub fn takes(&self, event: &Event) -> bool {
        self.takes_itself(event)
            && self
                .not
                .as_deref()
                .is_none_or(|not| !not.takes_itself(event))
    }

    /// Whether it may take an event of `project` from `origin`, every field
    /// of an origin and every attribute not known taken: one `not` is sure
    /// to take, by the project and origin alone, never.
    #[must_use]
    pub fn admits(&self, project: &str, origin: Option<&Origin>) -> bool {
        let may = |matching: &Self| {
            named(&matching.projects, project) && origin.is_none_or(|origin| matching.holds(origin))
        };
        let refused = |not: &Self| {
            not.attributes.is_empty() && (origin.is_some() || !not.reads_origin()) && may(not)
        };
        may(self) && self.not.as_deref().is_none_or(|not| !refused(not))
    }
}

#[cfg(test)]
mod tests;
