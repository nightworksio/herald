use super::*;

const BODY: &[u8] = b"{\"action\":\"published\"}";

fn now() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

fn claimed(scheme: Scheme, headers: &[(&str, &str)]) -> Result<Claim, Refusal> {
    claim(scheme, headers, BODY, now())
}

#[test]
fn a_github_webhook_claims_an_hmac_of_its_body() {
    let claim = claimed(Scheme::GitHub, &[("x-hub-signature-256", "sha256=00Ff10")]).unwrap();
    assert_eq!(
        claim,
        Claim {
            kind: Signature::HmacSha256,
            message: BODY.to_vec(),
            signature: vec![0x00, 0xff, 0x10],
        }
    );
}

#[test]
fn a_github_webhook_without_its_signature_or_its_prefix_is_refused() {
    assert_eq!(
        claimed(Scheme::GitHub, &[("X-Hub-Signature", "sha1=00")]),
        Err(Refusal::Unsigned {
            header: "X-Hub-Signature-256"
        })
    );
    for value in ["00ff", "sha256=", "sha256=0", "sha256=zz", "sha256=+1"] {
        assert_eq!(
            claimed(Scheme::GitHub, &[("X-Hub-Signature-256", value)]),
            Err(Refusal::Malformed {
                header: "X-Hub-Signature-256"
            }),
            "{value}"
        );
    }
}

#[test]
fn a_forgejo_webhook_claims_an_hmac_of_its_body_under_either_header() {
    for header in ["X-Forgejo-Signature", "x-gitea-signature"] {
        let claim = claimed(Scheme::Forgejo, &[(header, "abcd")]).unwrap();
        assert_eq!(claim.kind, Signature::HmacSha256, "{header}");
        assert_eq!(claim.message, BODY, "{header}");
        assert_eq!(claim.signature, [0xab, 0xcd], "{header}");
    }
    assert_eq!(
        claimed(Scheme::Forgejo, &[("X-Gitea-Signature", "abc")]),
        Err(Refusal::Malformed {
            header: "X-Gitea-Signature"
        })
    );
    assert_eq!(
        claimed(Scheme::Forgejo, &[]),
        Err(Refusal::Unsigned {
            header: "X-Forgejo-Signature"
        })
    );
}

#[test]
fn a_forgejo_header_is_read_before_a_gitea_one() {
    let claim = claimed(
        Scheme::Forgejo,
        &[("X-Gitea-Signature", "00"), ("X-Forgejo-Signature", "01")],
    )
    .unwrap();
    assert_eq!(claim.signature, [0x01]);
}

#[test]
fn a_discord_interaction_claims_an_ed25519_signature_of_its_timestamp_and_body() {
    let claim = claimed(
        Scheme::Discord,
        &[
            ("X-Signature-Ed25519", "0a0b"),
            ("X-Signature-Timestamp", "1800000000"),
        ],
    )
    .unwrap();
    assert_eq!(claim.kind, Signature::Ed25519);
    assert_eq!(claim.message, [b"1800000000".as_slice(), BODY].concat());
    assert_eq!(claim.signature, [0x0a, 0x0b]);
    assert_eq!(
        claimed(Scheme::Discord, &[("X-Signature-Timestamp", "1800000000")]),
        Err(Refusal::Unsigned {
            header: "X-Signature-Ed25519"
        })
    );
    assert_eq!(
        claimed(
            Scheme::Discord,
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
        Scheme::Slack,
        &[
            ("X-Slack-Signature", "v0=ff"),
            ("X-Slack-Request-Timestamp", "1800000000"),
        ],
    )
    .unwrap();
    assert_eq!(claim.kind, Signature::HmacSha256);
    assert_eq!(claim.message, [b"v0:1800000000:".as_slice(), BODY].concat());
    assert_eq!(claim.signature, [0xff]);
    assert_eq!(
        claimed(
            Scheme::Slack,
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
        assert!(claimed(Scheme::Slack, &headers).is_ok(), "{offset}");
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
            claimed(Scheme::Discord, &headers),
            Err(Refusal::Stale),
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
            claimed(Scheme::Slack, &headers),
            Err(Refusal::Malformed {
                header: "X-Slack-Request-Timestamp"
            }),
            "{stamp}"
        );
    }
    assert_eq!(
        claimed(Scheme::Slack, &[("X-Slack-Signature", "v0=ff")]),
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
        claimed(Scheme::Discord, &headers),
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
        Refusal::Stale.to_string(),
        "the request was signed more than 5 minutes from now, so it is taken for a replay"
    );
}
