//! A release's header: the open milestones its milestone selector names,
//! as each repository holding them describes them, the open issues they
//! carry where those are the board's rows, and each repository whose
//! milestones could not be read, with why (ADR-0082, ADR-0084); read
//! across every repository an owner view holds, and the GitLab groups
//! above them, each seen as its own (ADR-0086).

use std::collections::BTreeMap;

use jiff::civil::Date;

use super::overview::Unread;
use super::{Pull, View};
use crate::members::{Listed, Visibility};

/// The most repositories and GitLab groups a release's header reads the
/// milestones of.
pub const MOST_HEADER_REPOSITORIES: usize = 20;

/// The most milestones a release's header names of one repository.
pub const MOST_HEADER_MILESTONES: usize = 10;

/// The most open issues a release's rows name of one repository.
pub const MOST_ISSUE_ROWS: usize = 50;

/// What a release reads of each open milestone its selector matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderRead {
    /// Whether its header names the milestone.
    pub header: bool,
    /// Whether the milestone's open issues are the board's rows.
    pub issues: bool,
}

/// One open milestone a release's selector names, in one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenMilestone {
    /// The repository's full path.
    pub repository: String,
    /// Who may see the repository.
    pub visibility: Visibility,
    /// Its title.
    pub title: String,
    /// Its page, where the forge names one.
    pub url: Option<String>,
    /// Its due day, where it has one.
    pub due: Option<Date>,
    /// Whether its due day ended before the day it was read on, in UTC.
    pub overdue: bool,
    /// How many of its issues are open and closed, as its forge counts
    /// them, where the forge said.
    pub counts: Option<(u32, u32)>,
}

/// One open issue carrying a milestone a release's selector names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenIssue {
    /// The repository's full path.
    pub repository: String,
    /// Who may see it: its repository's visibility, private where the
    /// issue is confidential.
    pub visibility: Visibility,
    /// Its number.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Its page.
    pub url: String,
    /// The title of the milestone it carries.
    pub milestone: String,
}

/// A release's header, as one surface is shown it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    /// The milestones, in the order their repositories were read.
    pub milestones: Vec<OpenMilestone>,
    /// The open issues the milestones carry, where they are the board's
    /// rows, in the order their repositories were read.
    pub issues: Option<Vec<OpenIssue>>,
    /// Each repository whose milestones could not be read, or wholly.
    pub unread: Vec<Unread>,
}

impl Header {
    /// It with `other`'s milestones, issues and unread repositories added.
    pub fn merge(&mut self, other: Self) {
        self.milestones.extend(other.milestones);
        if let Some(issues) = other.issues {
            self.issues.get_or_insert_default().extend(issues);
        }
        self.unread.extend(other.unread);
    }

    /// It with only what `shows`, given a repository's visibility, lets
    /// out.
    #[must_use]
    pub fn shown(&self, shows: impl Fn(Visibility) -> bool) -> Self {
        Self {
            milestones: self
                .milestones
                .iter()
                .filter(|milestone| shows(milestone.visibility))
                .cloned()
                .collect(),
            issues: self.issues.as_ref().map(|issues| {
                issues
                    .iter()
                    .filter(|issue| shows(issue.visibility))
                    .cloned()
                    .collect()
            }),
            unread: self
                .unread
                .iter()
                .filter(|unread| shows(unread.visibility))
                .cloned()
                .collect(),
        }
    }

    /// It with only the repositories `view` holds.
    #[must_use]
    pub fn within(mut self, view: &View) -> Self {
        self.milestones
            .retain(|milestone| view.holds(&milestone.repository));
        if let Some(issues) = self.issues.as_mut() {
            issues.retain(|issue| view.holds(&issue.repository));
        }
        self.unread.retain(|unread| view.holds(&unread.repository));
        self
    }

    /// Who may see the strictest repository it names.
    #[must_use]
    pub fn strictest(&self) -> Option<Visibility> {
        let milestones = self.milestones.iter().map(|milestone| milestone.visibility);
        let issues = self.issues.iter().flatten().map(|issue| issue.visibility);
        let unread = self.unread.iter().map(|unread| unread.visibility);
        milestones.chain(issues).chain(unread).max()
    }
}

/// One repository, or GitLab group whose own milestones are read, that a
/// release's header asks to read (ADR-0086).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headed {
    /// The source reading it, by its place among those its view reads.
    pub source: usize,
    /// Its full path.
    pub path: String,
    /// Who may see what is read of it.
    pub visibility: Visibility,
    /// Whether it holds a pull request of the release.
    pub picked: bool,
    /// Whether it is a GitLab group.
    pub group: bool,
}

