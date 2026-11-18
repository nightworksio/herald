use herald_config::GithubApp;
use herald_core::SecretRef;
use herald_flows::Connected;
use herald_ports::{Algorithm, CryptoError, Secret, Signer, Tokens as _};

use super::*;

const LOOKUP: &str = "/orgs/nightworksio/installation";
const AS_USER: &str = "/users/nightworksio/installation";
const MINT: &str = "/app/installations/42/access_tokens";

/// The owner source reading as the App `herald`, whose key is the secret
/// `key`.
fn installed(key: &str) -> Config {
    installed_keeping(key, false)
}

/// [`installed`], its tokens kept in the store where `store_token` says.
fn installed_keeping(key: &str, store_token: bool) -> Config {
    let mut config = owned(|options| {
        options.app = Some("herald".to_owned());
        options.transport = Some(herald_sources_forge::Transport::Rest);
    });
    config.github_apps.insert(
        "herald".to_owned(),
        GithubApp {
            issuer: "Iv23liABCDEF".to_owned(),
            private_key: SecretRef {
                secret: key.to_owned(),
            },
            api: "https://api.github.com".to_owned(),
            webhook: None,
            store_token,
        },
    );
    config
}

/// A signer that signs as [`FakeSigner`] does and seals nothing.
struct Unsealing;

impl Signer for Unsealing {
    async fn sign(
        &self,
        algorithm: Algorithm,
        key: &Secret,
        message: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        FakeSigner.sign(algorithm, key, message).await
    }

    async fn public(&self, algorithm: Algorithm, key: &Secret) -> Result<Vec<u8>, CryptoError> {
        FakeSigner.public(algorithm, key).await
    }
}

fn token(expires: Timestamp) -> String {
    json!({ "token": "ghs_minted", "expires_at": expires.to_string() }).to_string()
}

