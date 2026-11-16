//! The projects every source and owner's member announces as, so a member
//! whose project another announces as is withheld (ADR-0030).

use std::collections::{BTreeMap, BTreeSet};

use herald_config::Config;
use herald_core::{Listed, SourceName};
use herald_kinds::Expand;

/// One source or member announcing as a project.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Claim {
    /// How a report names it.
    label: String,
    /// The owner source and forge id of the member it is, where it is one.
    member: Option<(SourceName, String)>,
}

/// The projects announced as, by their name ignoring case, and the sources
/// whose records could not be read, so whose projects are not known.
#[derive(Debug, Default)]
pub(crate) struct Claims {
    claimed: BTreeMap<String, Vec<Claim>>,
    unread: BTreeSet<SourceName>,
}

impl Claims {
    /// The projects `config`'s sources and apps name.
    pub(crate) fn of(config: &Config) -> Self {
        let mut claims = Self::default();
        for (name, source) in &config.sources {
            for project in source.projects() {
                claims.source(name, &project);
            }
        }
        for (name, app) in &config.apps {
            for project in app.named() {
                claims.claim(project, format!("the app {name}"), None);
            }
        }
        claims
    }

    /// The source `name` announcing as `project`.
    pub(crate) fn source(&mut self, name: &SourceName, project: &str) {
        self.claim(project, format!("the source {}", name.as_str()), None);
    }

    /// The source `name`, whose records could not be read.
    pub(crate) fn unread(&mut self, name: &SourceName) {
        self.unread.insert(name.clone());
    }

    /// Each of `taken`, the members of the owner source `owner`, announcing
    /// as the project `expand` names it.
    pub(crate) fn members<'a>(
        &mut self,
        (owner, expand): (&SourceName, &dyn Expand),
        taken: impl Iterator<Item = &'a Listed>,
    ) {
        for member in taken {
            if let Ok(project) = expand.project(member) {
                self.claim(
                    &project,
                    labelled(owner, member),
                    Some((owner.clone(), member.id.clone())),
                );
            }
        }
    }

    fn claim(&mut self, project: &str, label: String, member: Option<(SourceName, String)>) {
        self.claimed
            .entry(project.to_lowercase())
            .or_default()
            .push(Claim { label, member });
    }

    /// Why the member `repository` of the owner source `owner` is withheld:
    /// another member, source or app announces as its project, or a
    /// source's records could not be read, so that cannot be checked.
    pub(crate) fn collision(
        &self,
        (owner, expand): (&SourceName, &dyn Expand),
        repository: &Listed,
    ) -> Option<String> {
        let project = expand.project(repository).ok()?;
        if !self.unread.is_empty() {
            let unread: Vec<&str> = self.unread.iter().map(SourceName::as_str).collect();
            return Some(format!(
                "the records of {} could not be read, so whether another announces as its project {project} cannot be checked; its events are withheld this round",
                unread.join(", ")
            ));
        }
        let me = (owner.clone(), repository.id.clone());
        let other = self
            .claimed
            .get(&project.to_lowercase())?
            .iter()
            .find(|claim| claim.member.as_ref() != Some(&me))?;
        Some(format!(
            "{} announces as the project {project}, which {} announces as too; its events are withheld until they differ",
            repository.path, other.label
        ))
    }
}

/// How a report names the member `repository` of the owner source `owner`.
fn labelled(owner: &SourceName, repository: &Listed) -> String {
    format!("{}'s {}", owner.as_str(), repository.path)
}
