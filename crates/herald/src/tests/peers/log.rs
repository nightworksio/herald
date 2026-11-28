//! `herald peer log`: each peer's requests newest first, from the audit
//! the store keeps of each day.

use herald_compose::{Host, Hosting, ServingOn, Through};
use herald_config::Uses;
use herald_core::{Action, AuditDay, Audited};
use herald_ports::{Keyring, Published as _, Store};

use super::*;

struct Audit(Vec<AuditDay>);

impl<H: Host> Through<H> for Audit {
    type Done = ();

    async fn through<S: Store, K: Keyring>(self, serving: &ServingOn<'_, H, S, K>, _: &str) {
        let kept = serving.ports.store.published().unwrap();
        for day in &self.0 {
            kept.put_audit(day, None).await.unwrap();
        }
    }
}

fn entry(at: &str, peer: Option<&str>, outcome: u16) -> Audited {
    Audited {
        at: at.parse().unwrap(),
        peer: peer.map(str::to_owned),
        keyid: peer
            .is_none()
            .then(|| "did:web:stranger.example#key".to_owned()),
        action: Action::Push,
        resource: "sources.inbox".to_owned(),
        outcome,
        reason: "taken".to_owned(),
        evidence: std::collections::BTreeMap::new(),
        identity: None,
        digest: None,
    }
}

fn day(peer: &str, day: &str, entries: Vec<Audited>, more: u64) -> AuditDay {
    AuditDay {
        peer: peer.to_owned(),
        day: day.to_owned(),
        entries,
        more,
    }
}

fn audited(setup: &Setup, world: &World) {
    written(
        setup,
        world,
        vec![
            day(
                "org",
                "2027-01-14",
                vec![Audited {
                    keyid: Some("oidc https://issuer.example".to_owned()),
                    evidence: std::collections::BTreeMap::from([
                        ("iss".to_owned(), "https://issuer.example".to_owned()),
                        ("repository_id".to_owned(), "42".to_owned()),
                    ]),
                    identity: Some("github.com/1/v1".to_owned()),
                    digest: Some("sha-256=:YWI=:".to_owned()),
                    ..entry("2027-01-14T10:00:00Z", Some("org"), 202)
                }],
                0,
            ),
            day(
                "org",
                "2027-01-15",
                vec![entry("2027-01-15T07:00:00Z", Some("org"), 403)],
                2,
            ),
            day(
                AuditDay::STRANGERS,
                "2027-01-15",
                vec![entry("2027-01-15T06:00:00Z", None, 401)],
                0,
            ),
        ],
    );
}

/// The audit `days`, kept in the store `setup` names.
fn written(setup: &Setup, world: &World, days: Vec<AuditDay>) {
    let config = crate::read(&path_of(setup), &std::collections::BTreeMap::new()).unwrap();
    let connect =
        |_: herald_ports::Base<'_>, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let waiting = |_: &str, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let connector = Connector::new(&connect, &waiting);
    on_tokio(async {
        let composed = crate::stores::composed(
            &config,
            &setup.vars,
            &connector,
            Hosting::Actions,
            Uses::Polls,
            "test",
        )
        .await
        .unwrap();
        composed.run(Audit(days)).await;
    });
}

fn logged(setup: &Setup, world: &World, args: &[&str]) -> Result<(u8, String), String> {
    let path = path_of(setup);
    let config = crate::read(&path, &std::collections::BTreeMap::new()).unwrap();
    let mut all = vec!["herald", "peer", "log"];
    all.extend(args);
    let crate::cli::Command::Read(Reading::Peer(asked)) =
        crate::cli::Cli::try_parse_from(all).unwrap().command
    else {
        panic!("not a peer command");
    };
    let connect =
        |_: herald_ports::Base<'_>, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let waiting = |_: &str, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let connector = Connector::new(&connect, &waiting);
    let mut out = Vec::new();
    let code = on_tokio(crate::peers::peering(
        &config,
        &path,
        &asked,
        (&setup.vars, &connector),
        &mut "".as_bytes(),
        &mut out,
    ))?;
    Ok((code, String::from_utf8(out).unwrap()))
}

#[test]
fn each_peer_s_requests_are_shown_newest_first_and_the_busy_days_counted() {
    let (setup, world) = peered(&handed::signed(), &[OLDER]);
    audited(&setup, &world);
    let (code, out) = logged(&setup, &world, &[]).unwrap();
    assert_eq!(code, DONE);
    assert_eq!(
        out,
        "2027-01-15T07:00:00Z org push sources.inbox 403 taken\n\
         2027-01-15T06:00:00Z did:web:stranger.example#key push sources.inbox 401 taken\n\
         2027-01-14T10:00:00Z org push sources.inbox 202 taken (iss=https://issuer.example, repository_id=42, envelope github.com/1/v1, digest sha-256=:YWI=:)\n\
         2 more requests were counted on busy days but not kept.\n"
    );
    let (_, one) = logged(
        &setup,
        &world,
        &["org", "--since", "2027-01-15", "--limit", "1"],
    )
    .unwrap();
    assert_eq!(
        one,
        "2027-01-15T07:00:00Z org push sources.inbox 403 taken\n2 more requests were counted on busy days but not kept.\n"
    );
    let (_, quiet) = logged(&setup, &world, &["-"]).unwrap();
    assert_eq!(
        quiet,
        "2027-01-15T06:00:00Z did:web:stranger.example#key push sources.inbox 401 taken\n"
    );
    let (_, json) = logged(&setup, &world, &["-", "--format", "json"]).unwrap();
    let read: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(read[0]["outcome"], 401);
    assert_eq!(read[0]["keyid"], "did:web:stranger.example#key");
    assert_eq!(read.as_array().unwrap().len(), 1);
    assert_eq!(
        logged(&setup, &world, &["nobody"]).unwrap_err(),
        "there is no peer nobody in [peers], and no app stands for it"
    );
    assert!(
        logged(&setup, &world, &["--since", "soon"])
            .unwrap_err()
            .starts_with("--since soon is no day")
    );
    world.state.lock().unwrap().failing = Some(herald_ports::Method::Get);
    let unread = logged(&setup, &world, &[]).unwrap_err();
    assert!(unread.contains("500"), "{unread}");
}

#[test]
fn a_name_an_app_stands_for_is_logged_as_a_peer_is() {
    let (setup, world) = super::doctor::federated();
    written(
        &setup,
        &world,
        vec![day(
            "runs-ci",
            "2027-01-14",
            vec![entry("2027-01-14T10:00:00Z", Some("runs-ci"), 202)],
            0,
        )],
    );
    let (_, one) = logged(&setup, &world, &["runs-ci"]).unwrap();
    assert_eq!(
        one,
        "2027-01-14T10:00:00Z runs-ci push sources.inbox 202 taken\n"
    );
    let (_, all) = logged(&setup, &world, &[]).unwrap();
    assert_eq!(all, one);
}
