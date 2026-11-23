//! A repository's milestones, each announced as the `milestone` signal
//! (ADR-0079): delivered by webhook, or found by a poll that follows the
//! open ones and reads each that left the listing once; only a poll finds
//! one overdue, by its own clock (ADR-0080).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use herald_core::{
    ACTION, Attribute, Cursor, DUE, Draft, Event, ISSUES_CLOSED, ISSUES_OPEN, Kind, MILESTONE_LINK,
    NUMBER, PrereleaseRule, SIGNAL, Signal, Validators, versioned,
};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Request};
use herald_wire::SourceError;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan as _};

use crate::forge::Repository;
use crate::limited::checked;
use crate::listing::{Asked, Named};
use crate::options::{MilestoneAction, MilestoneChoice};
use crate::runs::Found;
use crate::{Forge, Origin, Token};

mod github;
mod gitlab;

/// The most bytes of a milestone's description its event carries as its
/// body.
const DESCRIPTION_MOST: usize = 4_096;

/// A milestone, as a forge describes it at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Milestone {
    /// What the forge reads it by: GitHub's number, GitLab's and Forgejo's
    /// id.
    pub(crate) key: u64,
    /// The number people know it by: GitHub's number, GitLab's iid,
    /// Forgejo's id.
    pub(crate) number: u64,
    /// The GitLab group it is of, where it is a group's.
    pub(crate) group: Option<u64>,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) open: bool,
    pub(crate) due: Option<String>,
    /// How many of its issues are open and closed, where the forge said.
    pub(crate) counts: Option<(u32, u32)>,
    pub(crate) created: Timestamp,
    pub(crate) updated: Option<Timestamp>,
    pub(crate) closed: Option<Timestamp>,
    /// Its page, where the forge names it.
    pub(crate) url: Option<String>,
    /// Its page under its repository's, where the forge names none.
    pub(crate) path: String,
}

impl Milestone {
    /// Its due day, where it has one.
    fn due_day(&self) -> Option<Date> {
        self.due.as_deref().and_then(|due| due.parse().ok())
    }

    /// When it closed: the forge's own mark, else its last change.
    fn closed_at(&self) -> Timestamp {
        self.closed.or(self.updated).unwrap_or(self.created)
    }

    /// When it did `action`.
    fn occurred(&self, action: MilestoneAction) -> Timestamp {
        match action {
            MilestoneAction::Opened => self.created,
            MilestoneAction::Closed => self.closed_at(),
            MilestoneAction::Reopened | MilestoneAction::Deleted => {
                self.updated.unwrap_or(self.created)
            }
            MilestoneAction::Overdue => self
                .due_day()
                .and_then(|due| due.checked_add(1.day()).ok())
                .and_then(|day| day.to_zoned(TimeZone::UTC).ok())
                .map_or(self.created, |day| day.timestamp()),
        }
    }

    /// What its identity says it did: a deletion takes the identity of
    /// the opening or closing it withdraws.
    fn acted(&self, action: MilestoneAction) -> String {
        match action {
            MilestoneAction::Opened => action.name().to_owned(),
            MilestoneAction::Deleted if self.open => MilestoneAction::Opened.name().to_owned(),
            MilestoneAction::Deleted | MilestoneAction::Closed => format!(
                "{}/{}",
                MilestoneAction::Closed.name(),
                self.closed_at().as_second()
            ),
            MilestoneAction::Reopened => {
                format!("{}/{}", action.name(), self.occurred(action).as_second())
            }
            MilestoneAction::Overdue => {
                format!(
                    "{}/{}",
                    action.name(),
                    self.due.as_deref().unwrap_or_default()
                )
            }
        }
    }
}

/// A milestone that did something a source may announce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgeMilestone {
    pub(crate) milestone: Milestone,
    pub(crate) action: MilestoneAction,
}

impl ForgeMilestone {
    /// Whether `choice` announces it.
    pub(crate) fn chosen(&self, choice: &MilestoneChoice) -> bool {
        (self.milestone.group.is_none() || choice.group)
            && choice.takes(self.action, &self.milestone.title)
    }

