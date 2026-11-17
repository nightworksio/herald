//! `herald peer`: another herald pinned by the keys its operator confirms,
//! listed, checked against its pinned keys, or no longer trusted.

use std::io::Write;

use herald_config::{Config, Peer};
use herald_core::kind_name;
use herald_crypto::RingVerifier;
use herald_environment::SystemClock;
use herald_flows::peering::{Discovered, Reading, bounds, discovered};
use herald_peers::keys::{Key, fingerprint};
use herald_peers::multibase::ed25519_public;
use herald_ports::Clock as _;
use tokio::io::{AsyncBufRead, AsyncBufReadExt as _};

use super::cli::Peering;
use super::output::say;
use super::{Connect, DONE, STOPPED, rewrite, text};

mod written;

/// What an operator answers to pin the keys shown.
const YES: [&str; 2] = ["y", "yes"];

/// What `herald peer add` is asked.
pub(crate) struct Adding<'a> {
    /// The peer's address, or its domain alone.
    pub(crate) url: &'a str,
    /// Its name, where given.
    pub(crate) name: Option<&'a str>,
    /// The keys to pin without asking, each a Multikey or a fingerprint.
    pub(crate) keys: &'a [String],
    /// Whether the table is printed rather than written.
    pub(crate) print: bool,
    /// Whether a peer already pinned takes its new keys.
    pub(crate) update: bool,
}

/// The `peer` command `asked`, against `config` read from `path`, reaching
/// peers through `connect`, asking the operator through `answers`.
pub(crate) async fn peering(
    config: &Config,
    path: &str,
    asked: &Peering,
    connect: Connect<'_>,
    answers: &mut (dyn AsyncBufRead + Send + Unpin),
    out: &mut dyn Write,
) -> Result<u8, String> {
    match asked {
        Peering::Add {
            url,
            name,
            key,
            print,
            update,
        } => {
            let adding = Adding {
                url,
                name: name.as_deref(),
                keys: key,
                print: *print,
                update: *update,
            };
            add(config, path, &adding, connect, answers, out).await
        }
        Peering::List => Ok(list(config, out)),
        Peering::Test { name } => test(config, name.as_deref(), connect, out).await,
        Peering::Revoke { name, key } => revoke(config, path, name, key.as_deref(), out),
    }
}

/// `key` as a line of a list: its Multikey and its fingerprint.
fn shown(key: &Key) -> String {
    format!("  {} {}", ed25519_public(key), fingerprint(key))
}

/// The discovery document at `url` of the instance on `domain`, read
/// within `most` bytes and checked against `pinned`, or where none are, the
/// keys it lists.
async fn read(
    config: &Config,
    connect: Connect<'_>,
    (url, domain, most): (&str, &str, usize),
    pinned: Option<&[Key]>,
) -> Result<Discovered, String> {
    let http = connect.reach(url)?;
    let reading = Reading {
        url,
        domain,
        most,
        bounds: bounds(config, SystemClock.now()),
    };
    discovered(&http, &RingVerifier, reading, pinned)
        .await
        .map_err(|why| why.to_string())
}

