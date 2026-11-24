//! herald's forge sources (ADR-0003): a release on GitHub, GitLab or
//! Forgejo becomes an [`Event`](herald_core::Event), parsed without I/O.
//!
//! Each forge's release event, as a webhook or as its CI's event, announces
//! a published release only, and a draft never announces. Polling a forge's
//! releases API is described as requests the flow sends and responses it
//! hands back.

mod alerts;
mod delivered;
mod events;
mod folding;
mod forge;
mod forgejo;
mod ghcr;
mod github;
mod gitlab;
mod graphql;
mod hooks;
mod issues;
mod kind;
mod limited;
mod listing;
mod members;
mod merge_read;
mod merge_write;
mod milestones;
mod moved;
mod options;
mod owner;
mod packages;
mod paging;
mod polled;
mod pulls;
mod queue;
mod rerunner;
mod roles;
mod runs;
mod tags;
mod wire;

pub use forge::{Forge, Origin, Repository, Token};
pub use ghcr::{GhcrKind, GhcrOptions, GhcrSource};
pub use herald_kinds::Hooked;
pub use kind::{ForgeSource, ForgejoKind, GitHubKind, GitLabKind};
pub use milestones::{ci_milestone, of_a_milestone};
pub use options::{
    ALERT_ACTIONS, AlertAction, Alerts, Conclusion, Discovery, EDITS_DEPTH, FEED_PAGE, FEED_PAGES,
    FORGEJO_QUEUELESS, FORGEJO_RETRIES, FORGEJO_UNWRITTEN, GITHUB_APPS, GITHUB_FAST_FORWARD,
    GITHUB_SCOPES, GITLAB_SCOPES, GITLAB_UPDATES, GRAPHQL_BATCH, GRAPHQL_BATCH_ALLOWED,
    ISSUE_ACTIONS, IssueAction, Issues, KEEP_MEMBERS, KEY, MILESTONE_ACTIONS, MOST_CODEOWNERS_PATH,
    MOST_CODEOWNERS_PATHS, MOST_LISTED, MOST_MEMBERS, MOST_MEMBERS_ALLOWED, MOST_MILESTONES,
    MOST_MILESTONES_ALLOWED, MemberHooks, Members, MilestoneAction, Milestones, Override,
    PULL_ACTIONS, PackageVisibility, PullAction, Pulls, RUN_ACTIONS, RUN_FROM, RunAction, RunFrom,
    Runs, Severity, SignIn, Signals, SourceOptions, TAG_WINDOW, TeamsFrom, Tool, Transport, Values,
    Write,
};
pub use polled::tag_event_of;
pub use queue::{MOST_OPEN, MOST_PULLS, Opened, PAGE, PER_PULL, Queued, ReadPath, cost};
pub use rerunner::{
    DENIED, ENDED, EXPIRED, ForgeRerunner, GITHUB_APP_USER_TOKEN, GITHUB_OAUTH_TOKEN, UNINSTALLED,
    UNNUMBERED,
};
pub use runs::{ci_run, of_a_run};