    /// The event it makes as a milestone of `repository`, read as `what`,
    /// its counts where `counts` says so; a deletion is marked withdrawn.
    pub(crate) fn event(
        self,
        what: &'static str,
        repository: &Repository,
        counts: bool,
    ) -> Result<Event, SourceError> {
        let Self { milestone, action } = self;
        let url = milestone.url.clone().or_else(|| {
            repository
                .page
                .as_ref()
                .map(|page| format!("{}{}", page.trim_end_matches('/'), milestone.path))
        });
        let host = crate::wire::host_named(what, repository, url.as_deref().unwrap_or_default())?;
        let holder = milestone.group.map_or_else(
            || format!("{host}/{}", repository.id),
            |group| format!("{host}/group/{group}"),
        );
        let identity = format!(
            "{holder}/milestone/{}/{}",
            milestone.key,
            milestone.acted(action)
        );
        let counted = milestone.counts.filter(|_| counts);
        let mut title = format!("Milestone {} {}", milestone.title, action.name());
        if let Some((open, closed)) = counted {
            let _ = write!(
                title,
                ": {closed} of {} closed",
                u64::from(open) + u64::from(closed)
            );
        }
        if let Some(due) = &milestone.due {
            let _ = write!(title, ", due {due}");
        }
        let prerelease = PrereleaseRule::Tag.marks(None, &milestone.title);
        let mut attributes = versioned(milestone.title.clone(), prerelease);
        attributes.extend([
            (
                SIGNAL.to_owned(),
                Attribute::Text(Signal::Milestone.name().to_owned()),
            ),
            (ACTION.to_owned(), Attribute::Text(action.name().to_owned())),
            (
                NUMBER.to_owned(),
                Attribute::Number(milestone.number.into()),
            ),
            (
                MILESTONE_LINK.to_owned(),
                Attribute::Text(milestone.title.clone()),
            ),
        ]);
        if let Some(due) = &milestone.due {
            attributes.insert(DUE.to_owned(), Attribute::Text(due.clone()));
        }
        if let Some((open, closed)) = counted {
            attributes.insert(ISSUES_OPEN.to_owned(), Attribute::Number(open.into()));
            attributes.insert(ISSUES_CLOSED.to_owned(), Attribute::Number(closed.into()));
        }
        let occurred = milestone.occurred(action);
        let mut body = milestone.description.unwrap_or_default();
        body.truncate(body.floor_char_boundary(DESCRIPTION_MOST));
        let draft = Draft {
            identity,
            kind: Kind::MILESTONE,
            project: repository.name.clone(),
            title,
            body,
            links: url
                .map(|url| BTreeMap::from([(MILESTONE_LINK.to_owned(), url)]))
                .unwrap_or_default(),
            attributes,
            occurred,
        };
        let event =
            Event::normalise(draft).map_err(|problem| SourceError::Invalid { what, problem })?;
        Ok(if action == MilestoneAction::Deleted {
            event.marked_withdrawn()
        } else {
            event
        })
    }
}

/// What a milestone's webhook delivery is read as on each forge.
const HOOKED: Named = Named {
    github: "GitHub's milestone event",
    gitlab: "GitLab's milestone event",
    forgejo: "Forgejo's milestone event",
};

/// What a listing of milestones is read as on each forge.
const LISTED: Named = Named {
    github: "GitHub's milestones",
    gitlab: "GitLab's milestones",
    forgejo: "Forgejo's milestones",
};

/// What one milestone is read as on each forge.
const ONE: Named = Named {
    github: "GitHub's milestone",
    gitlab: "GitLab's milestone",
    forgejo: "Forgejo's milestone",
};

/// What a milestone's webhook delivery is read as on `forge`.
pub(crate) const fn hooked_what(forge: Forge) -> &'static str {
    HOOKED.on(forge)
}

/// Whether the event `name` on `forge` is of a milestone: Forgejo and Gitea
/// send none.
pub(crate) fn hooked(forge: Forge, name: &str) -> bool {
    match forge {
        Forge::GitHub => name == github::HOOK,
        Forge::GitLab => name == gitlab::HOOK,
        Forge::Forgejo => false,
    }
}

/// What a milestone's webhook delivery names: the repository's full path,
/// and the milestone's event where `choice` announces it.
#[derive(Debug)]
pub(crate) struct Delivered {
    pub(crate) path: String,
    pub(crate) event: Option<Event>,
}

/// What the milestone delivery `payload` on `forge` names, its event where
/// `choice` announces it; `None` where it names no repository, as a GitLab
/// group's own webhook does.
///
/// # Errors
///
/// When the payload is not the milestone event it is named, or its
/// milestone makes no event.
pub(crate) fn delivered(
    forge: Forge,
    payload: &[u8],
    choice: &MilestoneChoice,
) -> Result<Option<Delivered>, SourceError> {
    let what = hooked_what(forge);
    let read = match forge {
        Forge::GitLab => gitlab::delivered(payload)?,
        Forge::GitHub | Forge::Forgejo => Some(github::delivered(what, payload)?),
    };
    let Some((path, repository, milestone)) = read else {
        return Ok(None);
    };
    let event = milestone
        .filter(|milestone| milestone.chosen(choice))
        .map(|milestone| milestone.event(what, &repository, choice.counts))
        .transpose()?;
    Ok(Some(Delivered { path, event }))
}