/// The peer `adding` asks for, its discovery document read and its keys
/// shown, pinned where the operator confirms them or names them.
async fn add(
    config: &Config,
    path: &str,
    adding: &Adding<'_>,
    connect: Connect<'_>,
    answers: &mut (dyn AsyncBufRead + Send + Unpin),
    out: &mut dyn Write,
) -> Result<u8, String> {
    let given = if adding.url.contains("://") {
        adding.url.to_owned()
    } else {
        format!("{}://{}", Peer::SCHEME, adding.url)
    };
    let (url, domain) = Peer::address(&given).ok_or_else(|| {
        format!("\"{}\" is not a peer's address; give its domain, or https://, its domain and any path its endpoints are under", adding.url)
    })?;
    let name = adding
        .name
        .map_or_else(|| domain.replace('.', "-"), str::to_owned);
    if !kind_name(&name) {
        return Err(format!(
            "\"{name}\" cannot name a peer; give --name of lowercase letters, digits and inner '-'"
        ));
    }
    let known = config.peers.get(&name);
    match (known, adding.update) {
        (Some(_), false) => {
            return Err(format!(
                "a peer named {name} is pinned already; give --update to pin its new keys, or another --name"
            ));
        }
        (None, true) => return Err(format!("no peer named {name} is pinned to update")),
        _ => {}
    }
    let most = known.map_or(Peer::MOST, |peer| peer.most);
    let discovery = Peer::discovery_at(&url);
    let found = read(config, connect, (&discovery, &domain, most), None).await?;
    let listed = &found.set.keys;
    say(
        out,
        &format!(
            "{domain} lists {} keys under sequence {}, its answer signed by {}:",
            listed.len(),
            found.set.sequence,
            fingerprint(&found.by)
        ),
    );
    for key in listed {
        let mut line = shown(key);
        if known.is_some_and(|peer| peer.keys.contains(key)) {
            line.push_str(" (pinned)");
        }
        if config.federation.revoked.contains(key) {
            line.push_str(" (revoked)");
        }
        say(out, &line);
    }
    let chosen = chosen(config, &name, listed, adding.keys)?;
    let keys: Vec<String> = chosen.iter().map(ed25519_public).collect();
    if adding.print {
        let _ = write!(out, "{}", written::table(&name, &url, &keys));
        return Ok(DONE);
    }
    if adding.keys.is_empty() && !confirmed(&name, path, keys.len(), answers, out).await {
        say(out, "Nothing was pinned.");
        return Ok(STOPPED);
    }
    rewrite(path, written::pinned(&text(path)?, &name, &url, &keys)?)?;
    say(
        out,
        &format!("Pinned {} keys for the peer {name} in {path}.", keys.len()),
    );
    Ok(DONE)
}

/// The keys of `listed` to pin for the peer `peer`: those `named` names,
/// each a Multikey or a fingerprint, or where it names none, every one not
/// revoked; never one revoked or pinned for another peer.
fn chosen(
    config: &Config,
    peer: &str,
    listed: &[Key],
    named: &[String],
) -> Result<Vec<Key>, String> {
    let revoked = &config.federation.revoked;
    let mut chosen = Vec::new();
    for name in named {
        let key = listed
            .iter()
            .find(|key| ed25519_public(key) == *name || fingerprint(key) == *name)
            .ok_or_else(|| format!("the peer lists no key {name}, so nothing was pinned"))?;
        if !chosen.contains(key) {
            chosen.push(*key);
        }
    }
    if named.is_empty() {
        chosen.extend(listed.iter().filter(|key| !revoked.contains(key)));
    }
    if let Some(key) = chosen.iter().find(|key| revoked.contains(key)) {
        return Err(format!(
            "the key {} is revoked in [federation] revoked, so nothing was pinned",
            ed25519_public(key)
        ));
    }
    if chosen.is_empty() {
        return Err("the peer lists no key that is not revoked, so nothing was pinned".to_owned());
    }
    let other = config.peers.iter().find(|(name, pinned)| {
        name.as_str() != peer && pinned.keys.iter().any(|key| chosen.contains(key))
    });
    if let Some((other, _)) = other {
        return Err(format!(
            "the peer {other} pins one of these keys already, and a key stands for one peer, so nothing was pinned"
        ));
    }
    Ok(chosen)
}

/// Whether the operator, asked through `answers`, confirms pinning `count`
/// keys for the peer `name` in `path`.
async fn confirmed(
    name: &str,
    path: &str,
    count: usize,
    answers: &mut (dyn AsyncBufRead + Send + Unpin),
    out: &mut dyn Write,
) -> bool {
    let _ = write!(
        out,
        "Compare each fingerprint with one the peer's operator gave you. Pin these {count} keys for the peer {name} in {path}? [y/N] "
    );
    let _ = out.flush();
    let mut answer = String::new();
    let _ = answers.read_line(&mut answer).await;
    YES.contains(&answer.trim().to_ascii_lowercase().as_str())
}

