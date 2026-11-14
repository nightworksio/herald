use std::collections::BTreeMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::{Cursor, Glob};

/// Who may see a repository.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
#[schemars(rename = "RepositoryVisibility")]
pub enum Visibility {
    /// Anyone.
    Public,
    /// Members of the forge's enterprise or instance.
    Internal,
    /// Those given access.
    Private,
}

impl Visibility {
    /// The visibility as a config and the forges spell it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Private => "private",
        }
    }
}

/// One repository an owner's listing names, as its selection reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    /// The forge's stable id for it, which a rename keeps.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its full path, such as `owner/name` or a GitLab project's
    /// `group/subgroup/name`.
    pub path: String,
    /// Its topics.
    pub topics: Vec<String>,
    /// Its custom properties, each with the values it holds.
    pub properties: BTreeMap<String, Vec<String>>,
    /// Who may see it.
    pub visibility: Visibility,
    /// Whether it is archived.
    pub archived: bool,
    /// Whether it is a fork.
    pub fork: bool,
    /// Whether it is a template.
    pub template: bool,
    /// Its main language, where the forge names one.
    pub language: Option<String>,
}

/// Which of an owner's repositories a source takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Names taken; every name where empty.
    pub include: Vec<Glob>,
    /// Names left out, whatever `include` says.
    pub exclude: Vec<Glob>,
    /// Topics, any one of which a member holds; any where empty.
    pub topics: Vec<String>,
    /// The visibilities taken.
    pub visibility: Vec<Visibility>,
    /// Whether archived repositories are taken.
    pub archived: bool,
    /// Whether forks are taken.
    pub fork: bool,
    /// Whether templates are taken.
    pub template: bool,
    /// Languages, any one of which a member is written in; any where empty.
    pub language: Vec<String>,
    /// Properties, every one of which a member holds with one of its
    /// values.
    pub properties: BTreeMap<String, Vec<String>>,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            include: Vec::new(),
            exclude: Vec::new(),
            topics: Vec::new(),
            visibility: vec![Visibility::Public],
            archived: false,
            fork: false,
            template: false,
            language: Vec::new(),
            properties: BTreeMap::new(),
        }
    }
}

/// Whether `wanted` is empty or `held` holds one of it, ignoring case.
fn any_of<'a>(wanted: &[String], mut held: impl Iterator<Item = &'a str>) -> bool {
    wanted.is_empty() || held.any(|held| wanted.iter().any(|want| want.eq_ignore_ascii_case(held)))
}

impl Selection {
    /// Whether it takes `member`.
    #[must_use]
    pub fn takes(&self, listed: &Listed) -> bool {
        let named = |globs: &[Glob]| globs.iter().any(|glob| glob.matches(&listed.name));
        (self.include.is_empty() || named(&self.include))
            && !named(&self.exclude)
            && any_of(&self.topics, listed.topics.iter().map(String::as_str))
            && any_of(&self.language, listed.language.as_deref().into_iter())
            && self.visibility.contains(&listed.visibility)
            && (self.archived || !listed.archived)
            && (self.fork || !listed.fork)
            && (self.template || !listed.template)
            && self.properties.iter().all(|(name, values)| {
                listed.properties.get(name).is_some_and(|held| {
                    any_of(values, held.iter().map(String::as_str)) && !held.is_empty()
                })
            })
    }

    /// Whether it may take a repository not everyone may see.
    #[must_use]
    pub fn takes_unlisted(&self) -> bool {
        self.visibility
            .iter()
            .any(|visibility| *visibility != Visibility::Public)
    }
}

/// One repository's membership of an owner source, kept with its cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Membership {
    /// The repository, as its owner's last listing named it.
    pub repository: Listed,
    /// When a listing first took it.
    pub discovered: Timestamp,
    /// When it left the selection, where it has.
    pub left: Option<Timestamp>,
    /// What polling it has seen, once it has been polled.
    pub cursor: Option<Cursor>,
}

/// What kind of account owns an owner source's repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Owner {
    /// An organization, or a GitLab group.
    Organization,
    /// A user.
    User,
}

