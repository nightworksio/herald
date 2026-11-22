//! herald's commands as flows over the ports (ADR-0001): each driver
//! constructs its ports and hands them to the same flow.
//!
//! [`prepare`] resolves a config's credentials and marker keys, and
//! [`Prepared::connect`] gives each route and source the `Http` that
//! reaches it. [`announce`] then sends an event to every route that takes
//! it, once, finishing what an earlier run left (ADR-0005); [`poll`] asks
//! each source for what it has not seen and announces it (ADR-0003); and
//! [`housekeep`] does each thread's chores as they fall due. [`verified`]
//! lets an inbound request through only once its signature is its sender's
//! (ADR-0007), and [`answer`] answers every request `herald server` and the
//! serverless drivers take, leaving its work to [`work`]. [`Sessions`]
//! holds the sessions a destination carries commands over. [`doctor`]
//! names what a config asks for that cannot be served, and [`preview`]
//! renders each route's templates as its destination would show them.
//! [`peering`] reads a peer's discovery document, checked against the keys
//! trusted for it.

mod acting;
mod advised;
mod announce;
mod applying;
mod approving;
mod audience;
mod budget;
mod codeowned;
mod command_answers;
mod command_endpoints;
mod command_requests;
mod deciding;
mod doctor;
mod follows;
mod grouping;
mod halted;
mod housekeep;
mod installations;
mod kept;
mod oidc;
mod orphans;
pub mod peering;
mod poll;
mod prepare;
mod preview;
mod publishing;
mod recall;
mod report;
mod reruns;
mod room;
pub mod round;
mod rules;
mod scope;
mod serving;
mod sessions;
mod trial;
mod verification;
mod withdraw;

pub use announce::{Announced, Outcome, Ports, Stop, Within, announce, announce_within};
pub use applying::{
    Applied, Applying, Done, Planned, PlansAsked, RerunPlans, SHARED_INSTALLATION, SigningIn,
    UNCHOSEN, UNFINISHED, UNKEPT, UNPLANNED, UNPLANNED_WITHHELD, UNSEPARATED, rerun_apply,
    rerun_plans,
};
pub use approving::{Decision, decide, followers};
pub use budget::{Budget, Requests};
pub use command_answers::{Answer, completed, deferral, interact, replied};
pub use doctor::{
    Cadence, EGRESS_GUARDED, Egress, Federation, FeedShown, Findings, Gap, GrantShown, Here, Layer,
    MOST_SKIPPED_NAMED, Memory, OwnKey, PeerShown, Pulls, PushShown, Reached, Rounds, RunShown,
    Shown, SourceShown, Tier, codeowners, doctor, egress_guarded, federation, granted, handed_over,
    kept_tokens, keys, online, recalls, secrets, shared, unserved,
};
pub use follows::{FollowError, Followed, followed};
pub use halted::UNSAID_FAILURE;
pub use herald_kinds::Replying;
pub use housekeep::{Chored, Housekept, HousekeptSchedule, housekeep, housekeep_announcing};
pub use installations::AHEAD as MINTED_AHEAD;
pub use orphans::{Orphaned, orphans, orphans_cleared};
pub use poll::{Fresh, Remembered, Sweep, Swept, announce_held, held_or_announced, poll};
pub use prepare::{
    Asking, Connected, PrepareError, Prepared, PreparedPages, UnreadSecret, prepare,
};
pub use preview::{Previewed, preview, preview_group};
pub use publishing::{Publishing, publish};
pub use report::{queue, report};
pub use room::{Place, Room, Running};
pub use round::{ROUND_SECRET, Round};
pub use serving::{
    Answered, Did, HEALTH, Inbound, Interacted, Job, Member, Reply, Serving, Webhooks, Withheld,
    Worked, answer, busy, refusal, uncomposed, unplaced, unworked, work,
};
pub use sessions::{Sessions, Taker};
pub use trial::trial;
pub use verification::{
    Admission, Checking, Keys, Rejected, Verified, activity_verified, admitted, released, verified,
};
pub use withdraw::{Withdrawable, WithdrawablePost, withdraw, withdrawable};