/// Each peer with its pinned keys.
fn list(config: &Config, out: &mut dyn Write) -> u8 {
    if config.peers.is_empty() {
        say(out, "The config names no peer.");
    }
    for (name, peer) in &config.peers {
        say(
            out,
            &format!("{name} {} (rotation {})", peer.url, peer.rotation.name()),
        );
        for key in &peer.keys {
            say(out, &shown(key));
        }
    }
    DONE
}

/// Each peer, or the one `name` names, read and checked against its pinned
/// keys, naming each key set that changed.
async fn test(
    config: &Config,
    name: Option<&str>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let peers: Vec<(&String, &Peer)> = config
        .peers
        .iter()
        .filter(|(named, _)| name.is_none_or(|name| *named == name))
        .collect();
    if peers.is_empty() {
        return Err(name.map_or_else(
            || "the config names no peer to test".to_owned(),
            |name| format!("the config names no peer {name}"),
        ));
    }
    let mut code = DONE;
    for (name, peer) in peers {
        let reached = read(
            config,
            connect,
            (&peer.discovery(), &peer.domain, peer.most),
            Some(&peer.keys),
        )
        .await;
        let found = match reached {
            Ok(found) => found,
            Err(why) => {
                say(out, &format!("{name}: {why}"));
                code = STOPPED;
                continue;
            }
        };
        let added: Vec<&Key> = found
            .set
            .keys
            .iter()
            .filter(|key| !peer.keys.contains(key))
            .collect();
        let dropped: Vec<&Key> = peer
            .keys
            .iter()
            .filter(|key| !found.set.keys.contains(key))
            .collect();
        say(
            out,
            &format!(
                "{name}: its answer is signed by {}, a pinned key; its key set is under sequence {}",
                fingerprint(&found.by),
                found.set.sequence
            ),
        );
        for (keys, said) in [
            (&added, "lists a key not pinned:"),
            (&dropped, "no longer lists a pinned key:"),
        ] {
            for key in keys {
                say(out, &format!("{name}: {said}"));
                say(out, &shown(key));
            }
        }
        if !added.is_empty() || !dropped.is_empty() {
            say(
                out,
                &format!(
                    "{name}: pin its keys anew with `herald peer add {} --name {name} --update`",
                    peer.url
                ),
            );
            code = STOPPED;
        }
    }
    Ok(code)
}

/// The peer `name` no longer trusted, or only its key `key`: taken from
/// `path` and added to `[federation] revoked`.
fn revoke(
    config: &Config,
    path: &str,
    name: &str,
    key: Option<&str>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let peer = config
        .peers
        .get(name)
        .ok_or_else(|| format!("the config names no peer {name}"))?;
    let pinned: Vec<String> = peer.keys.iter().map(ed25519_public).collect();
    let keys: Vec<String> = match key {
        Some(key) => {
            let found = peer
                .keys
                .iter()
                .find(|pinned| ed25519_public(pinned) == key || fingerprint(pinned) == key)
                .ok_or_else(|| format!("the peer {name} pins no key {key}"))?;
            vec![ed25519_public(found)]
        }
        None => pinned.clone(),
    };
    rewrite(path, written::revoked(&text(path)?, name, &keys)?)?;
    let removed = if keys.len() == pinned.len() {
        format!("removed [peers.{name}]")
    } else {
        format!("kept [peers.{name}] with its other keys")
    };
    say(
        out,
        &format!(
            "Revoked {} keys of the peer {name} in {path}: {removed}, and added them to [federation] revoked.",
            keys.len()
        ),
    );
    Ok(DONE)
}