/// What the source at `source` asks `view`'s release header to read, where
/// its release has a header or issue rows: each
/// repository of `pulls` the release picks, seen as its rows are; the one
/// repository `own` it reads, seen as private; each member of `members`
/// as listed; and where `groups` names the owner whose GitLab group
/// milestones it reads, each group at or under it above a member `view`
/// holds (ADR-0086).
#[must_use]
pub fn header_asked(
    view: &View,
    (source, groups): (usize, Option<&str>),
    (pulls, own, members): (&[Pull], Option<&str>, &[Listed]),
) -> Vec<Headed> {
    let repository = |path: &str, visibility: Visibility, picked: bool| Headed {
        source,
        path: path.to_owned(),
        visibility,
        picked,
        group: false,
    };
    let Some(release) = view
        .release
        .as_ref()
        .filter(|release| release.heading().is_some())
    else {
        return Vec::new();
    };
    let picked = pulls
        .iter()
        .filter(|pull| release.picks(pull))
        .map(|pull| repository(&pull.repository, pull.visibility, true));
    let own = own.map(|path| repository(path, Visibility::Private, false));
    let listed = members
        .iter()
        .map(|member| repository(&member.path, member.visibility, false));
    let grouped = groups
        .into_iter()
        .flat_map(|owner| grouped(view, (owner, members)))
        .map(|(path, visibility)| Headed {
            source,
            path,
            visibility,
            picked: false,
            group: true,
        });
    picked.chain(own).chain(listed).chain(grouped).collect()
}

/// The GitLab groups at or under `owner` above a member of `members` that
/// `view` holds, each once as the members name it, seen as the strictest
/// member under it, held or not, since its milestones count the issues of
/// every project under it.
fn grouped(view: &View, (owner, members): (&str, &[Listed])) -> Vec<(String, Visibility)> {
    let mut groups: BTreeMap<String, (String, Visibility)> = BTreeMap::new();
    for member in members.iter().filter(|member| view.holds(&member.path)) {
        let mut path = member.path.as_str();
        while let Some((group, _)) = path.rsplit_once('/') {
            if group.eq_ignore_ascii_case(owner) || under(group, owner) {
                groups
                    .entry(group.to_ascii_lowercase())
                    .or_insert_with(|| (group.to_owned(), strictest(group, members)));
            }
            path = group;
        }
    }
    groups.into_values().collect()
}

/// The strictest visibility of the members of `members` under the group at
/// `group`, private where none is.
fn strictest(group: &str, members: &[Listed]) -> Visibility {
    members
        .iter()
        .filter(|member| under(&member.path, group))
        .map(|member| member.visibility)
        .max()
        .unwrap_or(Visibility::Private)
}

/// Whether the full path `path` lies under the group at `group`, ignoring
/// case.
fn under(path: &str, group: &str) -> bool {
    path.to_ascii_lowercase()
        .starts_with(&format!("{}/", group.to_ascii_lowercase()))
}

/// Which of `asked` `view`'s release header reads, in the order read, and
/// each of those past [`MOST_HEADER_REPOSITORIES`] named unread with why:
/// each repository `view` and its release hold, and each group where the
/// release is held to no one repository, once, seen as the least strict
/// it was asked as; public before internal before private, so the bound
/// never spends what an audience may see on what it may not, those holding
/// a pull request of the release first, then by path (ADR-0086).
#[must_use]
pub fn heading(view: &View, asked: Vec<Headed>) -> (Vec<Headed>, Vec<Unread>) {
    let release = view.release.as_ref();
    let mut once: BTreeMap<(bool, String), Headed> = BTreeMap::new();
    for headed in asked.into_iter().filter(|headed| {
        if headed.group {
            release.is_none_or(|release| release.repository.is_none())
        } else {
            view.holds(&headed.path) && release.is_none_or(|release| release.takes(&headed.path))
        }
    }) {
        let key = (headed.group, headed.path.to_ascii_lowercase());
        let kept = once.entry(key).or_insert_with(|| headed.clone());
        kept.visibility = kept.visibility.min(headed.visibility);
        kept.picked |= headed.picked;
    }
    let mut read: Vec<Headed> = once.into_values().collect();
    read.sort_by_cached_key(|headed| {
        (
            headed.visibility,
            !headed.picked,
            headed.path.to_ascii_lowercase(),
            headed.group,
        )
    });
    let past = read.split_off(read.len().min(MOST_HEADER_REPOSITORIES));
    let unread = past
        .into_iter()
        .map(|headed| Unread {
            repository: headed.path,
            visibility: headed.visibility,
            reason: format!(
                "a release's header reads the milestones of at most {MOST_HEADER_REPOSITORIES} repositories and groups"
            ),
        })
        .collect();
    (read, unread)
}

impl Header {
    /// `self`, read as `headed` asked: where a GitLab group's, each issue
    /// seen as the strictest of how it was read and the member of
    /// `members` holding it, private where none does, and kept only where
    /// `view` holds its repository (ADR-0086).
    #[must_use]
    pub fn read_as(mut self, view: &View, (headed, members): (&Headed, &[Listed])) -> Self {
        if !headed.group {
            return self;
        }
        if let Some(issues) = self.issues.as_mut() {
            issues.retain(|issue| view.holds(&issue.repository));
            for issue in issues {
                let member = members
                    .iter()
                    .find(|member| member.path.eq_ignore_ascii_case(&issue.repository));
                let seen = member.map_or(Visibility::Private, |member| member.visibility);
                issue.visibility = issue.visibility.max(seen);
            }
        }
        self
    }
}

#[cfg(test)]
mod tests;
