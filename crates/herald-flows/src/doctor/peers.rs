//! What doctor names of the peers a config trusts (ADR-0047, ADR-0049),
//! and what each may do with each resource that grants it anything
//! (ADR-0054).

use herald_config::{Config, Rotation};
use herald_peers::multibase::ed25519_public;
use herald_ports::{Cursors, Published};
use jiff::Timestamp;

use super::{Findings, Gap, gap};

/// Each peer whose rotation is signed, as a stolen key of it could replace
/// itself there.
pub(super) fn signed(config: &Config, notes: &mut Vec<String>) {
    for (name, _) in config
        .peers
        .iter()
        .filter(|(_, peer)| peer.rotation == Rotation::Signed)
    {
        notes.push(format!(
            "peers.{name}: rotation = \"signed\" takes a key set the peer proves with a key trusted for it, so a stolen key of it can hand over to a key of the thief's; rotation = \"manual\" leaves each new key to you."
        ));
    }
}

/// Each peer whose rotation is signed, where the store keeps no peer's key
/// set.
pub(super) fn unkept(config: &Config, gaps: &mut Vec<Gap>) {
    for (name, _) in config
        .peers
        .iter()
        .filter(|(_, peer)| peer.rotation == Rotation::Signed)
    {
        gaps.push(gap(
            "taking the key set a peer hands over to",
            format!("peers.{name}.rotation"),
            "the markers store keeps no peer's key set, so only the keys herald.toml pins stand for the peer; set rotation = \"manual\", or choose a store that does".to_owned(),
        ));
    }
}

/// Each peer whose rotation is signed that handed over to keys other than
/// those pinned for it, as `published` keeps them: a note naming them and
/// how to pin them, or a gap where its set cannot be read.
pub async fn handed_over<P: Published>(config: &Config, published: &P) -> Findings {
    let mut findings = Findings::default();
    for (name, peer) in config
        .peers
        .iter()
        .filter(|(_, peer)| peer.rotation == Rotation::Signed)
    {
        let pinned: Vec<String> = peer.keys.iter().map(ed25519_public).collect();
        match published.peer_keys(name).await {
            Ok(Some(held)) if held.value.stands(&pinned) && held.value.keys != pinned => {
                findings.notes.push(format!(
                    "peers.{name}: the peer handed over to {} under sequence {}, which herald.toml does not pin; herald peer add {} --name {name} --update pins them.",
                    held.value.keys.join(", "),
                    held.value.sequence,
                    peer.url
                ));
            }
            Ok(_) => {}
            Err(error) => findings.gaps.push(gap(
                &format!("reading the key set the peer {name} handed over to"),
                format!("peers.{name}.rotation"),
                error.to_string(),
            )),
        }
    }
    findings
}

/// Each instance whose tombstones of its releases a herald source keeps as
/// many of as its `peer_share` lets it, and each whose envelope its last pull passed over
/// as proved too far ahead, as `cursors` keeps them: a note naming it; or a
/// gap where the cursor cannot be read.
pub async fn shared<C: Cursors>(config: &Config, cursors: &C) -> Findings {
    let mut findings = Findings::default();
    for (name, share) in config
        .sources
        .iter()
        .filter_map(|(name, source)| Some((name, source.share()?)))
    {
        let cursor = match cursors.cursor(name).await {
            Ok(Some(cursor)) => cursor.value,
            Ok(None) => continue,
            Err(error) => {
                findings.gaps.push(gap(
                    "reading what a herald source holds of each instance",
                    format!("sources.{}", name.as_str()),
                    error.to_string(),
                ));
                continue;
            }
        };
        for (maker, held) in cursor.held().iter().filter(|(_, held)| **held >= share) {
            findings.notes.push(format!(
                "sources.{}: it keeps {held} tombstones of releases {maker} withdrew, all peer_share lets it, so a pull takes no new release of it; raise peer_share to take them.",
                name.as_str()
            ));
        }
        for maker in &cursor.ahead {
            findings.notes.push(format!(
                "sources.{}: its last pull passed over an envelope of {maker} whose proof was made too far ahead of now.",
                name.as_str()
            ));
        }
    }
    findings
}

/// What each peer may do with each resource that grants it anything, until
/// when, at `now`: a note for each grant, one for each with no end, and
/// one for each that has ended or ends within `[federation]
/// grants_ending`.
#[must_use]
pub fn granted(config: &Config, now: Timestamp) -> Findings {
    let mut findings = Findings::default();
    let ending = now
        .checked_add(config.federation.grants_ending)
        .unwrap_or(Timestamp::MAX);
    for (name, peer) in &config.peers {
        for (source, grant) in &peer.grants {
            let actions: Vec<&str> = grant.actions.iter().map(|action| action.name()).collect();
            let resource = format!("sources.{}", source.as_str());
            let until = grant.until.map_or_else(
                || "with no end".to_owned(),
                |until| format!("until {until}"),
            );
            findings.notes.push(format!(
                "peers.{name} may {} {resource} {until}.",
                actions.join(" and ")
            ));
            match grant.until {
                None => findings.notes.push(format!(
                    "peers.{name}: its grant on {resource} never ends; give it an until, so a key stolen from the peer pushes only for so long."
                )),
                Some(until) if until <= now => findings.notes.push(format!(
                    "peers.{name}: its grant on {resource} ended at {until}, so herald refuses what it pushes; give it a later until, or remove it."
                )),
                Some(until) if until <= ending => findings.notes.push(format!(
                    "peers.{name}: its grant on {resource} ends at {until}; give it a later until to keep taking what it pushes."
                )),
                _ => {}
            }
        }
    }
    if config.peers.values().any(|peer| !peer.grants.is_empty()) {
        findings.notes.push(format!(
            "A peer pushing past its rate has the nonces of at most {} refused requests kept in {} minutes ([federation] refused_nonces); past them herald holds it, taking nothing of it until every request it made has expired.",
            config.federation.refused_nonces,
            herald_config::Federation::SIGNED_FOR_ALLOWED.as_mins()
        ));
    }
    findings
}
