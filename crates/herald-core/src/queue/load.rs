//! A view's load overview: the review requests open now and the reviews
//! given over a window, per team and per area, never per person; a team or
//! an area of fewer people than its view's least team folded into one
//! "other", shown only where the people folded into it are as many
//! (ADR-0075).

use std::collections::{BTreeMap, BTreeSet};

use jiff::{SignedDuration, Timestamp};

use super::area::unnamed;
use super::{Area, AreaKind, Pull, Reviewed, Row};
use crate::report::DAY;

/// The fewest people a team or an area is shown by name with where a view
/// names no number, and the fewest a view may name: below it one row's load
/// could be one person's.
pub const LEAST_TEAM: usize = 3;

/// The most people a view's least team may name.
pub const MOST_LEAST_TEAM: usize = 50;

/// How far back reviews given count where a view names no window.
pub const LOAD_WINDOW: SignedDuration = SignedDuration::from_hours(7 * 24);

/// The shortest window a view's load counts reviews given over.
pub const SHORTEST_LOAD_WINDOW: SignedDuration = DAY;

/// The longest window a view's load counts reviews given over: a review is
/// kept only as long as its pull request's record.
pub const LONGEST_LOAD_WINDOW: SignedDuration = SignedDuration::from_hours(90 * 24);

/// The most teams a config names: each is matched against every review
/// request and review of every row a view shows.
pub const MOST_TEAMS: usize = 200;

/// The most members one team names.
pub const MOST_TEAM_MEMBERS: usize = 500;

/// The most characters a member's login holds, GitLab's longest username.
pub const MOST_LOGIN: usize = 255;

/// How a view counts its load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Load {
    /// How far back reviews given count.
    pub window: SignedDuration,
    /// The fewest people a team or an area is shown by name with.
    pub least_team: usize,
    /// Whether a team, group or role area whose members no team names is
    /// shown as it is, though it may be one person.
    pub unknown_teams: bool,
}

impl Default for Load {
    fn default() -> Self {
        Self {
            window: LOAD_WINDOW,
            least_team: LEAST_TEAM,
            unknown_teams: false,
        }
    }
}

/// A team's member: a login, on one forge host where it names one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TeamMember {
    login: String,
    host: Option<String>,
}

impl TeamMember {
    /// The member `written` names, `login` or `login@host`, each compared
    /// without case.
    ///
    /// # Errors
    ///
    /// Where it is no login, or names an empty host.
    pub fn of(written: &str) -> Result<Self, String> {
        let (login, host) = match written.rsplit_once('@') {
            Some((login, host)) => (login, Some(host)),
            None => (written, None),
        };
        let refused = |why: &str| Err(format!("\"{}\" {why}", written.escape_debug()));
        if login.is_empty() || login.contains('@') {
            return refused("is no login; give login or login@host");
        }
        if login.chars().count() > MOST_LOGIN {
            return refused(&format!("is longer than {MOST_LOGIN} characters"));
        }
        let bare = |text: &str| {
            text.chars()
                .all(|c| !c.is_whitespace() && !c.is_control() && c != '/')
        };
        if !bare(login) {
            return refused("holds a space, a slash or a control character");
        }
        if host.is_some_and(|host| host.is_empty() || !bare(host)) {
            return refused("names no host after its @; give login@host");
        }
        Ok(Self {
            login: login.to_lowercase(),
            host: host.map(str::to_lowercase),
        })
    }

    /// Whether it is `login` on a forge at `host`.
    fn is(&self, login: &str, host: Option<&str>) -> bool {
        self.login.eq_ignore_ascii_case(login)
            && self
                .host
                .as_deref()
                .is_none_or(|own| host.is_some_and(|host| host.eq_ignore_ascii_case(own)))
    }
}

/// A team the config names: its members.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Team {
    members: BTreeSet<TeamMember>,
}