fn connected(config: &Config, script: &Script) -> Connected<Script> {
    let prepared =
        pollster::block_on(prepare(config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap()
}

/// What a round of `connected` at `at` did, by member.
fn round(
    config: &Config,
    connected: &Connected<Script>,
    store: &Failing,
    at: Timestamp,
) -> Vec<(Option<String>, Sweep)> {
    round_signed(config, connected, (store, &FakeSigner), at)
}

/// [`round`], signing with `signer`.
fn round_signed<G: Signer>(
    config: &Config,
    connected: &Connected<Script>,
    (store, signer): (&Failing, &G),
    at: Timestamp,
) -> Vec<(Option<String>, Sweep)> {
    let clock = FakeClock::at(at);
    let ports = herald_flows::Ports {
        store,
        clock: &clock,
        signer,
        budget: Budget::UNBOUNDED,
    };
    pollster::block_on(poll(
        config,
        connected,
        &ports,
        &herald_testing::FakeVerifier,
        "r1",
    ))
    .into_iter()
    .map(|swept| (swept.member, swept.outcome))
    .collect()
}

fn answer(script: &Script, answers: &[(&str, u16, &str)]) {
    let mut queued = script.answers.lock().unwrap();
    for (path, status, body) in answers {
        queued.push_back(((*path).to_owned(), *status, (*body).to_owned()));
    }
}

#[test]
fn an_owner_reading_as_an_app_mints_a_token_once_and_again_shortly_before_it_expires() {
    let store = healthy();
    let config = installed("HERALD_APP_KEY");
    let script = Script::default();
    let connected = connected(&config, &script);
    let expires = NOON + SignedDuration::from_hours(1);
    let two = repositories(&[(1, "herald"), (2, "gate")]);
    let (herald, gate) = (
        releases("herald", &[("v1", NOON)]),
        releases("gate", &[("v1", NOON)]),
    );
    let (herald_described, gate_described) = (described(1, "herald"), described(2, "gate"));
    answer(
        &script,
        &[
            (LOOKUP, 200, r#"{"id":42}"#),
            (MINT, 201, &token(expires)),
            (LISTING, 200, &two),
            (&released("herald"), 200, &herald),
            ("/repos/nightworksio/herald", 200, &herald_described),
            (&released("gate"), 200, &gate),
            ("/repos/nightworksio/gate", 200, &gate_described),
        ],
    );
    assert_eq!(
        round(&config, &connected, &store, NOON),
        [
            listed(2, &["herald", "gate"], &[]),
            member("herald", first(1)),
            member("gate", first(1)),
        ]
    );
    let unchanged = [
        (LISTING, 200, &two[..]),
        (&released("herald")[..], 304, ""),
        (&released("gate")[..], 304, ""),
    ];
    let quiet = [
        listed(2, &[], &[]),
        member("herald", Sweep::Unchanged),
        member("gate", Sweep::Unchanged),
    ];
    answer(&script, &unchanged);
    let fresh = expires - herald_flows::MINTED_AHEAD - SignedDuration::from_secs(1);
    assert_eq!(round(&config, &connected, &store, fresh), quiet);
    answer(
        &script,
        &[(MINT, 201, &token(expires + SignedDuration::from_hours(1)))],
    );
    answer(&script, &unchanged);
    assert_eq!(
        round(
            &config,
            &connected,
            &store,
            expires - herald_flows::MINTED_AHEAD
        ),
        quiet
    );
    assert!(script.answers.lock().unwrap().is_empty());
}

#[test]
fn an_owner_whose_app_mints_no_token_is_not_polled_that_round_and_says_why() {
    let store = healthy();
    let config = installed("HERALD_APP_KEY");
    let script = Script::answering(&[(LOOKUP, 404, "{}"), (AS_USER, 404, "{}")]);
    assert_eq!(
        round(&config, &connected(&config, &script), &store, NOON),
        [(
            None,
            Sweep::Stopped(Stop::Source(
                "the App's installation could not be read: the App is not installed there; install it on the owner or the repository".to_owned()
            ))
        )]
    );
    let unreachable = Script::default();
    assert!(matches!(
        &round(&config, &connected(&config, &unreachable), &store, NOON)[..],
        [(None, Sweep::Stopped(Stop::Unreachable(_)))]
    ));
    let unsigned = installed("BAD");
    assert_eq!(
        round(&unsigned, &connected(&unsigned, &Script::default()), &store, NOON),
        [(
            None,
            Sweep::Stopped(Stop::Source(
                "the GitHub App herald's private key signs nothing: the secret BAD is not a key for this signature; give a private key as PEM, PKCS#8 or an RSA key's PKCS#1".to_owned()
            ))
        )]
    );
    let healed = Script::default();
    let connected = connected(&config, &healed);
    answer(&healed, &[(LOOKUP, 500, "{}")]);
    assert!(matches!(
        &round(&config, &connected, &store, NOON)[..],
        [(None, Sweep::Stopped(Stop::Source(_)))]
    ));
    answer(
        &healed,
        &[
            (LOOKUP, 200, r#"{"id":42}"#),
            (MINT, 201, &token(NOON + SignedDuration::from_hours(1))),
            (LISTING, 200, "[]"),
        ],
    );
    assert_eq!(
        round(&config, &connected, &store, NOON),
        [listed(0, &[], &[])]
    );
}

#[test]
fn doctor_online_mints_the_app_s_token_before_it_lists_the_owner() {
    let config = installed("HERALD_APP_KEY");
    let probes = [
        ("/users/@me", 200, r#"{"id":"99"}"#),
        (
            "/channels/1566200000000000100",
            200,
            r#"{"id":"1566200000000000100","guild_id":"1","type":5}"#,
        ),
    ];
    let online = |answers: &[(&str, u16, &str)]| {
        let mut all = probes.to_vec();
        all.extend_from_slice(answers);
        let script = Script::answering(&all);
        let found = pollster::block_on(herald_flows::online(
            &connected(&config, &script),
            (&FakeSigner, NOON),
        ));
        (found, script)
    };
    let (unminted, _) = online(&[(LOOKUP, 404, "{}"), (AS_USER, 404, "{}")]);
    let gap = unminted
        .gaps
        .iter()
        .find(|gap| gap.key == "sources.gate.app")
        .unwrap();
    assert_eq!(
        gap.feature,
        "minting the token the source \"gate\" reads with"
    );
    assert!(gap.why.contains("not installed there"), "{}", gap.why);
    let token = token(NOON + SignedDuration::from_hours(1));
    let (minted, script) = online(&[
        (LOOKUP, 200, r#"{"id":42}"#),
        (MINT, 201, &token),
        (LISTING, 200, "[]"),
    ]);
    assert!(minted.gaps.iter().all(|gap| gap.key != "sources.gate.app"));
    assert!(
        script
            .seen()
            .last()
            .unwrap()
            .contains("/orgs/nightworksio/repos")
    );
}

#[test]
fn an_app_keeping_its_tokens_mints_none_while_the_kept_one_is_good_and_again_once_tampered_or_due()
{
    let mut store = healthy();
    let config = installed_keeping("HERALD_APP_KEY", true);
    let name = "github_apps.herald/owner/nightworksio";
    let expires = NOON + SignedDuration::from_hours(1);
    let run = |store: &Failing, at: Timestamp, answers: &[(&str, u16, &str)]| {
        let script = Script::default();
        let connected = connected(&config, &script);
        answer(&script, answers);
        let swept = round(&config, &connected, store, at);
        assert!(script.answers.lock().unwrap().is_empty());
        swept
    };
    let minted = token(expires);
    let minting = [
        (LOOKUP, 200, r#"{"id":42}"#),
        (MINT, 201, minted.as_str()),
        (LISTING, 200, "[]"),
    ];
    assert_eq!(run(&store, NOON, &minting), [listed(0, &[], &[])]);
    let kept = pollster::block_on(store.inner.token(name))
        .unwrap()
        .unwrap();
    assert_eq!(kept.value.expires, expires);
    let sealed = kept.value.sealed_bytes().unwrap();
    assert_eq!(
        sealed,
        format!("sealed|the App's key|herald github-app installation token v1|{name}|{expires}|ghs_minted").as_bytes()
    );
    let later = NOON + SignedDuration::from_mins(10);
    assert_eq!(
        run(&store, later, &[(LISTING, 200, "[]")]),
        [listed(0, &[], &[])]
    );
    let tampered = herald_core::KeptToken {
        sealed: "dGFtcGVyZWQ=".to_owned(),
        ..kept.value.clone()
    };
    pollster::block_on(store.inner.put_token(&tampered, Some(kept.version))).unwrap();
    run(&store, later, &minting);
    let rekept = pollster::block_on(store.inner.token(name))
        .unwrap()
        .unwrap();
    assert_ne!(rekept.value.sealed, tampered.sealed);
    let rewrite = |value: herald_core::KeptToken| {
        let version = pollster::block_on(store.inner.token(name))
            .unwrap()
            .map(|kept| kept.version);
        pollster::block_on(store.inner.put_token(&value, version)).unwrap();
    };
    let elsewhere = format!("github_apps.herald/owner/other|{expires}");
    let moved = pollster::block_on(FakeSigner.seal(
        &Secret::new("HERALD_APP_KEY", "the App's key"),
        herald_ports::Sealing {
            purpose: "herald github-app installation token v1",
            bound: elsewhere.as_bytes(),
        },
        b"ghs_other",
    ))
    .unwrap();
    rewrite(herald_core::KeptToken::new(
        name.to_owned(),
        &moved,
        expires,
    ));
    run(&store, later, &minting);
    let extended = herald_core::KeptToken {
        expires: expires + SignedDuration::from_hours(5),
        ..pollster::block_on(store.inner.token(name))
            .unwrap()
            .unwrap()
            .value
    };
    rewrite(extended);
    run(&store, later, &minting);
    run(&store, expires - herald_flows::MINTED_AHEAD, &minting);
    store.fault = Fault::ReadsTokens;
    run(&store, later, &minting);
}

#[test]
fn a_token_the_store_does_not_keep_is_noted_and_the_round_still_announces_with_it() {
    let mut store = healthy();
    let config = installed_keeping("HERALD_APP_KEY", true);
    let expires = NOON + SignedDuration::from_hours(1);
    let due = expires - herald_flows::MINTED_AHEAD;
    let run = |store: &Failing, at: Timestamp, answers: &[(&str, u16, &str)]| {
        let script = Script::default();
        let connected = connected(&config, &script);
        answer(&script, answers);
        let swept = round(&config, &connected, store, at);
        assert!(script.answers.lock().unwrap().is_empty());
        swept
    };
    let one = repositories(&[(1, "herald")]);
    let minted = token(expires);
    assert_eq!(
        run(
            &store,
            NOON,
            &[
                (LOOKUP, 200, r#"{"id":42}"#),
                (MINT, 201, &minted),
                (LISTING, 200, &one),
                (
                    &released("herald"),
                    200,
                    &releases("herald", &[("v1", NOON)])
                ),
                ("/repos/nightworksio/herald", 200, &described(1, "herald")),
            ]
        ),
        [listed(1, &["herald"], &[]), member("herald", first(1))]
    );
    store.fault = Fault::WritesTokens;
    let newer = releases("herald", &[("v2", due), ("v1", NOON)]);
    let reminted = token(expires + SignedDuration::from_hours(1));
    let swept = run(
        &store,
        due,
        &[
            (LOOKUP, 200, r#"{"id":42}"#),
            (MINT, 201, &reminted),
            (LISTING, 200, &one),
            (&released("herald"), 200, &newer),
            ("/repos/nightworksio/herald", 200, &described(1, "herald")),
            (
                "/channels/1566200000000000100/messages",
                200,
                r#"{"id":"3"}"#,
            ),
        ],
    );
    assert_eq!(
        swept[..2],
        [
            (
                None,
                Sweep::Unkept(
                    "the sqlite store refused it: the store could not be reached: broken"
                        .to_owned()
                )
            ),
            listed(1, &[], &[]),
        ]
    );
    let [(_, Sweep::Polled { announced, .. })] = &swept[2..] else {
        panic!("{swept:?}");
    };
    assert_eq!(announced.len(), 1);
    assert!(matches!(
        &announced[0].announced[0].outcome,
        herald_flows::Outcome::Stopped { landed, .. } if landed[..] == [herald_core::Step::Post]
    ));
    assert!(!format!("{swept:?}").contains("ghs_minted"));
}

#[test]
fn a_token_that_cannot_be_sealed_is_noted_and_read_with_unkept() {
    let store = healthy();
    let config = installed_keeping("HERALD_APP_KEY", true);
    let script = Script::default();
    let connected = connected(&config, &script);
    answer(
        &script,
        &[
            (LOOKUP, 200, r#"{"id":42}"#),
            (MINT, 201, &token(NOON + SignedDuration::from_hours(1))),
            (LISTING, 200, "[]"),
        ],
    );
    assert_eq!(
        round_signed(&config, &connected, (&store, &Unsealing), NOON),
        [
            (
                None,
                Sweep::Unkept(
                    "it could not be sealed: nothing can be sealed under the secret HERALD_APP_KEY on this host"
                        .to_owned()
                )
            ),
            listed(0, &[], &[]),
        ]
    );
    assert_eq!(
        pollster::block_on(store.inner.token("github_apps.herald/owner/nightworksio")).unwrap(),
        None
    );
}

#[test]
fn doctor_online_names_an_app_whose_tokens_the_store_does_not_keep() {
    let config = installed_keeping("HERALD_APP_KEY", true);
    let minting = [
        (LOOKUP, 200, r#"{"id":42}"#),
        (MINT, 201, &token(NOON + SignedDuration::from_hours(1))[..]),
    ];
    let kept = |store: &Failing| {
        let script = Script::answering(&minting);
        pollster::block_on(herald_flows::kept_tokens(
            &connected(&config, &script),
            (&FakeSigner, Some(store), NOON),
            "sqlite",
        ))
    };
    let mut store = healthy();
    store.fault = Fault::WritesTokens;
    let refused = kept(&store);
    assert_eq!(
        refused
            .gaps
            .iter()
            .map(|gap| (&gap.key[..], &gap.feature[..], &gap.why[..]))
            .collect::<Vec<_>>(),
        [(
            "github_apps.herald.store_token",
            "keeping the GitHub App herald's installation tokens",
            "the sqlite store refused it: the store could not be reached: broken"
        )]
    );
    store.fault = Fault::Nothing;
    assert_eq!(kept(&store).gaps, []);
    assert!(
        pollster::block_on(store.inner.token("github_apps.herald/owner/nightworksio"))
            .unwrap()
            .is_some()
    );
}

/// `config` with the source `name` of the repository `nightworksio/cli`,
/// reading as the App `app`.
fn reading(config: &mut Config, name: &str, app: &str) {
    let mut forged = config.sources[&SourceName::new("gate")]
        .of::<herald_sources_forge::ForgeSource>()
        .unwrap()
        .clone();
    forged.options.owner = None;
    forged.options.repository = Some("nightworksio/cli".to_owned());
    forged.options.app = Some(app.to_owned());
    config
        .sources
        .insert(SourceName::new(name), herald_config::Source::new(forged));
}

#[test]
fn sources_reading_one_installation_of_one_app_share_its_token() {
    let mut config = installed("HERALD_APP_KEY");
    let second = config.github_apps["herald"].clone();
    config.github_apps.insert("second".to_owned(), second);
    reading(&mut config, "again", "herald");
    reading(&mut config, "cli", "herald");
    reading(&mut config, "other", "second");
    let repository = "/repos/nightworksio/cli/installation";
    let minted = token(NOON + SignedDuration::from_hours(1));
    let script = Script::answering(&[
        (repository, 200, r#"{"id":42}"#),
        (MINT, 201, &minted),
        (LOOKUP, 200, r#"{"id":42}"#),
        (MINT, 201, &minted),
        (repository, 200, r#"{"id":42}"#),
        (MINT, 201, &minted),
    ]);
    let found = pollster::block_on(herald_flows::kept_tokens(
        &connected(&config, &script),
        (&FakeSigner, Some(&healthy()), NOON),
        "sqlite",
    ));
    assert_eq!(found.gaps, []);
    assert!(script.answers.lock().unwrap().is_empty());
}

#[test]
fn a_fixed_installation_is_minted_for_at_once_and_its_token_kept_under_its_id() {
    let store = healthy();
    let mut config = installed_keeping("HERALD_APP_KEY", true);
    let mut forged = config.sources[&SourceName::new("gate")]
        .of::<herald_sources_forge::ForgeSource>()
        .unwrap()
        .clone();
    forged.options.installation = std::num::NonZeroU64::new(42);
    config
        .sources
        .insert(SourceName::new("gate"), herald_config::Source::new(forged));
    let script = Script::answering(&[(MINT, 201, &token(NOON + SignedDuration::from_hours(1)))]);
    let found = pollster::block_on(herald_flows::kept_tokens(
        &connected(&config, &script),
        (&FakeSigner, Some(&store), NOON),
        "sqlite",
    ));
    assert_eq!(found.gaps, []);
    assert!(
        pollster::block_on(store.inner.token("github_apps.herald/installation/42"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_source_naming_an_app_the_config_lacks_reads_without_a_token() {
    let store = healthy();
    let mut config = installed("HERALD_APP_KEY");
    config.github_apps.clear();
    let script = Script::answering(&[(LISTING, 200, "[]")]);
    assert_eq!(
        round(&config, &connected(&config, &script), &store, NOON),
        [listed(0, &[], &[])]
    );
    assert!(script.answers.lock().unwrap().is_empty());
    let mut unread = healthy();
    unread.fault = Fault::ReadsMembers;
    let swept = round(
        &config,
        &connected(&config, &Script::default()),
        &unread,
        NOON,
    );
    assert!(
        matches!(&swept[..], [(None, Sweep::Stopped(Stop::Store(_)))]),
        "{swept:?}"
    );
}

#[test]
fn a_server_reads_no_webhook_secret_for_an_app_that_names_none() {
    let config = installed("HERALD_APP_KEY");
    assert!(pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Serves)).is_ok());
}
