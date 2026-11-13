//! The layers of support and what each host offers (ADR-0002).

use std::collections::BTreeSet;

use jiff::SignedDuration;

/// A layer of support, in ADR-0002's order: each serves everything the one
/// before it serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// The CLI in a workflow or any CI, with no endpoint.
    Actions,
    /// A serverless driver's endpoint.
    Serverless,
    /// A process of its own that holds its endpoint open.
    Container,
}

impl Tier {
    /// The tier as ADR-0002 and doctor's JSON name it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Actions => "actions",
            Self::Serverless => "serverless",
            Self::Container => "container",
        }
    }
}

/// How a host reaches what herald fetches on others' behalf (ADR-0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Egress {
    /// Names resolve where a private address is refused.
    Checked,
    /// Names resolve out of herald's sight, so nothing is fetched on others'
    /// behalf until `HERALD_EGRESS_GUARDED` says the host's egress refuses
    /// private addresses.
    Unchecked,
}

/// How often a platform's own scheduler asks for each round, as its deploy
/// files ship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    /// What asks, as a sentence names it.
    pub by: &'static str,
    /// How often it asks for a poll.
    pub poll: SignedDuration,
    /// How often it asks for housekeeping.
    pub housekeep: SignedDuration,
}

/// What asks a layer for its rounds of polling and housekeeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounds {
    /// A scheduled workflow, running `herald poll` and `herald housekeep`.
    Workflow,
    /// herald itself, at the periods the config's `[schedule]` names.
    Scheduled,
    /// The platform's own scheduler.
    Platform(Cadence),
    /// Only a request holding the round secret.
    Asked,
}

/// How long what a layer holds in memory lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Memory {
    /// As long as the process, across requests.
    Lasting,
    /// One request, or one run.
    PerRequest,
}

/// What a layer of support serves (ADR-0002), as its host describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    /// Its name, as `herald doctor --layer` writes it.
    pub name: &'static str,
    /// The layer of ADR-0002 it belongs to.
    pub tier: Tier,
    /// The store kinds it keeps records in.
    pub stores: Vec<&'static str>,
    /// Whether it serves herald's endpoint, where commands are posted.
    pub endpoint: bool,
    /// Whether it holds sessions open, which carry commands to it.
    pub sessions: bool,
    /// How it reaches what herald fetches on others' behalf.
    pub egress: Egress,
    /// What asks it for rounds.
    pub rounds: Rounds,
    /// The secrets, any one of which a request asking it for a round holds,
    /// where its rounds are asked for over HTTP alone.
    pub round_secrets: &'static [&'static str],
    /// How long the keys it keeps in memory last.
    pub keyring: Memory,
}

/// What doctor finds set where it runs, for the settings a layer needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Here {
    /// Whether the config is held to what is set here, as `--online` asks:
    /// a setting a layer needs and that is unset here is a gap rather than a
    /// note.
    pub online: bool,
    /// Whether `HERALD_EGRESS_GUARDED` is `true` here.
    pub guarded: bool,
    /// The round secrets set here.
    pub round_secrets: BTreeSet<String>,
}
