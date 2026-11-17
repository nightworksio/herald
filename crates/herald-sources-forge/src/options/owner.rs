//! The keys that choose an owner's members and name their projects
//! (ADR-0029, ADR-0030).

use std::num::NonZeroUsize;

use herald_core::{Glob, ProjectName, REPOSITORY_FIELDS, Sandbox, Selection, path_of_names};
use jiff::SignedDuration;

use super::{OVERRIDDEN, SourceOptions, globs, tie};
use crate::Forge;

pub use herald_core::{KEEP_MEMBERS, MOST_LISTED, MOST_MEMBERS, MOST_MEMBERS_ALLOWED};

/// A property's values: one, or any of several.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Values {
    /// One value.
    One(String),
    /// Any one of several values.
    Any(Vec<String>),
}

impl Values {
    fn listed(&self) -> Vec<String> {
        match self {
            Self::One(value) => vec![value.clone()],
            Self::Any(values) => values.clone(),
        }
    }
}

/// Where an owner source's member file is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum Members {
    /// A file beside the config, such as `repos.toml`.
    File {
        /// Its path, from the config's directory.
        file: String,
    },
    /// A file in a repository on the source's forge, read through its API
    /// with the source's token each round.
    Repository {
        /// The repository, as `repository` writes one.
        repository: String,
        /// The file's path in it.
        path: String,
        /// The branch, tag or commit it is read at; the default branch where
        /// absent.
        #[serde(rename = "ref")]
        reference: Option<String>,
    },
}

/// The field each entry of a member file names its repository by where
/// `key` is absent.
pub const KEY: &str = "name";

/// The keys that only an owner source may give.
const OWNED: [&str; 24] = [
    "include",
    "exclude",
    "topics",
    "visibility",
    "archived",
    "fork",
    "template",
    "language",
    "properties",
    "subgroups",
    "most_listed",
    "most_members",
    "keep_members",
    "listed_page",
    "members",
    "tables",
    "key",
    "project",
    "repositories",
    "transport",
    "graphql",
    "graphql_batch",
    "feed_page",
    "feed_pages",
];

/// The project template `source`, compiled, where it compiles.
fn compiled(source: &str) -> Option<ProjectName> {
    ProjectName::compile(source, Sandbox::DEFAULT).ok()
}

impl SourceOptions {
    pub(super) fn owner_check(&self, forge: Forge, owner: &str) -> Result<(), String> {
        if !path_of_names(owner, forge.owners()) {
            return Err(format!(
                "\"{owner}\" is not an owner; give it as {}, of letters, digits, '.', '_' and '-'",
                forge.owner_shape()
            ));
        }
        if self.subgroups.is_some() && forge != Forge::GitLab {
            return Err("subgroups is a GitLab group's; remove it".to_owned());
        }
        self.transport_check(forge)?;
        if self.members.is_none() && (!self.tables.is_empty() || self.key.is_some()) {
            return Err(
                "tables and key read a member file; give members, or remove them".to_owned(),
            );
        }
        if let Some(Members::Repository {
            repository, path, ..
        }) = &self.members
        {
            if !path_of_names(repository, forge.names()) {
                return Err(format!(
                    "members.repository \"{repository}\" is not a repository; give it as {}",
                    forge.shape()
                ));
            }
            if path
                .split('/')
                .any(|segment| matches!(segment, "" | "." | ".."))
            {
                return Err(format!(
                    "members.path \"{path}\" is not a file's path in the repository; give it as dir/file.toml, without empty, . or .. parts"
                ));
            }
        }
        if let Some(project) = &self.project {
            Self::project_check(project).map_err(|reason| format!("project {reason}"))?;
        }
        for (glob, chosen) in &self.repositories {
            Glob::new(glob).map_err(|error| format!("repositories: {error}"))?;
            if let Some(project) = &chosen.project {
                Self::project_check(project)
                    .map_err(|reason| format!("repositories.\"{glob}\".project {reason}"))?;
            }
        }
        for (key, sets) in OVERRIDDEN {
            let globs: Vec<Glob> = self
                .repositories
                .iter()
                .filter(|(_, chosen)| sets(chosen))
                .filter_map(|(glob, _)| Glob::new(glob).ok())
                .collect();
            if let Some((glob, other)) = tie(&globs) {
                return Err(format!(
                    "repositories \"{glob}\" and \"{other}\" both set {key}, are as narrow as each other and could both name one repository; make one narrower"
                ));
            }
        }
        if self.most_members().get() > MOST_MEMBERS_ALLOWED {
            return Err(format!(
                "most_members is at most {MOST_MEMBERS_ALLOWED}; lower it"
            ));
        }
        self.selection().map(|_| ())
    }