/// Whether a CI run's own event of the name `name` on `forge` is of a
/// milestone.
#[must_use]
pub fn of_a_milestone(forge: Forge, name: &str) -> bool {
    hooked(forge, name)
}

/// What a CI run's own milestone event `payload` announces for a source of
/// `options` on `forge`: the milestone's event, from its repository, where
/// its `milestones` choose it; nothing otherwise.
///
/// # Errors
///
/// When the payload is not a milestone event, or its milestone makes no
/// event.
pub fn ci_milestone(
    (forge, options): (Forge, &crate::SourceOptions),
    payload: &[u8],
) -> Result<Option<Event>, SourceError> {
    let found = delivered(forge, payload, &options.milestone_choice())?;
    let Some(event) = found.and_then(|found| found.event) else {
        return Ok(None);
    };
    let delivered = crate::delivered::delivered(forge, hooked_what(forge), payload)?;
    let host = crate::forge::host_of(forge, &crate::forge::api_of(forge, options));
    Ok(Some(event.from_origin(crate::delivered::origin(
        forge, &host, delivered,
    ))))
}

/// The milestones a poll follows: a repository's own, or those of the
/// GitLab group holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listing {
    Own,
    Group,
}

impl Listing {
    /// The name a poll keeps its first page's validators and open
    /// milestones under; each further page's adds `/<page>`, and the ids
    /// of those it saw close `/closed`.
    const fn kept(self) -> &'static str {
        match self {
            Self::Own => "milestone",
            Self::Group => "milestone-group",
        }
    }
}

/// The ids `cursor` keeps under `key`, where it keeps any.
fn kept_ids(cursor: Option<&Cursor>, key: &str) -> Option<Vec<u64>> {
    cursor
        .and_then(|cursor| cursor.ids.get(key))
        .map(|ids| ids.iter().filter_map(|id| id.parse().ok()).collect())
}

/// `ids`, as a cursor keeps them.
fn written(ids: &[u64]) -> Vec<String> {
    ids.iter().map(u64::to_string).collect()
}

/// The day of each id `cursor` keeps under `key`, as `<id>:<day>`.
fn kept_days(cursor: Option<&Cursor>, key: &str) -> BTreeMap<u64, Date> {
    cursor
        .and_then(|cursor| cursor.ids.get(key))
        .into_iter()
        .flatten()
        .filter_map(|kept| {
            let (id, day) = kept.split_once(':')?;
            Some((id.parse().ok()?, day.parse().ok()?))
        })
        .collect()
}

/// `days`, as a cursor keeps them.
fn written_days(days: &BTreeMap<u64, Date>) -> Vec<String> {
    days.iter().map(|(id, day)| format!("{id}:{day}")).collect()
}

/// What a poll found of a repository's milestones: the validators and ids
/// to keep, whether any listing changed, and each milestone that did
/// something since.
#[derive(Debug, Default)]
pub(crate) struct Followed {
    listings: BTreeMap<String, Validators>,
    ids: BTreeMap<String, Vec<String>>,
    pub(crate) changed: bool,
    found: Vec<ForgeMilestone>,
}

/// How a repository's milestones are polled.
pub(crate) struct Asking<'a> {
    pub(crate) origin: &'a Origin,
    pub(crate) token: Option<&'a Token>,
    pub(crate) choice: &'a MilestoneChoice,
}