/// An owner source's last listing of its repositories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    /// What kind of account the owner is.
    pub owner: Owner,
    /// The `ETag` of each page the listing read, in order.
    pub pages: Vec<Option<String>>,
    /// When the source was first listed.
    pub first: Timestamp,
    /// When it was last listed in full.
    pub listed: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(name: &str) -> Listed {
        Listed {
            id: "1".to_owned(),
            name: name.to_owned(),
            path: format!("lemonfiber/{name}"),
            topics: vec!["Media".to_owned()],
            properties: BTreeMap::from([("team".to_owned(), vec!["media".to_owned()])]),
            visibility: Visibility::Public,
            archived: false,
            fork: false,
            template: false,
            language: Some("Rust".to_owned()),
        }
    }

    fn globs(texts: &[&str]) -> Vec<Glob> {
        texts.iter().map(|text| Glob::new(text).unwrap()).collect()
    }

    #[test]
    fn by_default_public_repositories_are_taken_and_archives_forks_and_templates_left_out() {
        let selection = Selection::default();
        assert!(selection.takes(&listed("herald")));
        assert!(!selection.takes_unlisted());
        for left in [
            Listed {
                archived: true,
                ..listed("old")
            },
            Listed {
                fork: true,
                ..listed("fork")
            },
            Listed {
                template: true,
                ..listed("plugin-template")
            },
            Listed {
                visibility: Visibility::Private,
                ..listed("secret")
            },
            Listed {
                visibility: Visibility::Internal,
                ..listed("inner")
            },
        ] {
            assert!(!selection.takes(&left), "{left:?}");
        }
        let everything = Selection {
            visibility: vec![
                Visibility::Public,
                Visibility::Internal,
                Visibility::Private,
            ],
            archived: true,
            fork: true,
            template: true,
            ..Selection::default()
        };
        assert!(everything.takes_unlisted());
        assert!(everything.takes(&Listed {
            archived: true,
            fork: true,
            template: true,
            visibility: Visibility::Private,
            ..listed("all")
        }));
    }

    #[test]
    fn names_are_taken_by_include_and_left_out_by_exclude_which_wins() {
        let selection = Selection {
            include: globs(&["plugin-*", "lemonfiber"]),
            exclude: globs(&["plugin-template", "PLUGIN-OLD-*"]),
            template: true,
            ..Selection::default()
        };
        assert!(selection.takes(&listed("plugin-sonarr")));
        assert!(selection.takes(&listed("LemonFiber")));
        assert!(!selection.takes(&listed("website")));
        assert!(!selection.takes(&listed("plugin-template")));
        assert!(!selection.takes(&listed("plugin-old-radarr")));
    }

    #[test]
    fn topics_and_languages_are_any_of_and_properties_all_of_the_named() {
        let topics = Selection {
            topics: vec!["media".to_owned(), "tools".to_owned()],
            ..Selection::default()
        };
        assert!(topics.takes(&listed("a")));
        assert!(!topics.takes(&Listed {
            topics: Vec::new(),
            ..listed("b")
        }));
        let language = Selection {
            language: vec!["rust".to_owned()],
            ..Selection::default()
        };
        assert!(language.takes(&listed("a")));
        assert!(!language.takes(&Listed {
            language: None,
            ..listed("b")
        }));
        assert!(!language.takes(&Listed {
            language: Some("PHP".to_owned()),
            ..listed("c")
        }));
        let properties = Selection {
            properties: BTreeMap::from([
                ("team".to_owned(), vec!["media".to_owned()]),
                (
                    "tier".to_owned(),
                    vec!["core".to_owned(), "edge".to_owned()],
                ),
            ]),
            ..Selection::default()
        };
        let tiered = |tier: &[&str]| Listed {
            properties: BTreeMap::from([
                ("team".to_owned(), vec!["Media".to_owned()]),
                (
                    "tier".to_owned(),
                    tier.iter().map(|tier| (*tier).to_owned()).collect(),
                ),
            ]),
            ..listed("a")
        };
        assert!(properties.takes(&tiered(&["edge"])));
        assert!(!properties.takes(&tiered(&["lab"])));
        assert!(!properties.takes(&tiered(&[])));
        assert!(!properties.takes(&listed("untiered")));
        let named_only = Selection {
            properties: BTreeMap::from([("team".to_owned(), Vec::new())]),
            ..Selection::default()
        };
        assert!(named_only.takes(&listed("a")));
        assert!(!named_only.takes(&Listed {
            properties: BTreeMap::from([("team".to_owned(), Vec::new())]),
            ..listed("b")
        }));
    }

    #[test]
    fn each_visibility_is_named_as_the_forges_spell_it() {
        assert_eq!(
            [
                Visibility::Public,
                Visibility::Internal,
                Visibility::Private
            ]
            .map(Visibility::name),
            ["public", "internal", "private"]
        );
    }
}