impl Team {
    /// The team `name` naming `members`.
    ///
    /// # Errors
    ///
    /// Where `name` names no team, it names past [`MOST_TEAM_MEMBERS`]
    /// members, or one is no login.
    pub fn of(name: &str, members: &[String]) -> Result<Self, String> {
        if let Some(reason) = unnamed(name) {
            return Err(reason.replace("area name", "team name"));
        }
        if members.len() > MOST_TEAM_MEMBERS {
            return Err(format!(
                "a team names at most {MOST_TEAM_MEMBERS} members; split it"
            ));
        }
        let members = members
            .iter()
            .map(|written| TeamMember::of(written))
            .collect::<Result<_, _>>()?;
        Ok(Self { members })
    }

    /// The people it holds, each login once whatever host it is on.
    #[must_use]
    pub fn people(&self) -> BTreeSet<&str> {
        self.members
            .iter()
            .map(|member| member.login.as_str())
            .collect()
    }

    /// Whether `login` on a forge at `host` is one of its members.
    fn holds(&self, login: &str, host: Option<&str>) -> bool {
        self.members.iter().any(|member| member.is(login, host))
    }
}

/// The teams a config names, by name.
pub type Teams = BTreeMap<String, Team>;

/// `name` as a team, an area and a review request compare it: without its
/// leading `@`s and case.
fn bare(name: &str) -> String {
    name.trim_start_matches('@').to_lowercase()
}

/// The reviews kept mode saw given on each open pull request a view read,
/// each reviewer's newest, by the pull request's page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Given {
    reviews: BTreeMap<String, Vec<(String, Timestamp)>>,
    /// Whether a source of the view was read live, so its reviews given are
    /// not known.
    pub live: bool,
}

impl Given {
    /// It with `reviews`, given on `pull`, added.
    pub fn add(&mut self, pull: &Pull, reviews: &BTreeMap<String, Reviewed>) {
        let given = reviews
            .iter()
            .map(|(login, reviewed)| (login.clone(), reviewed.at));
        self.reviews
            .entry(pull.url.clone())
            .or_default()
            .extend(given);
    }

    /// It with `other`'s reviews added, live where either is.
    pub fn merge(&mut self, other: Self) {
        for (url, reviews) in other.reviews {
            self.reviews.entry(url).or_default().extend(reviews);
        }
        self.live |= other.live;
    }

    /// The reviewers of `pull` who gave a review at or after `since`.
    fn of(&self, pull: &Pull, since: Timestamp) -> impl Iterator<Item = &str> {
        self.reviews
            .get(&pull.url)
            .into_iter()
            .flatten()
            .filter(move |(_, at)| *at >= since)
            .map(|(login, _)| login.as_str())
    }
}

/// What one team or area counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Loaded {
    /// The review requests open now.
    pub requested: usize,
    /// The reviews given over the window.
    pub reviewed: usize,
}

/// A view's load, as the rows one surface shows count it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loads {
    /// Each team of at least the least team, by name.
    pub teams: Vec<(String, Loaded)>,
    /// Each area shown by name.
    pub areas: Vec<(Area, Loaded)>,
    /// What the teams and areas folded count together, where the people
    /// folded into it are at least the least team.
    pub other: Option<Loaded>,
    /// Whether reviews given were counted: only where every source was
    /// read kept.
    pub reviewed: bool,
}

/// The review requests and reviews one row of the load counts, each a
/// pull request's index and a login or team, so one counted twice by two
/// folded rows counts once in "other".
#[derive(Default)]
struct Tally {
    requested: BTreeSet<(usize, String)>,
    reviewed: BTreeSet<(usize, String)>,
}

impl Tally {
    fn count(&self) -> Loaded {
        Loaded {
            requested: self.requested.len(),
            reviewed: self.reviewed.len(),
        }
    }

    fn is_empty(&self) -> bool {
        self.requested.is_empty() && self.reviewed.is_empty()
    }

    fn fold(&mut self, other: Self) {
        self.requested.extend(other.requested);
        self.reviewed.extend(other.reviewed);
    }
}

/// What one row the view shows asks and was given: its forge's host, each
/// review request open on it, and each reviewer who gave one in the window.
struct Asked<'r> {
    pull: &'r Pull,
    host: Option<String>,
    requested: Vec<String>,
    reviewed: Vec<String>,
}

/// What folds into "other": the tallies, and the people known to be in
/// them.
#[derive(Default)]
struct Folded {
    tally: Tally,
    people: BTreeSet<String>,
}

