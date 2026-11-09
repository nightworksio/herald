//! The followers of herald's ActivityPub actors from the CLI (ADR-0004):
//! who follows an actor and whose follow waits for approval, and a waiting
//! follow approved or rejected.

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::Served;
use herald_config::Config;
use herald_core::{DestinationName, Sent, Subscription};
use herald_crypto::RingSigner;
use herald_environment::SystemClock;
use herald_flows::{Connected, Decision, FollowError, Followed, decide, followers};
use herald_ports::{Clock as _, Keyring, Store as Keeps};

use super::cli::Following;
use super::connect::Connection;
use super::output::say;
use super::stores::{Commands, STORES, reached};
use super::{Connect, DONE, STOPPED};

/// What the operator asks of an actor's followers.
struct Asked<'a> {
    connected: &'a Connected<Connection>,
    destination: DestinationName,
    actor: &'a str,
    decided: Option<(&'a str, Decision)>,
}

/// What was read or decided.
enum Done {
    Listed(Vec<Subscription>),
    Decided(Followed),
}

impl Served for Asked<'_> {
    type Done = Result<Done, FollowError>;

    async fn over<S: Keeps + Keyring>(self, store: &S) -> Self::Done {
        match self.decided {
            None => followers(self.connected, store, &self.destination, self.actor)
                .await
                .map(Done::Listed),
            Some((follower, decision)) => decide(
                self.connected,
                store,
                &RingSigner,
                SystemClock.now(),
                &self.destination,
                self.actor,
                follower,
                decision,
            )
            .await
            .map(Done::Decided),
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
    let (records, connected) =
        reached(config, vars, connect, Commands::Unanswered, &STORES).await?;
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
    let asked = Asked {
        connected: &connected,
        destination: DestinationName::new(destination),
        actor,
        decided,
    };
    let done = records
        .serve(&connected, asked)
        .await
        .map_err(|error| error.to_string())?;
    Ok(match done {
        Done::Listed(kept) => {
            listed(actor, &kept, out);
            DONE
        }
        Done::Decided(followed) => told(&followed, out),
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

/// What a decision did, and the code it ends with: one that the follower's
/// inbox did not take the answer to stops.
fn told(followed: &Followed, out: &mut dyn Write) -> u8 {
    let (said, answered) = match followed {
        Followed::Accepted {
            follower,
            actor,
            answered,
        } => (format!("{follower} follows {actor} now"), answered),
        Followed::Rejected {
            follower,
            actor,
            answered,
        } => (format!("{follower} was turned away from {actor}"), answered),
        Followed::Waiting { .. } | Followed::Undone { .. } | Followed::Ignored(_) => {
            return DONE;
        }
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
