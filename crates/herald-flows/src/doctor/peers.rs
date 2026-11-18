//! What doctor names of the peers a config trusts (ADR-0047, ADR-0049).

use herald_config::{Config, Rotation};
use herald_peers::multibase::ed25519_public;
use herald_ports::Published;

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