    /// Whether the `project` template compiles and reads a repository's
    /// attributes alone.
    pub(crate) fn project_check(project: &str) -> Result<(), String> {
        let named =
            ProjectName::compile(project, Sandbox::DEFAULT).map_err(|error| error.reason)?;
        let unknown = named.unknown();
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "reads {}, which a repository does not carry; use repository.{}",
                unknown.join(", "),
                REPOSITORY_FIELDS.join(", repository.")
            ))
        }
    }

    /// The project template, compiled, where one is given.
    #[must_use]
    pub fn project(&self) -> Option<ProjectName> {
        self.project.as_deref().and_then(compiled)
    }

    /// Each glob of `repositories` that sets a project, with its template
    /// compiled, narrowest first.
    #[must_use]
    pub fn projects(&self) -> Vec<(Glob, ProjectName)> {
        let mut projects: Vec<(Glob, ProjectName)> = self
            .repositories
            .iter()
            .filter_map(|(glob, chosen)| {
                Some((
                    Glob::new(glob).ok()?,
                    chosen.project.as_deref().and_then(compiled)?,
                ))
            })
            .collect();
        projects.sort_by_key(|(glob, _)| std::cmp::Reverse(glob.specificity()));
        projects
    }

    /// Each owner's key the options give.
    pub(super) fn owned(&self) -> Vec<&'static str> {
        let given = [
            !self.include.is_empty(),
            !self.exclude.is_empty(),
            !self.topics.is_empty(),
            self.visibility.is_some(),
            self.archived,
            self.fork,
            self.template,
            !self.language.is_empty(),
            !self.properties.is_empty(),
            self.subgroups.is_some(),
            self.most_listed.is_some(),
            self.most_members.is_some(),
            self.keep_members.is_some(),
            self.listed_page.is_some(),
            self.members.is_some(),
            !self.tables.is_empty(),
            self.key.is_some(),
            self.project.is_some(),
            !self.repositories.is_empty(),
            self.transport.is_some(),
            self.graphql.is_some(),
            self.graphql_batch.is_some(),
            self.feed_page.is_some(),
            self.feed_pages.is_some(),
        ];
        OWNED
            .into_iter()
            .zip(given)
            .filter_map(|(key, given)| given.then_some(key))
            .collect()
    }

    /// Which of the owner's repositories the source takes.
    ///
    /// # Errors
    ///
    /// When a glob of `include` or `exclude` cannot be read.
    pub fn selection(&self) -> Result<Selection, String> {
        Ok(Selection {
            include: globs(&self.include)?,
            exclude: globs(&self.exclude)?,
            topics: self.topics.clone(),
            visibility: self
                .visibility
                .clone()
                .unwrap_or_else(|| Selection::default().visibility),
            archived: self.archived,
            fork: self.fork,
            template: self.template,
            language: self.language.clone(),
            properties: self
                .properties
                .iter()
                .map(|(name, values)| (name.clone(), values.listed()))
                .collect(),
            names: None,
        })
    }

    /// The most repositories a listing reads before the filters.
    #[must_use]
    pub fn most_listed(&self) -> NonZeroUsize {
        self.most_listed.unwrap_or(MOST_LISTED)
    }

    /// The most members the filters may keep.
    #[must_use]
    pub fn most_members(&self) -> NonZeroUsize {
        self.most_members.unwrap_or(MOST_MEMBERS)
    }

    /// The field each entry of the member file names its repository by.
    #[must_use]
    pub fn key(&self) -> &str {
        self.key.as_deref().unwrap_or(KEY)
    }

    /// How long a member that left is kept.
    #[must_use]
    pub fn keep_members(&self) -> SignedDuration {
        self.keep_members.map_or(KEEP_MEMBERS, |kept| kept.0)
    }

    /// Whether a GitLab group's members include its subgroups' projects.
    #[must_use]
    pub fn subgroups(&self) -> bool {
        self.subgroups.unwrap_or(true)
    }
}
