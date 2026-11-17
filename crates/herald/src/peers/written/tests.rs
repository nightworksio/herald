use super::*;

const KEYS: [&str; 2] = ["z6MkOne", "z6MkTwo"];

fn keys(written: &[&str]) -> Vec<String> {
    written.iter().map(|key| (*key).to_owned()).collect()
}

#[test]
fn a_new_peer_is_written_under_its_own_header_after_every_other_line() {
    let text = "version = 1 # kept\n\n[store]\nkind = \"markers\"\n";
    let pinned = pinned(text, "org", "https://herald.example", &keys(&KEYS)).unwrap();
    assert_eq!(
        pinned,
        format!(
            "{text}\n[peers.org]\nurl = \"https://herald.example\"\nkeys = [\"z6MkOne\", \"z6MkTwo\"]\n"
        )
    );
}

#[test]
fn a_peer_written_already_has_its_url_and_keys_replaced_and_the_rest_kept() {
    let text = "[peers.org]\nurl = \"https://old.example\"\nkeys = [\"z6MkOld\"]\nrate = \"60/m\" # slow\n";
    let pinned = pinned(text, "org", "https://herald.example", &keys(&KEYS[..1])).unwrap();
    assert_eq!(
        pinned,
        "[peers.org]\nurl = \"https://herald.example\"\nkeys = [\"z6MkOne\"]\nrate = \"60/m\" # slow\n"
    );
    let inline = "peers = { org = { url = \"https://old.example\", keys = [] } }\n";
    assert_eq!(
        super::pinned(inline, "org", "https://herald.example", &keys(&KEYS[..1])).unwrap(),
        "peers = { org = { url = \"https://herald.example\", keys = [\"z6MkOne\"] } }\n"
    );
}

#[test]
fn what_is_no_table_or_no_toml_is_left_to_the_operator() {
    for (text, said) in [
        (
            "peers = 1\n",
            "peers is not written as a table; edit it by hand",
        ),
        (
            "[peers]\norg = 1\n",
            "peers.org is not written as a table; edit it by hand",
        ),
    ] {
        assert_eq!(
            pinned(text, "org", "https://herald.example", &[]).unwrap_err(),
            said
        );
    }
    assert!(pinned("[peers", "org", "https://herald.example", &[]).is_err());
}

#[test]
fn a_revoked_peer_leaves_and_its_keys_join_the_revoked_list_once() {
    let text = "[federation]\ndomain = \"herald.example\"\nrevoked = [\"z6MkTwo\"]\n\n[peers.org]\nurl = \"https://herald.example\"\nkeys = [\"z6MkOne\", \"z6MkTwo\"]\n";
    assert_eq!(
        revoked(text, "org", &keys(&KEYS)).unwrap(),
        "[federation]\ndomain = \"herald.example\"\nrevoked = [\"z6MkTwo\", \"z6MkOne\"]\n"
    );
}

#[test]
fn a_revoked_key_leaves_a_peer_that_pins_another() {
    let text = "[peers.org]\nurl = \"https://herald.example\"\nkeys = [\"z6MkOne\", \"z6MkTwo\"]\n";
    assert_eq!(
        revoked(text, "org", &keys(&KEYS[1..])).unwrap(),
        "[peers.org]\nurl = \"https://herald.example\"\nkeys = [\"z6MkOne\"]\n\n[federation]\nrevoked = [\"z6MkTwo\"]\n"
    );
}

#[test]
fn revoking_a_peer_not_written_is_refused() {
    assert_eq!(
        revoked("version = 1\n", "org", &keys(&KEYS)).unwrap_err(),
        "herald.toml names no peer org"
    );
    assert_eq!(
        revoked(
            "[peers.org]\nkeys = [\"z6MkOne\"]\n[federation]\nrevoked = 1\n",
            "org",
            &keys(&KEYS[..1])
        )
        .unwrap(),
        "[federation]\nrevoked = [\"z6MkOne\"]\n"
    );
    assert_eq!(
        revoked(
            "federation = 1\n[peers.org]\nkeys = [\"z6MkOne\"]\n",
            "org",
            &keys(&KEYS[..1])
        )
        .unwrap_err(),
        "federation is not written as a table; edit it by hand"
    );
    assert_eq!(
        revoked("[peers", "org", &keys(&KEYS))
            .unwrap_err()
            .lines()
            .next()
            .map(str::to_owned),
        "[peers"
            .parse::<DocumentMut>()
            .unwrap_err()
            .to_string()
            .lines()
            .next()
            .map(str::to_owned)
    );
}
