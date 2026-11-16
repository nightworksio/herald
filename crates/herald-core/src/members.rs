use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::{Cursor, Glob};

/// The most an owner's listing reads, before the filters, where
/// `most_listed` is absent.
pub const MOST_LISTED: NonZeroUsize = NonZeroUsize::new(10_000).unwrap();

/// The most members a selection keeps where `most_members` is absent.
pub const MOST_MEMBERS: NonZeroUsize = NonZeroUsize::new(1_000).unwrap();

/// The most `most_members` may be.
pub const MOST_MEMBERS_ALLOWED: usize = 100_000;

/// How long a member that left is kept where `keep_members` is absent.
pub const KEEP_MEMBERS: SignedDuration = SignedDuration::from_hours(30 * 24);

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
    /// The names a member file lists, each a repository's name or full
    /// path, ignoring case; where given, no other repository is taken.
    pub names: Option<Vec<String>>,
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
            names: None,
        }
    }
}

/// Whether `wanted` is empty or `held` holds one of it, ignoring case.
pub(crate) fn any_of<'a>(wanted: &[String], mut held: impl Iterator<Item = &'a str>) -> bool {
    wanted.is_empty() || held.any(|held| wanted.iter().any(|want| want.eq_ignore_ascii_case(held)))
}

/// Whether `held` holds every property of `wanted` with one of the values
/// given for it, ignoring case, any value where none is given.
pub(crate) fn holds_all(
    wanted: &BTreeMap<String, Vec<String>>,
    held: &BTreeMap<String, Vec<String>>,
) -> bool {
    wanted.iter().all(|(name, values)| {
        held.get(name)
            .is_some_and(|held| !held.is_empty() && any_of(values, held.iter().map(String::as_str)))
    })
}

/// Whether `name` is `repository`'s name or full path, ignoring case.
fn naming(name: &str, (repository, path): (&str, &str)) -> bool {
    name.eq_ignore_ascii_case(repository) || name.eq_ignore_ascii_case(path)
}

impl Listed {
    /// Whether `name` names it, by its name or its full path, ignoring case.
    #[must_use]
    pub fn named(&self, name: &str) -> bool {
        naming(name, (&self.name, &self.path))
    }

    /// Its owner's path: the full path without its name.
    #[must_use]
    pub fn owner(&self) -> &str {
        self.path
            .rsplit_once('/')
            .map_or(self.path.as_str(), |(owner, _)| owner)
    }
}

/// One repository as a webhook delivery names it: each attribute the
/// delivery leaves out is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Delivered {
    /// Its name.
    pub name: String,
    /// Its full path.
    pub path: String,
    /// Its topics.
    pub topics: Option<Vec<String>>,
    /// Its custom properties, each with the values it holds.
    pub properties: Option<BTreeMap<String, Vec<String>>>,
    /// Who may see it.
    pub visibility: Option<Visibility>,
    /// Whether it is archived.
    pub archived: Option<bool>,
    /// Whether it is a fork.
    pub fork: Option<bool>,
    /// Whether it is a template.
    pub template: Option<bool>,
    /// Its main language.
    pub language: Option<String>,
}

/// A repository's attributes as a selection reads them, each where known.
struct Seen<'a> {
    name: &'a str,
    path: &'a str,
    topics: Option<&'a [String]>,
    properties: Option<&'a BTreeMap<String, Vec<String>>>,
    visibility: Option<Visibility>,
    archived: Option<bool>,
    fork: Option<bool>,
    template: Option<bool>,
    language: Spoken<'a>,
}

/// A repository's main language, as a selection reads it.
#[derive(Clone, Copy)]
enum Spoken<'a> {
    /// Not known, so not filtered on.
    Unknown,
    /// Known: its language, or that it has none.
    Known(Option<&'a str>),
}

impl Selection {
    /// Whether it takes `member`.
    #[must_use]
    pub fn takes(&self, listed: &Listed) -> bool {
        self.holds(&Seen {
            name: &listed.name,
            path: &listed.path,
            topics: Some(&listed.topics),
            properties: Some(&listed.properties),
            visibility: Some(listed.visibility),
            archived: Some(listed.archived),
            fork: Some(listed.fork),
            template: Some(listed.template),
            language: Spoken::Known(listed.language.as_deref()),
        })
    }

    /// Whether it takes `delivered` by what the delivery names, each
    /// attribute it leaves out aside.
    #[must_use]
    pub fn admits(&self, delivered: &Delivered) -> bool {
        self.holds(&Seen {
            name: &delivered.name,
            path: &delivered.path,
            topics: delivered.topics.as_deref(),
            properties: delivered.properties.as_ref(),
            visibility: delivered.visibility,
            archived: delivered.archived,
            fork: delivered.fork,
            template: delivered.template,
            language: delivered
                .language
                .as_deref()
                .map_or(Spoken::Unknown, |language| Spoken::Known(Some(language))),
        })
    }