impl Asking<'_> {
    /// The request for `path` under what `listing` names.
    fn request(&self, listing: Listing, path: &str) -> Result<Request, Unpolled> {
        match listing {
            Listing::Own => self.origin.get(path, self.token),
            Listing::Group => gitlab::group_request(self.origin, self.token, path),
        }
    }

    /// What a poll at `now` since `cursor` finds through `http` of the
    /// milestones it follows: each listing asked conditional on what
    /// `cursor` keeps, a page at a time up to `most`; each milestone that
    /// left a listing read once, as closed where it closed; each open one
    /// whose due day ended before `now` overdue; and on GitLab the counts
    /// of each one announced, where they are read.
    pub(crate) async fn asked(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        cursor: Option<&Cursor>,
        now: Timestamp,
    ) -> Result<Followed, Unpolled> {
        let mut followed = Followed::default();
        let group = self.choice.group && self.origin.forge() == Forge::GitLab;
        for listing in [Some(Listing::Own), group.then_some(Listing::Group)]
            .into_iter()
            .flatten()
        {
            self.follow(http, (cursor, now), listing, &mut followed)
                .await?;
        }
        Ok(followed)
    }

    /// `followed` with what `listing` finds at `now` since `cursor`.
    async fn follow(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (cursor, now): (Option<&Cursor>, Timestamp),
        listing: Listing,
        followed: &mut Followed,
    ) -> Result<(), Unpolled> {
        let forge = self.origin.forge();
        let what = LISTED.on(forge);
        let reading = self.origin.reading();
        let size = reading.page.min(self.choice.most).max(1);
        let key = listing.kept();
        let mut open: Vec<u64> = Vec::new();
        let mut listed: BTreeMap<u64, Milestone> = BTreeMap::new();
        let pages = self.choice.most.div_ceil(size);
        for page in 1..=pages {
            let named = if page == 1 {
                key.to_owned()
            } else {
                format!("{key}/{page}")
            };
            let path = match forge {
                Forge::GitHub => format!("/milestones?state=open&per_page={size}&page={page}"),
                Forge::GitLab => format!("/milestones?state=active&per_page={size}&page={page}"),
                Forge::Forgejo => format!("/milestones?state=open&limit={size}&page={page}"),
            };
            let request = self.request(listing, &path)?;
            let asked = Asked::requested(http, (forge, request), (cursor, &named)).await?;
            let (validators, body) = asked.read(what)?;
            followed.changed |= asked.changed();
            let mut ids = match body {
                Some(body) => {
                    let read = read_listed(forge, what, reading.parse, body)
                        .map_err(Unpolled::unreadable)?;
                    let ids: Vec<u64> = read.iter().map(|milestone| milestone.key).collect();
                    listed.extend(read.into_iter().map(|milestone| (milestone.key, milestone)));
                    ids
                }
                None => kept_ids(cursor, &named).unwrap_or_default(),
            };
            let full = ids.len() >= size;
            ids.truncate(self.choice.most - open.len());
            open.extend(&ids);
            followed.listings.insert(named.clone(), validators);
            followed.ids.insert(named, written(&ids));
            if !full || open.len() >= self.choice.most {
                break;
            }
        }
        let current: BTreeSet<u64> = open.iter().copied().collect();
        self.overdue(http, (cursor, now), (listing, &current, &listed), followed)
            .await?;
        let closed_key = format!("{key}/closed");
        let mut closed = kept_ids(cursor, &closed_key).unwrap_or_default();
        if let Some(kept) = kept_ids(cursor, key).filter(|_| followed.changed) {
            let mut was: BTreeSet<u64> = kept.into_iter().collect();
            for page in 2..=pages {
                match kept_ids(cursor, &format!("{key}/{page}")) {
                    Some(ids) => was.extend(ids),
                    None => break,
                }
            }
            let appeared = listed
                .into_iter()
                .filter(|(id, _)| current.contains(id) && !was.contains(id));
            for (id, milestone) in appeared {
                let reopened = closed.contains(&id) || milestone.closed.is_some();
                closed.retain(|closed| *closed != id);
                let action = if reopened {
                    MilestoneAction::Reopened
                } else {
                    MilestoneAction::Opened
                };
                self.found(
                    http,
                    listing,
                    ForgeMilestone { milestone, action },
                    followed,
                )
                .await?;
            }
            for id in was.into_iter().filter(|id| !current.contains(id)) {
                let Some(milestone) = self.read_one(http, listing, id).await? else {
                    continue;
                };
                if milestone.open {
                    continue;
                }
                closed.retain(|closed| *closed != id);
                closed.insert(0, id);
                let action = MilestoneAction::Closed;
                self.found(
                    http,
                    listing,
                    ForgeMilestone { milestone, action },
                    followed,
                )
                .await?;
            }
        }
        closed.truncate(self.choice.most);
        followed.ids.insert(closed_key, written(&closed));
        Ok(())
    }

    /// `followed` with each milestone of `open`, those `listing` names,
    /// whose due day ended before `now`, once a due day, where the source
    /// announces it overdue: read from `listed`, those listed this round,
    /// else once by its own path. The due days of `open` are kept, and of
    /// them those already past, whatever the source announces, so a first
    /// poll, or one that newly announces `overdue`, finds only the days
    /// that pass after it.
    async fn overdue(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (cursor, now): (Option<&Cursor>, Timestamp),
        (listing, open, listed): (Listing, &BTreeSet<u64>, &BTreeMap<u64, Milestone>),
        followed: &mut Followed,
    ) -> Result<(), Unpolled> {
        let key = listing.kept();
        let (due_key, overdue_key) = (format!("{key}/due"), format!("{key}/overdue"));
        let mut dues = kept_days(cursor, &due_key);
        for (id, milestone) in listed {
            match milestone.due_day() {
                Some(day) => dues.insert(*id, day),
                None => dues.remove(id),
            };
        }
        dues.retain(|id, _| open.contains(id));
        let mut overdue = kept_days(cursor, &overdue_key);
        overdue.retain(|id, day| dues.get(id) == Some(day));
        let today = now.to_zoned(TimeZone::UTC).date();
        let first = kept_ids(cursor, key).is_none();
        let announced = !first && self.choice.actions.contains(&MilestoneAction::Overdue);
        let passed: Vec<(u64, Date)> = dues
            .iter()
            .filter(|(id, day)| **day < today && overdue.get(id) != Some(day))
            .map(|(id, day)| (*id, *day))
            .collect();
        for (id, day) in passed {
            overdue.insert(id, day);
            followed.changed = true;
            if !announced {
                continue;
            }
            let read = match listed.get(&id) {
                Some(milestone) => Some(milestone.clone()),
                None => self.read_one(http, listing, id).await?,
            };
            let Some(milestone) = read.filter(|read| read.open && read.due_day() == Some(day))
            else {
                continue;
            };
            let action = MilestoneAction::Overdue;
            self.found(
                http,
                listing,
                ForgeMilestone { milestone, action },
                followed,
            )
            .await?;
        }
        followed.ids.insert(due_key, written_days(&dues));
        followed.ids.insert(overdue_key, written_days(&overdue));
        Ok(())
    }

    /// The milestone `key` of what `listing` names, read through `http`;
    /// `None` where the forge no longer has it.
    async fn read_one(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        listing: Listing,
        key: u64,
    ) -> Result<Option<Milestone>, Unpolled> {
        let forge = self.origin.forge();
        let what = ONE.on(forge);
        let request = self.request(listing, &format!("/milestones/{key}"))?;
        let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
        if answer.status == herald_core::status::NOT_FOUND {
            return Ok(None);
        }
        checked(forge, what, &answer)?;
        read_one(forge, what, self.origin.reading().parse, &answer.body)
            .map(Some)
            .map_err(Unpolled::unreadable)
    }

    /// `followed` with `found`, where the source announces it, its counts
    /// read through `http` where GitLab names none and they are read.
    async fn found(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        listing: Listing,
        mut found: ForgeMilestone,
        followed: &mut Followed,
    ) -> Result<(), Unpolled> {
        if !found.chosen(self.choice) {
            return Ok(());
        }
        let forge = self.origin.forge();
        if forge == Forge::GitLab && self.choice.counts {
            let path = gitlab::statistics_path(&found.milestone.title);
            let answer = http
                .send(self.request(listing, &path)?)
                .await
                .map_err(Unpolled::unreachable)?;
            let what = gitlab::STATISTICS;
            checked(Forge::GitLab, what, &answer)?;
            found.milestone.counts = Some(
                gitlab::counted(self.origin.reading().parse, &answer.body)
                    .map_err(Unpolled::unreadable)?,
            );
        }
        followed.found.push(found);
        Ok(())
    }

    /// The events of `repository` that `followed` found, and what to keep.
    pub(crate) fn read(
        &self,
        followed: Followed,
        repository: &Repository,
    ) -> Result<Found, Unpolled> {
        let what = LISTED.on(self.origin.forge());
        let events = followed
            .found
            .into_iter()
            .map(|found| found.event(what, repository, self.choice.counts))
            .collect::<Result<_, _>>()
            .map_err(Unpolled::unreadable)?;
        Ok(Found {
            events,
            validators: None,
            listings: followed.listings,
            ids: followed.ids,
        })
    }
}

/// The milestones in `forge`'s listing `body`, read as `what`, parsing no
/// more than `limit` bytes.
fn read_listed(
    forge: Forge,
    what: &'static str,
    limit: usize,
    body: &[u8],
) -> Result<Vec<Milestone>, SourceError> {
    match forge {
        Forge::GitLab => gitlab::listed(what, limit, body),
        Forge::GitHub | Forge::Forgejo => github::listed(forge, what, limit, body),
    }
}

/// The one milestone `forge`'s `body` describes, read as `what`, parsing no
/// more than `limit` bytes.
fn read_one(
    forge: Forge,
    what: &'static str,
    limit: usize,
    body: &[u8],
) -> Result<Milestone, SourceError> {
    match forge {
        Forge::GitLab => gitlab::one(what, limit, body),
        Forge::GitHub | Forge::Forgejo => github::one(forge, what, limit, body),
    }
}

#[cfg(test)]
mod tests;
