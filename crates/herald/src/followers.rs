//! The followers of herald's ActivityPub actors from the CLI (ADR-0004):
//! who follows an actor and whose follow waits for approval, and a waiting
//! follow approved or rejected.

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::{Host, Hosting, ServingOn, Through};
use herald_config::{Config, Uses};
use herald_core::{DestinationName, Sent, Subscription};
use herald_flows::{Decision, FollowError, decide, followers};
use herald_ports::{Clock as _, Keyring, Store};

use super::cli::Following;
use super::output::say;
use super::stores::composed;
use super::{Connect, DONE, STOPPED, place};

/// What the operator asks of an actor's followers.
struct FollowersRequest<'a> {
    destination: DestinationName,
    actor: &'a str,
    decided: Option<(&'a str, Decision)>,
}

/// What was read or decided.
enum Done<'a> {
    Listed(Vec<Subscription>),
    Decided {
        follower: &'a str,
        decision: Decision,
        answered: Sent,
    },
}

impl<'a, H: Host> Through<H> for FollowersRequest<'a> {
    type Done = Result<Done<'a>, FollowError>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Self::Done {
        let ports = serving.ports;
        match self.decided {
            None => followers(
                serving.connected,
                ports.store,
                &self.destination,
                self.actor,
            )
            .await
            .map(Done::Listed),
            Some((follower, decision)) => decide(
                serving.connected,
                ports.store,
                ports.signer,
                ports.clock.now(),
                &self.destination,
                self.actor,
                follower,
                decision,
            )
            .await
            .map(|answered| Done::Decided {
                follower,
                decision,
                answered,
            }),
        }
    }
}

/// Answers `following`: an actor's followers listed, or a waiting follow
/// approved or rejected, saying what was done.
///
/// # Errors
///
/// When the store or the routes cannot be reached, or the follow cannot
/// be read or decided.
pub(crate) async fn following(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    following: &Following,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let composed = composed(
        config,
        vars,
        connect,
        Hosting::Actions,
        Uses::Announces,
        place(vars),
    )
    .await?;
    let (destination, actor, decided) = match following {
        Following::List { destination, actor } => (destination, actor, None),
        Following::Approve {
            destination,
            actor,
            follower,
        } => (
            destination,
            actor,
            Some((follower.as_str(), Decision::Approve)),
        ),
        Following::Reject {
            destination,
            actor,
            follower,
        } => (
            destination,
            actor,
            Some((follower.as_str(), Decision::Reject)),
        ),
    };
    let asked = FollowersRequest {
        destination: DestinationName::new(destination),
        actor,
        decided,
    };
    let done = composed
        .run(asked)
        .await
        .map_err(|error| error.to_string())?;
    Ok(match done {
        Done::Listed(kept) => {
            listed(actor, &kept, out);
            DONE
        }
        Done::Decided {
            follower,
            decision,
            answered,
        } => decision_reported(follower, decision, actor, &answered, out),
    })
}

/// Every follower of `actor` in `kept`, those waiting last.
fn listed(actor: &str, kept: &[Subscription], out: &mut dyn Write) {
    if kept.is_empty() {
        say(
            out,
            &format!("No one follows {actor}, and no follow waits."),
        );
    }
    for approved in kept.iter().filter(|kept| kept.pending.is_none()) {
        say(out, &format!("follows: {}", approved.subscriber));
    }
    for waiting in kept.iter().filter(|kept| kept.pending.is_some()) {
        say(out, &format!("waits: {}", waiting.subscriber));
    }
}

/// What `decision` of `follower`'s follow of `actor` did, and the code it
/// ends with: one that the follower's inbox did not take the answer to
/// stops.
fn decision_reported(
    follower: &str,
    decision: Decision,
    actor: &str,
    answered: &Sent,
    out: &mut dyn Write,
) -> u8 {
    let said = match decision {
        Decision::Approve => format!("{follower} follows {actor} now"),
        Decision::Reject => format!("{follower} was turned away from {actor}"),
    };
    say(out, &said);
    let unanswered = match answered {
        Sent::Landed(_) => return DONE,
        Sent::RetryAfter(after) => format!("it asked to wait {after:#}"),
        Sent::Refused(reason) => reason.clone(),
    };
    say(
        out,
        &format!("Its inbox did not take the answer, which was not sent again: {unanswered}"),
    );
    STOPPED
}