impl Folded {
    /// `tally` folded, its people `people`, where it counts anything.
    fn add<'p>(&mut self, tally: Tally, people: impl IntoIterator<Item = &'p str>) {
        if !tally.is_empty() {
            self.people.extend(people.into_iter().map(str::to_owned));
            self.tally.fold(tally);
        }
    }
}

impl Loads {
    /// The load `rows` count, by `teams` and `load`, reviews given counted
    /// from `given` over the window before `now`.
    #[must_use]
    pub fn of(rows: &[Row], given: &Given, (teams, load): (&Teams, Load), now: Timestamp) -> Self {
        let since = now.checked_sub(load.window).unwrap_or(now);
        let asked: Vec<Asked<'_>> = rows
            .iter()
            .map(|row| Asked {
                pull: &row.pull,
                host: url::Url::parse(&row.pull.url)
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_lowercase)),
                requested: row
                    .pull
                    .reviewers
                    .iter()
                    .map(|reviewer| reviewer.to_lowercase())
                    .collect(),
                reviewed: given.of(&row.pull, since).map(str::to_lowercase).collect(),
            })
            .collect();
        let mut folded = Folded::default();
        let mut shown_teams = Vec::new();
        for (name, team) in teams {
            let tally = team_tally(&asked, (name, team));
            let people = team.people();
            if people.len() >= load.least_team {
                if !tally.is_empty() {
                    shown_teams.push((name.clone(), tally.count()));
                }
            } else {
                folded.add(tally, people);
            }
        }
        let mut shown_areas = Vec::new();
        for (area, tally) in area_tallies(&asked) {
            match area.kind {
                AreaKind::Paths | AreaKind::Section => shown_areas.push((area, tally.count())),
                AreaKind::Person => {
                    let login = bare(&area.name);
                    folded.add(tally, [login.as_str()]);
                }
                AreaKind::Name => folded.add(tally, []),
                AreaKind::Team | AreaKind::Group | AreaKind::Role => match teams
                    .iter()
                    .find(|(name, _)| bare(name) == bare(&area.name))
                    .map(|(_, team)| team.people())
                {
                    Some(people) if people.len() >= load.least_team => {
                        shown_areas.push((area, tally.count()));
                    }
                    Some(people) => folded.add(tally, people),
                    None if load.unknown_teams => shown_areas.push((area, tally.count())),
                    None => folded.add(tally, []),
                },
            }
        }
        let other = (folded.people.len() >= load.least_team).then(|| folded.tally.count());
        Self {
            teams: shown_teams,
            areas: shown_areas,
            other,
            reviewed: !given.live,
        }
    }
}

/// What `team`, named `name`, counts of `asked`: each request naming it
/// as `org/team`, or naming one of its members; each review a member gave.
fn team_tally(asked: &[Asked<'_>], (name, team): (&str, &Team)) -> Tally {
    let name = bare(name);
    let mut tally = Tally::default();
    for (index, row) in asked.iter().enumerate() {
        let host = row.host.as_deref();
        for login in &row.requested {
            let named = if login.contains('/') {
                bare(login) == name
            } else {
                team.holds(login, host)
            };
            if named {
                tally.requested.insert((index, login.clone()));
            }
        }
        for login in &row.reviewed {
            if team.holds(login, host) {
                tally.reviewed.insert((index, login.clone()));
            }
        }
    }
    tally
}

/// What each area a row of `asked` falls in counts: every request open and
/// review given on each of its rows.
fn area_tallies(asked: &[Asked<'_>]) -> BTreeMap<Area, Tally> {
    let mut tallies: BTreeMap<Area, Tally> = BTreeMap::new();
    for (index, row) in asked.iter().enumerate() {
        for area in &row.pull.areas {
            let tally = tallies.entry(area.clone()).or_default();
            tally
                .requested
                .extend(row.requested.iter().map(|login| (index, login.clone())));
            tally
                .reviewed
                .extend(row.reviewed.iter().map(|login| (index, login.clone())));
        }
    }
    tallies
}

#[cfg(test)]
mod tests;
