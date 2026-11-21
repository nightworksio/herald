use std::collections::BTreeMap;

use jiff::{SignedDuration, Timestamp};

use super::{Failure, Overviews, Pull, Reason, Rules, Triage, Turn};
use crate::bump::{Bump, BumpPattern, read_bump};
use crate::decide::Slot;
use crate::glob::Glob;

/// What a view does with a bot's pull requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Bots {
    /// Lists them with everyone's.
    Include,
    /// Leaves them out.
    Exclude,
    /// Lists only them.
    Only,
    /// Gathers them into batches of the same bump, apart from everyone's.
    #[default]
    Batch,
}

/// What tells a view whose turn a pull request is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Deciding {
    /// herald's own rules.
    #[default]
    Herald,
    /// The forge's own status, mapped as it is.
    Forge,
    /// The first of these labels a pull request carries, each with the
    /// state it puts it in.
    Labels(Vec<(String, Turn)>),
}

/// What a view groups its rows by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupBy {
    /// Whose turn it is.
    #[default]
    Turn,
    /// The repository.
    Repository,
    /// The author.
    Author,
    /// Nothing: one group.
    Nothing,
}

/// Which open pull requests a surface shows, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// The states shown, every one where empty.
    pub states: Vec<Turn>,
    /// What it does with a bot's pull requests.
    pub bots: Bots,
    /// The repositories shown, by full path, every one where empty.
    pub repositories: Vec<Glob>,
    /// The authors shown, every one where empty.
    pub authors: Vec<Glob>,
    /// The labels, any one of which a pull request shown carries, every
    /// pull request where empty.
    pub labels: Vec<Glob>,
    /// How long one waits with nobody acting before it is stale.
    pub stale: SignedDuration,
    /// Whether it shows only the stale.
    pub stale_only: bool,
    /// What it groups its rows by.
    pub group_by: GroupBy,
    /// What tells whose turn each is.
    pub deciding: Deciding,
    /// herald's own rules, where they tell.
    pub rules: Rules,
    /// Its own bump patterns, read before herald's.
    pub bumps: Vec<BumpPattern>,
    /// How it triages its failing checks (ADR-0065).
    pub triage: Triage,
}

impl Default for View {
    fn default() -> Self {
        Self {
            states: Vec::new(),
            bots: Bots::default(),
            repositories: Vec::new(),
            authors: Vec::new(),
            labels: Vec::new(),
            stale: SignedDuration::from_hours(14 * 24),
            stale_only: false,
            group_by: GroupBy::default(),
            deciding: Deciding::default(),
            rules: Rules::default(),
            bumps: Vec::new(),
            triage: Triage::default(),
        }
    }
}

/// One pull request as a view shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The pull request.
    pub pull: Pull,
    /// Whose turn it is.
    pub state: Turn,
    /// Why.
    pub reason: Reason,
    /// How long it has waited since anyone acted on it.
    pub waited: SignedDuration,
    /// Whether it waited past the view's `stale`.
    pub stale: bool,
    /// What a decider settled for it, by slot (ADR-0057).
    pub decided: BTreeMap<Slot, String>,
    /// The open pull requests of its repository merging into its branch
    /// whose heads change a path its head changes, by number (ADR-0063).
    pub overlaps: Vec<u64>,
}

impl Row {
    /// The logins whose turn it is: its author's where it is a draft or
    /// waits on them, each reviewer asked where it waits on review, and
    /// nobody's otherwise.
    #[must_use]
    pub fn waiting_on(&self) -> Vec<&str> {
        match self.state {
            Turn::Draft | Turn::WaitingOnAuthor => {
                self.pull.author.iter().map(String::as_str).collect()
            }
            Turn::WaitingOnReview => self.pull.reviewers.iter().map(String::as_str).collect(),
            _ => Vec::new(),
        }
    }
}

/// The rows that share what a view groups by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grouped {
    /// What they share: a state's name, a repository's full path, or an
    /// author's login, empty where the forge names none.
    pub name: String,
    /// The rows, oldest first.
    pub rows: Vec<Row>,
}

/// A bot's pull requests proposing the same bump, in any repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    /// The bump, where a pattern read one.
    pub bump: Option<Bump>,
    /// The rows, oldest first.
    pub rows: Vec<Row>,
    /// How many are green and mergeable.
    pub green: usize,
    /// How many have a failed check.
    pub failing: usize,
}

/// A view's rows, as every surface shows them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Board {
    /// The groups, in the order the view lists them.
    pub groups: Vec<Grouped>,
    /// The bots' batches, the most pull requests first.
    pub batches: Vec<Batch>,
    /// How many it shows in each state, every state named.
    pub counts: Vec<(Turn, usize)>,
    /// How many of those it shows are stale.
    pub stale: usize,
    /// Each check failing on the pull requests it shows (ADR-0065).
    pub failures: Vec<Failure>,
    /// Its overviews, as its audience may see them (ADR-0066).
    pub overviews: Overviews,
    /// How many pull requests of private or internal repositories were
    /// withheld from it, where its output was not said to be private.
    pub withheld: usize,
}

impl Board {
    /// How many pull requests it shows, grouped and batched.
    #[must_use]
    pub fn total(&self) -> usize {
        self.counts.iter().map(|(_, count)| count).sum()
    }
}

/// A view's board as a report shows it: the view's name, and its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Showing<'a> {
    /// The view's name.
    pub view: &'a str,
    /// Its rows.
    pub board: &'a Board,
}

/// Whether `globs` is empty or one of it matches `name`.
fn named(globs: &[Glob], name: &str) -> bool {
    globs.is_empty() || globs.iter().any(|glob| glob.matches(name))
}

