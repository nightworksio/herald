use super::fixtures::{ACTIVITYPUB, APP, DISCORD, FORGEJO, GITHUB, GITLAB, MATRIX, SLACK};
use super::*;
use crate::bounds::Inbound;

const BODY: &[u8] = b"{\"action\":\"published\"}";

fn now() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

fn claimed(scheme: Scheme, headers: &[(&str, &str)]) -> Result<Claim, Refusal> {
    claim(scheme, headers, BODY, now(), &Inbound::DEFAULT)
}

#[test]
fn a_github_webhook_claims_an_hmac_of_its_body() {
    let claim = claimed(
        GITHUB,
        &[
            ("x-hub-signature-256", "sha256=00Ff10"),
            ("x-github-delivery", "72d3162e-cc78-11e3-81ab-4c9367dc0958"),
        ],
    )
    .unwrap();
    assert_eq!(
        claim,
        Claim {
            kind: Signature::HmacSha256,
            message: BODY.to_vec(),
            signatures: vec![vec![0x00, 0xff, 0x10]],
            delivery: Some("00ff10".to_owned()),
            signer: None,
        }
    );
}

#[test]
fn a_github_webhook_without_its_signature_or_its_prefix_is_refused() {
    assert_eq!(
        claimed(GITHUB, &[("X-Hub-Signature", "sha1=00")]),
        Err(Refusal::Unsigned {
            header: "X-Hub-Signature-256"
        })
    );
    for value in ["00ff", "sha256=", "sha256=0", "sha256=zz", "sha256=+1"] {
        assert_eq!(
            claimed(GITHUB, &[("X-Hub-Signature-256", value)]),
            Err(Refusal::Malformed {
                header: "X-Hub-Signature-256"
            }),
            "{value}"
        );
    }
}

#[test]
fn a_forgejo_webhook_claims_an_hmac_of_its_body_under_either_header() {
    for (header, delivered) in [
        ("X-Forgejo-Signature", "X-Forgejo-Delivery"),
        ("x-gitea-signature", "x-gitea-delivery"),
    ] {
        let claim = claimed(FORGEJO, &[(header, "abcd"), (delivered, "7")]).unwrap();
        assert_eq!(claim.kind, Signature::HmacSha256, "{header}");
        assert_eq!(claim.message, BODY, "{header}");
        assert_eq!(claim.signatures, [[0xab, 0xcd]], "{header}");
        assert_eq!(claim.delivery.as_deref(), Some("abcd"), "{header}");
    }
    assert_eq!(
        claimed(FORGEJO, &[("X-Gitea-Signature", "abc")]),
        Err(Refusal::Malformed {
            header: "X-Gitea-Signature"
        })
    );
    assert_eq!(
        claimed(FORGEJO, &[]),
        Err(Refusal::Unsigned {
            header: "X-Forgejo-Signature"
        })
    );
}

#[test]
fn a_forgejo_header_is_read_before_a_gitea_one() {
    let claim = claimed(
        FORGEJO,
        &[
            ("X-Gitea-Signature", "00"),
            ("X-Forgejo-Signature", "01"),
            ("X-Gitea-Delivery", "gitea"),
            ("X-Forgejo-Delivery", "forgejo"),
        ],
    )
    .unwrap();
    assert_eq!(claim.signatures, [[0x01]]);
    assert_eq!(claim.delivery.as_deref(), Some("01"));
}

#[test]
fn a_discord_interaction_claims_an_ed25519_signature_of_its_timestamp_and_body() {
    let claim = claimed(
        DISCORD,
        &[
            ("X-Signature-Ed25519", "0A0b"),
            ("X-Signature-Timestamp", "1800000000"),
        ],
    )
    .unwrap();
    assert_eq!(claim.kind, Signature::Ed25519);
    assert_eq!(claim.message, [b"1800000000".as_slice(), BODY].concat());
    assert_eq!(claim.signatures, [[0x0a, 0x0b]]);
    assert_eq!(claim.delivery.as_deref(), Some("0a0b"));
    assert_eq!(
        claimed(DISCORD, &[("X-Signature-Timestamp", "1800000000")]),
        Err(Refusal::Unsigned {
            header: "X-Signature-Ed25519"
        })
    );
    assert_eq!(
        claimed(
            DISCORD,
            &[
                ("X-Signature-Ed25519", "nope"),
                ("X-Signature-Timestamp", "1800000000"),
            ],
        ),
        Err(Refusal::Malformed {
            header: "X-Signature-Ed25519"
        })
    );
}

#[test]
fn a_slack_request_claims_an_hmac_of_its_versioned_base_string() {
    let claim = claimed(
        SLACK,
        &[
            ("X-Slack-Signature", "v0=ff"),
            ("X-Slack-Request-Timestamp", "1800000000"),
        ],
    )
    .unwrap();
    assert_eq!(claim.kind, Signature::HmacSha256);
    assert_eq!(claim.message, [b"v0:1800000000:".as_slice(), BODY].concat());
    assert_eq!(claim.signatures, [[0xff]]);
    assert_eq!(claim.delivery.as_deref(), Some("ff"));
    assert_eq!(
        claimed(
            SLACK,
            &[
                ("X-Slack-Signature", "ff"),
                ("X-Slack-Request-Timestamp", "1800000000"),
            ],
        ),
        Err(Refusal::Malformed {
            header: "X-Slack-Signature"
        })
    );
}

