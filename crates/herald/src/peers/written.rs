//! `herald.toml` edited in place: a peer's table written or removed, and
//! revoked keys added to `[federation] revoked`, every other line kept as
//! it was.

use toml_edit::{Array, DocumentMut, Item, Table, TableLike, value};

/// The table every peer sits under.
const PEERS: &str = "peers";
/// The table the instance's own settings sit under.
const FEDERATION: &str = "federation";
/// The key naming a peer's address.
const URL: &str = "url";
/// The key naming a peer's pinned keys.
const KEYS: &str = "keys";
/// The key naming the keys never trusted again.
const REVOKED: &str = "revoked";

/// `text` read as a TOML document.
fn document(text: &str) -> Result<DocumentMut, String> {
    text.parse::<DocumentMut>()
        .map_err(|error| error.to_string())
}

/// The table `name` of `document`, made where it is absent, as a table
/// under its parent's header where `implicit`.
fn table_of<'d>(
    document: &'d mut Table,
    name: &str,
    implicit: bool,
) -> Result<&'d mut dyn TableLike, String> {
    let entry = document.entry(name).or_insert_with(|| {
        let mut table = Table::new();
        table.set_implicit(implicit);
        Item::Table(table)
    });
    entry
        .as_table_like_mut()
        .ok_or_else(|| format!("{name} is not written as a table; edit it by hand"))
}

/// `keys` as a TOML array.
fn array(keys: &[String]) -> Array {
    keys.iter().map(String::as_str).collect()
}

/// `[peers.<name>]` as herald writes it, pinning `keys` at `url`.
pub(crate) fn table(name: &str, url: &str, keys: &[String]) -> String {
    let mut document = DocumentMut::new();
    let _ = pin(&mut document, name, url, keys);
    document.to_string()
}

/// `document` with `[peers.<name>]` at `url` pinning `keys`: a new table,
/// or the one already there with its `url` and `keys` replaced and every
/// other setting kept.
fn pin(document: &mut DocumentMut, name: &str, url: &str, keys: &[String]) -> Result<(), String> {
    let peers = table_of(document.as_table_mut(), PEERS, true)?;
    let peer = peers.entry(name).or_insert(Item::Table(Table::new()));
    let peer = peer
        .as_table_like_mut()
        .ok_or_else(|| format!("{PEERS}.{name} is not written as a table; edit it by hand"))?;
    peer.insert(URL, value(url));
    peer.insert(KEYS, value(array(keys)));
    Ok(())
}

/// `text` with `[peers.<name>]` at `url` pinning `keys`.
///
/// # Errors
///
/// `text` is no TOML, or writes `peers` or the peer as something other than
/// a table.
pub(crate) fn pinned(text: &str, name: &str, url: &str, keys: &[String]) -> Result<String, String> {
    let mut document = document(text)?;
    pin(&mut document, name, url, keys)?;
    Ok(document.to_string())
}

/// `text` with `keys` taken from `[peers.<name>]`, the table removed where
/// it pins no other, and each added once to `[federation] revoked`.
///
/// # Errors
///
/// `text` is no TOML, writes `federation`, `peers`, the peer or a list as
/// something else, or names no such peer.
pub(crate) fn revoked(text: &str, name: &str, keys: &[String]) -> Result<String, String> {
    let mut document = document(text)?;
    let peers = table_of(document.as_table_mut(), PEERS, true)?;
    let peer = peers
        .get_mut(name)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| format!("herald.toml names no peer {name}"))?;
    let kept: Vec<String> = peer
        .get(KEYS)
        .and_then(Item::as_array)
        .into_iter()
        .flatten()
        .filter_map(|key| key.as_str())
        .filter(|key| !keys.iter().any(|revoked| revoked == key))
        .map(str::to_owned)
        .collect();
    if kept.is_empty() {
        peers.remove(name);
    } else {
        peer.insert(KEYS, value(array(&kept)));
    }
    let federation = table_of(document.as_table_mut(), FEDERATION, false)?;
    let mut listed: Vec<String> = federation
        .get(REVOKED)
        .and_then(Item::as_array)
        .into_iter()
        .flatten()
        .filter_map(|key| key.as_str().map(str::to_owned))
        .collect();
    for key in keys {
        if !listed.contains(key) {
            listed.push(key.clone());
        }
    }
    federation.insert(REVOKED, value(array(&listed)));
    Ok(document.to_string())
}

#[cfg(test)]
mod tests;