    fn holds(&self, seen: &Seen<'_>) -> bool {
        let named = |globs: &[Glob]| globs.iter().any(|glob| glob.matches(seen.name));
        let flag = |taken: bool, held: Option<bool>| taken || held != Some(true);
        self.names.as_ref().is_none_or(|names| {
            names
                .iter()
                .any(|name| naming(name, (seen.name, seen.path)))
        }) && (self.include.is_empty() || named(&self.include))
            && !named(&self.exclude)
            && seen
                .topics
                .is_none_or(|topics| any_of(&self.topics, topics.iter().map(String::as_str)))
            && match seen.language {
                Spoken::Unknown => true,
                Spoken::Known(language) => any_of(&self.language, language.into_iter()),
            }
            && seen
                .visibility
                .is_none_or(|visibility| self.visibility.contains(&visibility))
            && flag(self.archived, seen.archived)
            && flag(self.fork, seen.fork)
            && flag(self.template, seen.template)
            && seen
                .properties
                .is_none_or(|properties| holds_all(&self.properties, properties))
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
    /// Why its events are withheld, where its project template names the
    /// project another member or source announces as.
    #[serde(default)]
    pub collides: Option<String>,
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
    /// A digest of what the listing asked for: the owner, where, and the
    /// selection; a listing that asks for something else is read whole.
    pub fingerprint: String,
    /// The `ETag` of the member file, where the source reads one through
    /// its forge.
    #[serde(default)]
    pub file: Option<String>,
    /// How far the owner's events were read, where the source follows them.
    #[serde(default)]
    pub events: Option<EventsRead>,
}

/// How far an owner's events feed was read.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EventsRead {
    /// The `ETag` of the feed's first page.
    pub etag: Option<String>,
    /// The id of the newest event read.
    pub newest: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_owner_s_caps_default_to_ten_thousand_listed_a_thousand_kept_for_thirty_days() {
        assert_eq!(MOST_LISTED.get(), 10_000);
        assert_eq!(MOST_MEMBERS.get(), 1_000);
        assert_eq!(MOST_MEMBERS_ALLOWED, 100_000);
        assert_eq!(KEEP_MEMBERS, SignedDuration::from_hours(720));
    }

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
    fn a_member_file_takes_only_the_names_it_lists_by_name_or_path_and_the_filters_narrow_it() {
        let selection = Selection {
            names: Some(vec!["HERALD".to_owned(), "lemonfiber/Gate".to_owned()]),
            exclude: globs(&["gate"]),
            ..Selection::default()
        };
        assert!(selection.takes(&listed("herald")));
        assert!(!selection.takes(&listed("gate")));
        assert!(!selection.takes(&listed("website")));
        assert!(
            Selection {
                names: Some(vec!["lemonfiber/gate".to_owned()]),
                ..Selection::default()
            }
            .takes(&listed("gate"))
        );
        assert!(
            !Selection {
                names: Some(Vec::new()),
                ..Selection::default()
            }
            .takes(&listed("herald"))
        );
    }

    #[test]
    fn a_delivery_is_admitted_by_what_it_names_and_what_it_leaves_out_is_aside() {
        let narrow = Selection {
            include: globs(&["plugin-*"]),
            topics: vec!["media".to_owned()],
            language: vec!["rust".to_owned()],
            properties: BTreeMap::from([("team".to_owned(), vec!["media".to_owned()])]),
            names: Some(vec!["lemonfiber/plugin-a".to_owned()]),
            ..Selection::default()
        };
        let bare = Delivered {
            name: "plugin-a".to_owned(),
            path: "lemonfiber/plugin-a".to_owned(),
            ..Delivered::default()
        };
        assert!(narrow.admits(&bare));
        let full = Delivered {
            topics: Some(vec!["Media".to_owned()]),
            properties: Some(BTreeMap::from([(
                "team".to_owned(),
                vec!["media".to_owned()],
            )])),
            visibility: Some(Visibility::Public),
            archived: Some(false),
            fork: Some(false),
            template: Some(false),
            language: Some("Rust".to_owned()),
            ..bare.clone()
        };
        assert!(narrow.admits(&full));
        for refused in [
            Delivered {
                name: "web".to_owned(),
                ..bare.clone()
            },
            Delivered {
                path: "lemonfiber/plugin-b".to_owned(),
                ..bare.clone()
            },
            Delivered {
                topics: Some(Vec::new()),
                ..bare.clone()
            },
            Delivered {
                properties: Some(BTreeMap::new()),
                ..bare.clone()
            },
            Delivered {
                visibility: Some(Visibility::Private),
                ..bare.clone()
            },
            Delivered {
                archived: Some(true),
                ..bare.clone()
            },
            Delivered {
                fork: Some(true),
                ..bare.clone()
            },
            Delivered {
                template: Some(true),
                ..bare.clone()
            },
            Delivered {
                language: Some("Go".to_owned()),
                ..bare.clone()
            },
        ] {
            assert!(!narrow.admits(&refused), "{refused:?}");
        }
    }

    #[test]
    fn a_repository_is_named_by_its_name_or_its_full_path_ignoring_case() {
        assert!(listed("herald").named("HERALD"));
        assert!(listed("herald").named("Lemonfiber/Herald"));
        assert!(!listed("herald").named("lemonfiber"));
        assert!(!listed("herald").named("other/herald"));
    }

    #[test]
    fn a_repository_s_owner_is_its_path_without_its_name() {
        assert_eq!(listed("herald").owner(), "lemonfiber");
        let nested = Listed {
            path: "group/sub/herald".to_owned(),
            ..listed("herald")
        };
        assert_eq!(nested.owner(), "group/sub");
        let bare = Listed {
            path: "herald".to_owned(),
            ..listed("herald")
        };
        assert_eq!(bare.owner(), "herald");
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