impl View {
    /// Whether it shows the repository at the full path `repository`: the
    /// one home of its repository filter, which its rows and its overviews
    /// both take (ADR-0066).
    #[must_use]
    pub fn holds(&self, repository: &str) -> bool {
        named(&self.repositories, repository)
    }

    /// Whether it holds every repository of its sources, filtering none
    /// out by name.
    #[must_use]
    pub fn holds_every(&self) -> bool {
        self.repositories.is_empty()
    }

    /// Whose turn `pull` is, by what tells the view.
    fn decided(&self, pull: &Pull) -> (Turn, Reason) {
        match &self.deciding {
            Deciding::Herald => pull.waiting(&self.rules),
            Deciding::Forge => pull.forge_waiting(),
            Deciding::Labels(labels) => pull.labelled_waiting(labels),
        }
    }

    /// Whether it shows `pull`, leaving its state aside.
    fn takes(&self, pull: &Pull) -> bool {
        let bots = match self.bots {
            Bots::Exclude => !pull.bot,
            Bots::Only => pull.bot,
            Bots::Include | Bots::Batch => true,
        };
        bots && self.holds(&pull.repository)
            && named(&self.authors, pull.author.as_deref().unwrap_or_default())
            && (self.labels.is_empty()
                || pull.labels.iter().any(|label| named(&self.labels, label)))
    }

    /// `pull` as a row at `now`, where the view shows it.
    fn row(&self, pull: &Pull, now: Timestamp) -> Option<Row> {
        if !self.takes(pull) {
            return None;
        }
        let (state, reason) = self.decided(pull);
        let waited = now.duration_since(pull.updated).max(SignedDuration::ZERO);
        let past = waited >= self.stale;
        let shown =
            (self.states.is_empty() || self.states.contains(&state)) && (past || !self.stale_only);
        shown.then(|| Row {
            pull: pull.clone(),
            state,
            reason,
            waited,
            stale: past,
            decided: BTreeMap::new(),
            overlaps: Vec::new(),
        })
    }

    /// What a row is grouped under.
    fn group_of(&self, row: &Row) -> String {
        match self.group_by {
            GroupBy::Turn => row.state.name().to_owned(),
            GroupBy::Repository => row.pull.repository.clone(),
            GroupBy::Author => row.pull.author.clone().unwrap_or_default(),
            GroupBy::Nothing => String::new(),
        }
    }

    /// The bump `pull` proposes, by the view's patterns, then herald's.
    fn bump_of(&self, pull: &Pull) -> Option<Bump> {
        read_bump(&self.bumps, &pull.title)
    }
}

/// Oldest first: the longest wait, then by repository and number.
fn oldest_first(rows: &mut [Row]) {
    rows.sort_by(|left, right| {
        right
            .waited
            .cmp(&left.waited)
            .then_with(|| left.pull.repository.cmp(&right.pull.repository))
            .then_with(|| left.pull.number.cmp(&right.pull.number))
    });
}

/// The rows `view` shows of `pulls` at `now`, each decided by the view's
/// own rules alone.
#[must_use]
pub fn rows(view: &View, pulls: &[Pull], now: Timestamp) -> Vec<Row> {
    let mut shared = super::overlaps(pulls);
    pulls
        .iter()
        .filter_map(|pull| view.row(pull, now))
        .map(|mut row| {
            let since = now.checked_sub(view.triage.flaky_window).unwrap_or(now);
            row.pull.flips.retain(|flip| flip.at >= since);
            row.overlaps = shared
                .remove(&(row.pull.repository.clone(), row.pull.number))
                .unwrap_or_default();
            row
        })
        .collect()
}

/// The rows `view` shows of `pulls` at `now`: grouped, the bots' batched
/// where it batches them, each oldest first.
#[must_use]
pub fn board(view: &View, pulls: &[Pull], now: Timestamp) -> Board {
    grouped(view, rows(view, pulls, now))
}

/// `rows` as `view` shows them: grouped, the bots' batched where it
/// batches them, each oldest first.
#[must_use]
pub fn grouped(view: &View, rows: Vec<Row>) -> Board {
    let counts = Turn::ALL
        .into_iter()
        .map(|state| (state, rows.iter().filter(|row| row.state == state).count()))
        .collect();
    let stale = rows.iter().filter(|row| row.stale).count();
    let failures = view.triage.failures(&rows);
    let mut groups: BTreeMap<(usize, String), Vec<Row>> = BTreeMap::new();
    let mut batches: BTreeMap<Option<Bump>, Vec<Row>> = BTreeMap::new();
    for row in rows {
        if view.bots == Bots::Batch && row.pull.bot {
            batches
                .entry(view.bump_of(&row.pull))
                .or_default()
                .push(row);
            continue;
        }
        let order = if view.group_by == GroupBy::Turn {
            Turn::ALL
                .iter()
                .position(|state| *state == row.state)
                .unwrap_or_default()
        } else {
            0
        };
        groups
            .entry((order, view.group_of(&row)))
            .or_default()
            .push(row);
    }
    let groups = groups
        .into_iter()
        .map(|((_, name), mut rows)| {
            oldest_first(&mut rows);
            Grouped { name, rows }
        })
        .collect();
    let mut batches: Vec<Batch> = batches
        .into_iter()
        .map(|(bump, mut rows)| {
            oldest_first(&mut rows);
            Batch {
                green: rows.iter().filter(|row| row.pull.green()).count(),
                failing: rows.iter().filter(|row| row.pull.failing()).count(),
                bump,
                rows,
            }
        })
        .collect();
    batches.sort_by_key(|batch| std::cmp::Reverse(batch.rows.len()));
    Board {
        groups,
        batches,
        counts,
        stale,
        failures,
        overviews: Overviews::default(),
        withheld: 0,
    }
}

#[cfg(test)]
mod tests;