#[test]
fn a_timestamp_inside_the_window_either_way_is_taken() {
    for offset in [-300, 0, 300] {
        let stamp = (1_800_000_000 + offset).to_string();
        let headers = [
            ("X-Slack-Signature", "v0=ff"),
            ("X-Slack-Request-Timestamp", stamp.as_str()),
        ];
        assert_eq!(
            probe(claimed(SLACK, &headers)),
            format!("HmacSha256 v0:{stamp}:{{\"action\":\"published\"}}"),
            "{offset}"
        );
    }
}

#[test]
fn a_timestamp_outside_the_window_is_a_replay() {
    for offset in [-301, 301, -1_800_000_000] {
        let stamp = (1_800_000_000 + offset).to_string();
        let headers = [
            ("X-Signature-Ed25519", "00"),
            ("X-Signature-Timestamp", stamp.as_str()),
        ];
        assert_eq!(
            claimed(DISCORD, &headers),
            Err(Refusal::Stale {
                window: Inbound::DEFAULT.replay_window,
            }),
            "{offset}"
        );
    }
}

#[test]
fn a_timestamp_that_is_not_unix_seconds_is_malformed() {
    for stamp in [
        "",
        "+1800000000",
        "-1",
        "1800000000.5",
        "soon",
        "99999999999999999999",
    ] {
        let headers = [
            ("X-Slack-Signature", "v0=ff"),
            ("X-Slack-Request-Timestamp", stamp),
        ];
        assert_eq!(
            claimed(SLACK, &headers),
            Err(Refusal::Malformed {
                header: "X-Slack-Request-Timestamp"
            }),
            "{stamp}"
        );
    }
    assert_eq!(
        claimed(SLACK, &[("X-Slack-Signature", "v0=ff")]),
        Err(Refusal::Unsigned {
            header: "X-Slack-Request-Timestamp"
        })
    );
}

#[test]
fn a_timestamp_beyond_the_calendar_is_malformed() {
    let headers = [
        ("X-Signature-Ed25519", "00"),
        ("X-Signature-Timestamp", "9999999999999"),
    ];
    assert_eq!(
        claimed(DISCORD, &headers),
        Err(Refusal::Malformed {
            header: "X-Signature-Timestamp"
        })
    );
}

#[test]
fn each_refusal_reads_as_a_sentence() {
    assert_eq!(
        Refusal::Unsigned {
            header: "X-Hub-Signature-256"
        }
        .to_string(),
        "the request carries no X-Hub-Signature-256"
    );
    assert_eq!(
        Refusal::Malformed {
            header: "X-Hub-Signature-256"
        }
        .to_string(),
        "the request's X-Hub-Signature-256 is not in its form"
    );
    assert_eq!(
        Refusal::Stale {
            window: Inbound::DEFAULT.replay_window,
        }
        .to_string(),
        "the request was signed more than 5 minutes from now, so it is taken for a replay"
    );
}

fn gitlab(signature: &str) -> Result<Claim, Refusal> {
    claimed(
        GITLAB,
        &[
            ("webhook-id", "msg_2KWPBgLlAfxdpx2AI54pPJ85f4W"),
            ("webhook-timestamp", "1800000000"),
            ("webhook-signature", signature),
        ],
    )
}

#[test]
fn a_gitlab_webhook_claims_each_v1_signature_of_its_id_timestamp_and_body() {
    let claim = gitlab("v1,AAE= v2,ignored v1,/w==").unwrap();
    assert_eq!(claim.kind, Signature::HmacSha256);
    assert_eq!(
        claim.message,
        [
            b"msg_2KWPBgLlAfxdpx2AI54pPJ85f4W.1800000000.".as_slice(),
            BODY
        ]
        .concat()
    );
    assert_eq!(claim.signatures, [vec![0x00, 0x01], vec![0xff]]);
    assert_eq!(
        claim.delivery.as_deref(),
        Some("msg_2KWPBgLlAfxdpx2AI54pPJ85f4W")
    );
}

#[test]
fn a_gitlab_webhook_with_no_v1_signature_or_one_not_in_base64_is_malformed() {
    for signature in ["", "v2,AAE=", "v1,", "v1,!!!", "v1,AAE= v1,%%"] {
        assert_eq!(
            gitlab(signature),
            Err(Refusal::Malformed {
                header: "webhook-signature"
            }),
            "{signature}"
        );
    }
}

#[test]
fn a_gitlab_webhook_names_what_it_lacks() {
    assert_eq!(
        claimed(GITLAB, &[]),
        Err(Refusal::Unsigned {
            header: "webhook-id"
        })
    );
    assert_eq!(
        claimed(
            GITLAB,
            &[("webhook-id", "msg"), ("webhook-timestamp", "1800000000")]
        ),
        Err(Refusal::Unsigned {
            header: "webhook-signature"
        })
    );
    assert_eq!(
        claimed(
            GITLAB,
            &[
                ("webhook-id", "msg"),
                ("webhook-timestamp", "1"),
                ("webhook-signature", "v1,AA==")
            ],
        ),
        Err(Refusal::Stale {
            window: Inbound::DEFAULT.replay_window,
        })
    );
}

#[test]
fn each_scheme_reads_its_key_as_it_is_written() {
    assert_eq!(key(GITLAB, "whsec_AAE="), Some(vec![0x00, 0x01]));
    for written in ["AAE=", "whsec_", "whsec_!!"] {
        assert_eq!(key(GITLAB, written), None, "{written}");
    }
    assert_eq!(key(DISCORD, "0aFF"), Some(vec![0x0a, 0xff]));
    assert_eq!(key(DISCORD, "public"), None);
    for scheme in [GITHUB, FORGEJO, SLACK] {
        assert_eq!(
            key(scheme, "secret"),
            Some(b"secret".to_vec()),
            "{scheme:?}"
        );
        assert_eq!(key(scheme, ""), None, "{scheme:?}");
    }
}

#[test]
fn a_gitlab_webhook_carrying_more_signatures_than_a_rotation_needs_is_refused_unchecked() {
    let entries = |count: usize| vec!["v1,AAE="; count].join(" ");
    assert_eq!(
        gitlab(&entries(Inbound::DEFAULT.signatures)).map(|claim| claim.signatures.len()),
        Ok(Inbound::DEFAULT.signatures)
    );
    for count in [Inbound::DEFAULT.signatures + 1, 10_000] {
        assert_eq!(
            gitlab(&entries(count)),
            Err(Refusal::Malformed {
                header: "webhook-signature"
            }),
            "{count}"
        );
    }
}

#[test]
fn a_matrix_transaction_claims_the_homeserver_token_it_presents() {
    let claim = claimed(MATRIX, &[("authorization", "Bearer hs-token")]).unwrap();
    assert_eq!(claim.kind, Signature::Token);
    assert_eq!(claim.message, Vec::<u8>::new());
    assert_eq!(claim.signatures, [b"hs-token".to_vec()]);
    assert_eq!(claim.delivery, None);
    for value in ["hs-token", "Bearer ", "Basic aHM6dG9rZW4="] {
        assert_eq!(
            claimed(MATRIX, &[("Authorization", value)]),
            Err(Refusal::Malformed {
                header: "Authorization"
            }),
            "{value}"
        );
    }
    assert_eq!(
        claimed(MATRIX, &[]),
        Err(Refusal::Unsigned {
            header: "Authorization"
        })
    );
    assert_eq!(key(MATRIX, "hs-token"), Some(b"hs-token".to_vec()));
}

#[test]
fn a_token_is_the_same_only_byte_for_byte() {
    assert!(same(b"hs-token", b"hs-token"));
    for presented in [
        b"hs-tokeN".as_slice(),
        b"sh-token",
        b"Hs-token",
        b"hs-token-",
        b"hs-toke",
        b"",
    ] {
        assert!(!same(b"hs-token", presented), "{presented:?}");
    }
}

#[test]
fn an_unsigned_delivery_header_changes_nothing_a_store_admits() {
    let signature = [("X-Hub-Signature-256", "sha256=0a0b")];
    let renamed = [
        ("X-Hub-Signature-256", "sha256=0a0b"),
        ("X-GitHub-Delivery", "a-fresh-id"),
    ];
    assert_eq!(
        claimed(GITHUB, &signature).map(|claim| claim.delivery),
        claimed(GITHUB, &renamed).map(|claim| claim.delivery)
    );
}

#[test]
fn each_scheme_keeps_its_deliveries_under_its_own_name() {
    let schemes = [
        GITHUB,
        FORGEJO,
        GITLAB,
        DISCORD,
        SLACK,
        MATRIX,
        ACTIVITYPUB,
        APP,
    ];
    assert_eq!(
        schemes.map(|scheme| scheme.name),
        [
            "github",
            "forgejo",
            "gitlab",
            "discord",
            "slack",
            "matrix",
            "activitypub",
            "app"
        ]
    );
}

#[test]
fn an_app_s_event_is_claimed_by_its_token_and_not_by_a_signature() {
    assert_eq!(
        claim(
            APP,
            &[("authorization", "Bearer t")],
            b"{}",
            now(),
            &Inbound::DEFAULT
        ),
        Err(Refusal::Unaddressed)
    );
    assert_eq!(key(APP, "t"), Some(b"t".to_vec()));
    assert_eq!(key(APP, ""), None);
}

fn probe(claim: Result<Claim, Refusal>) -> String {
    match claim {
        Ok(claim) => format!(
            "{:?} {}",
            claim.kind,
            String::from_utf8(claim.message).unwrap()
        ),
        Err(refusal) => format!("{refusal:?}"),
    }
}

mod activitypub;
