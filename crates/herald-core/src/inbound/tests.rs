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
    let claim = claimed(
        Scheme::GitHub,
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
    for (header, delivered) in [
        ("X-Forgejo-Signature", "X-Forgejo-Delivery"),
        ("x-gitea-signature", "x-gitea-delivery"),
    ] {
        let claim = claimed(Scheme::Forgejo, &[(header, "abcd"), (delivered, "7")]).unwrap();
        assert_eq!(claim.kind, Signature::HmacSha256, "{header}");
        assert_eq!(claim.message, BODY, "{header}");
        assert_eq!(claim.signatures, [[0xab, 0xcd]], "{header}");
        assert_eq!(claim.delivery.as_deref(), Some("abcd"), "{header}");
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
        Scheme::Discord,
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
    assert_eq!(claim.signatures, [[0xff]]);
    assert_eq!(claim.delivery.as_deref(), Some("ff"));
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

fn gitlab(signature: &str) -> Result<Claim, Refusal> {
    claimed(
        Scheme::GitLab,
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
        claimed(Scheme::GitLab, &[]),
        Err(Refusal::Unsigned {
            header: "webhook-id"
        })
    );
    assert_eq!(
        claimed(
            Scheme::GitLab,
            &[("webhook-id", "msg"), ("webhook-timestamp", "1800000000")]
        ),
        Err(Refusal::Unsigned {
            header: "webhook-signature"
        })
    );
    assert_eq!(
        claimed(
            Scheme::GitLab,
            &[
                ("webhook-id", "msg"),
                ("webhook-timestamp", "1"),
                ("webhook-signature", "v1,AA==")
            ],
        ),
        Err(Refusal::Stale)
    );
}

#[test]
fn each_scheme_reads_its_key_as_it_is_written() {
    assert_eq!(key(Scheme::GitLab, "whsec_AAE="), Some(vec![0x00, 0x01]));
    for written in ["AAE=", "whsec_", "whsec_!!"] {
        assert_eq!(key(Scheme::GitLab, written), None, "{written}");
    }
    assert_eq!(key(Scheme::Discord, "0aFF"), Some(vec![0x0a, 0xff]));
    assert_eq!(key(Scheme::Discord, "public"), None);
    for scheme in [Scheme::GitHub, Scheme::Forgejo, Scheme::Slack] {
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
        gitlab(&entries(MOST_SIGNATURES)).map(|claim| claim.signatures.len()),
        Ok(MOST_SIGNATURES)
    );
    for count in [MOST_SIGNATURES + 1, 10_000] {
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
    let claim = claimed(Scheme::Matrix, &[("authorization", "Bearer hs-token")]).unwrap();
    assert_eq!(claim.kind, Signature::Token);
    assert_eq!(claim.message, Vec::<u8>::new());
    assert_eq!(claim.signatures, [b"hs-token".to_vec()]);
    assert_eq!(claim.delivery, None);
    for value in ["hs-token", "Bearer ", "Basic aHM6dG9rZW4="] {
        assert_eq!(
            claimed(Scheme::Matrix, &[("Authorization", value)]),
            Err(Refusal::Malformed {
                header: "Authorization"
            }),
            "{value}"
        );
    }
    assert_eq!(
        claimed(Scheme::Matrix, &[]),
        Err(Refusal::Unsigned {
            header: "Authorization"
        })
    );
    assert_eq!(key(Scheme::Matrix, "hs-token"), Some(b"hs-token".to_vec()));
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
        claimed(Scheme::GitHub, &signature).map(|claim| claim.delivery),
        claimed(Scheme::GitHub, &renamed).map(|claim| claim.delivery)
    );
}

#[test]
fn each_scheme_keeps_its_deliveries_under_its_own_name() {
    let schemes = [
        Scheme::GitHub,
        Scheme::Forgejo,
        Scheme::GitLab,
        Scheme::Discord,
        Scheme::Slack,
        Scheme::Matrix,
        Scheme::ActivityPub,
        Scheme::App,
    ];
    assert_eq!(
        schemes.map(Scheme::name),
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
        claim(Scheme::App, &[("authorization", "Bearer t")], b"{}", now()),
        Err(Refusal::Unaddressed)
    );
    assert_eq!(key(Scheme::App, "t"), Some(b"t".to_vec()));
    assert_eq!(key(Scheme::App, ""), None);
}

const PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAtz228sa1z4u70xtqdZER\nBwxBvj+JP0V8SEK7SPdlULv1XbBcoI7T238LnlVGo4IN+C/TsW4tpwORACY7iOb2\npF8LZU0LBOFBV7U4fsd2JwwZa45m8U9wd5Q00xcqQoMZYfwQF3HonOlHwNt8DKpb\nmlKsH1WjhhCcjD5m1Q8LeEuGhVviBtrtAor81FzU106aGDYkMwcrUouxSPzDx8+I\nS7oicynJwyHgqqq8ImqZnqYGufaQuqxcOxNNL1GEMtwW6uylOyPIUb4aJ2wcoXTs\nt1dNhGrVFrSUYlad0nhHNuzF8n83Pj0cELoGmrBepQ8+LqI+Yj6sUIagTEi7P0tz\nowIDAQAB\n-----END PUBLIC KEY-----\n";

const ACTIVITY: &[u8] = b"{\"type\":\"Follow\"}";

fn activity_headers<'a>(
    signature: &'a str,
    date: &'a str,
    digest: &'a str,
) -> Vec<(&'a str, &'a str)> {
    vec![
        ("(request-target)", "post /inbox"),
        ("(audience)", "herald.example"),
        ("Host", "herald.example"),
        ("Date", date),
        ("Digest", digest),
        ("Signature", signature),
    ]
}

const DATE: &str = "Fri, 15 Jan 2027 08:00:00 GMT";

fn activity_now() -> Timestamp {
    "2027-01-15T08:00:00Z".parse().unwrap()
}

fn digest() -> String {
    use sha2::Digest as _;
    format!(
        "SHA-256={}",
        STANDARD.encode(sha2::Sha256::digest(ACTIVITY))
    )
}

const SIGNED: &str = r#"keyId="https://social.example/users/ana#main-key",algorithm="rsa-sha256",headers="(request-target) host date digest",signature="AAE=""#;

/// `body` claimed as sent where the pseudo-headers among `headers` say,
/// with those taken out of the headers the sender sent.
fn addressed(headers: &[(&str, &str)], body: &[u8]) -> Result<Claim, Refusal> {
    let pseudo = |name: &str, otherwise: &'static str| {
        headers
            .iter()
            .find(|(named, _)| *named == name)
            .map_or(otherwise, |(_, value)| *value)
    };
    let sent: Vec<(&str, &str)> = headers
        .iter()
        .filter(|(name, _)| !name.starts_with('('))
        .copied()
        .collect();
    let to = Addressed {
        target: pseudo("(request-target)", "post /inbox"),
        audience: pseudo("(audience)", "herald.example"),
    };
    activity_claim(to, &sent, body, activity_now())
}

fn activitypub(headers: &[(&str, &str)]) -> Result<Claim, Refusal> {
    addressed(headers, ACTIVITY)
}

#[test]
fn an_activity_claims_rsa_over_the_headers_its_signature_names() {
    let digest = digest();
    let claim = activitypub(&activity_headers(SIGNED, DATE, &digest)).unwrap();
    assert_eq!(claim.kind, Signature::RsaSha256);
    assert_eq!(
        String::from_utf8(claim.message).unwrap(),
        format!(
            "(request-target): post /inbox\nhost: herald.example\ndate: {DATE}\ndigest: {digest}"
        )
    );
    assert_eq!(claim.signatures, [[0x00, 0x01]]);
    assert_eq!(
        claim.signer.as_deref(),
        Some("https://social.example/users/ana#main-key")
    );
    assert_eq!(claim.delivery.as_deref(), Some("0001"));
}

#[test]
fn an_activity_must_sign_its_target_host_date_and_digest() {
    let digest = digest();
    for (dropped, header) in [
        ("(request-target) ", "(request-target)"),
        ("host ", "host"),
        ("date ", "date"),
        (" digest", "digest"),
    ] {
        let signature = SIGNED.replace(dropped, "");
        assert_eq!(
            activitypub(&activity_headers(&signature, DATE, &digest)),
            Err(Refusal::Uncovered { header }),
            "{header}"
        );
    }
    let defaulted = SIGNED.replace(r#"headers="(request-target) host date digest","#, "");
    assert_eq!(
        activitypub(&activity_headers(&defaulted, DATE, &digest)),
        Err(Refusal::Uncovered {
            header: "(request-target)"
        })
    );
}

#[test]
fn an_activity_without_a_body_need_not_sign_a_digest() {
    let signature = SIGNED.replace(" digest", "");
    let headers = activity_headers(&signature, DATE, "");
    let claim = addressed(&headers, b"").unwrap();
    assert!(!String::from_utf8(claim.message).unwrap().contains("digest"));
}

#[test]
fn an_activity_whose_digest_is_not_its_bodys_is_refused() {
    let altered = "SHA-256=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    assert_eq!(
        activitypub(&activity_headers(SIGNED, DATE, altered)),
        Err(Refusal::Unmatched { header: "Digest" })
    );
    let both = format!("SHA-512=xyz, {}", digest().replace("SHA-256", "sha-256"));
    assert!(activitypub(&activity_headers(SIGNED, DATE, &both)).is_ok());
    for written in ["", "MD5=abc", "SHA-256"] {
        assert_eq!(
            activitypub(&activity_headers(SIGNED, DATE, written)),
            Err(Refusal::Malformed { header: "Digest" }),
            "{written}"
        );
    }
}

#[test]
fn an_activity_dated_outside_the_window_or_undated_is_refused() {
    let digest = digest();
    assert_eq!(
        activitypub(&activity_headers(
            SIGNED,
            "Fri, 15 Jan 2027 07:54:59 GMT",
            &digest
        )),
        Err(Refusal::Stale)
    );
    assert!(
        activitypub(&activity_headers(
            SIGNED,
            "Fri, 15 Jan 2027 07:55:00 GMT",
            &digest
        ))
        .is_ok()
    );
    assert_eq!(
        activitypub(&activity_headers(
            SIGNED,
            "Fri, 15 Jan 2027 08:05:01 GMT",
            &digest
        )),
        Err(Refusal::Stale)
    );
    assert_eq!(
        activitypub(&activity_headers(SIGNED, "yesterday", &digest)),
        Err(Refusal::Malformed { header: "Date" })
    );
}

#[test]
fn a_signature_header_not_in_its_form_is_malformed() {
    let digest = digest();
    for signature in [
        "",
        "keyId=ana",
        r#"keyId="ana""#,
        r#"keyId="a",keyId="b",headers="(request-target) host date digest",signature="AAE=""#,
        r#"keyId="",headers="(request-target) host date digest",signature="AAE=""#,
        r#"keyId="a",headers="(request-target) host date digest",signature="!!""#,
        r#"keyId="a",headers="(request-target) host date digest",signature="""#,
        r#"keyId="a",algorithm="hmac-sha256",headers="(request-target) host date digest",signature="AAE=""#,
        r#"keyId="a" headers="(request-target) host date digest""#,
        r#"="a",signature="AAE=""#,
        r#"keyId="a",headers="(request-target) host date digest x-missing",signature="AAE=""#,
    ] {
        assert_eq!(
            activitypub(&activity_headers(signature, DATE, &digest)),
            Err(Refusal::Malformed {
                header: "Signature"
            }),
            "{signature}"
        );
    }
    let hs2019 = SIGNED.replace("rsa-sha256", "hs2019");
    assert!(activitypub(&activity_headers(&hs2019, DATE, &digest)).is_ok());
    let unnamed = SIGNED.replace(r#"algorithm="rsa-sha256","#, "");
    assert!(activitypub(&activity_headers(&unnamed, DATE, &digest)).is_ok());
}

/// The test key's SPKI made wrong one way each, as an actor might publish it.
const NOT_KEYS: [(&str, &str); 8] = [
    (
        "another algorithm",
        "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQIFAAOCAQ8AMIIBCgKCAQEAtz228sa1z4u70xtqdZER\nBwxBvj+JP0V8SEK7SPdlULv1XbBcoI7T238LnlVGo4IN+C/TsW4tpwORACY7iOb2\npF8LZU0LBOFBV7U4fsd2JwwZa45m8U9wd5Q00xcqQoMZYfwQF3HonOlHwNt8DKpb\nmlKsH1WjhhCcjD5m1Q8LeEuGhVviBtrtAor81FzU106aGDYkMwcrUouxSPzDx8+I\nS7oicynJwyHgqqq8ImqZnqYGufaQuqxcOxNNL1GEMtwW6uylOyPIUb4aJ2wcoXTs\nt1dNhGrVFrSUYlad0nhHNuzF8n83Pj0cELoGmrBepQ8+LqI+Yj6sUIagTEi7P0tz\nowIDAQAB\n-----END PUBLIC KEY-----\n",
    ),
    (
        "bytes after the key",
        "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAtz228sa1z4u70xtqdZER\nBwxBvj+JP0V8SEK7SPdlULv1XbBcoI7T238LnlVGo4IN+C/TsW4tpwORACY7iOb2\npF8LZU0LBOFBV7U4fsd2JwwZa45m8U9wd5Q00xcqQoMZYfwQF3HonOlHwNt8DKpb\nmlKsH1WjhhCcjD5m1Q8LeEuGhVviBtrtAor81FzU106aGDYkMwcrUouxSPzDx8+I\nS7oicynJwyHgqqq8ImqZnqYGufaQuqxcOxNNL1GEMtwW6uylOyPIUb4aJ2wcoXTs\nt1dNhGrVFrSUYlad0nhHNuzF8n83Pj0cELoGmrBepQ8+LqI+Yj6sUIagTEi7P0tz\nowIDAQABAA==\n-----END PUBLIC KEY-----\n",
    ),
    (
        "bytes after its bit string",
        "-----BEGIN PUBLIC KEY-----\nMIIBJDANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAtz228sa1z4u70xtqdZER\nBwxBvj+JP0V8SEK7SPdlULv1XbBcoI7T238LnlVGo4IN+C/TsW4tpwORACY7iOb2\npF8LZU0LBOFBV7U4fsd2JwwZa45m8U9wd5Q00xcqQoMZYfwQF3HonOlHwNt8DKpb\nmlKsH1WjhhCcjD5m1Q8LeEuGhVviBtrtAor81FzU106aGDYkMwcrUouxSPzDx8+I\nS7oicynJwyHgqqq8ImqZnqYGufaQuqxcOxNNL1GEMtwW6uylOyPIUb4aJ2wcoXTs\nt1dNhGrVFrSUYlad0nhHNuzF8n83Pj0cELoGmrBepQ8+LqI+Yj6sUIagTEi7P0tz\nowIDAQABBQA=\n-----END PUBLIC KEY-----\n",
    ),
    (
        "a bit string that is no RSA key",
        "-----BEGIN PUBLIC KEY-----\nMBcwDQYJKoZIhvcNAQEBBQADBgAEAwECAw==\n-----END PUBLIC KEY-----\n",
    ),
    (
        "bytes after the RSA key",
        "-----BEGIN PUBLIC KEY-----\nMIIBIzANBgkqhkiG9w0BAQEFAAOCARAAMIIBCgKCAQEAtz228sa1z4u70xtqdZER\nBwxBvj+JP0V8SEK7SPdlULv1XbBcoI7T238LnlVGo4IN+C/TsW4tpwORACY7iOb2\npF8LZU0LBOFBV7U4fsd2JwwZa45m8U9wd5Q00xcqQoMZYfwQF3HonOlHwNt8DKpb\nmlKsH1WjhhCcjD5m1Q8LeEuGhVviBtrtAor81FzU106aGDYkMwcrUouxSPzDx8+I\nS7oicynJwyHgqqq8ImqZnqYGufaQuqxcOxNNL1GEMtwW6uylOyPIUb4aJ2wcoXTs\nt1dNhGrVFrSUYlad0nhHNuzF8n83Pj0cELoGmrBepQ8+LqI+Yj6sUIagTEi7P0tz\nowIDAQABAA==\n-----END PUBLIC KEY-----\n",
    ),
    (
        "a length in three bytes",
        "-----BEGIN PUBLIC KEY-----\nMIMAASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBALc9tvLGtc+Lu9MbanWR\nEQcMQb4/iT9FfEhCu0j3ZVC79V2wXKCO09t/C55VRqOCDfgv07FuLacDkQAmO4jm\n9qRfC2VNCwThQVe1OH7HdicMGWuOZvFPcHeUNNMXKkKDGWH8EBdx6JzpR8DbfAyq\nW5pSrB9Vo4YQnIw+ZtUPC3hLhoVb4gba7QKK/NRc1NdOmhg2JDMHK1KLsUj8w8fP\niEu6InMpycMh4KqqvCJqmZ6mBrn2kLqsXDsTTS9RhDLcFurspTsjyFG+GidsHKF0\n7LdXTYRq1Ra0lGJWndJ4RzbsxfJ/Nz49HBC6BpqwXqUPPi6iPmI+rFCGoExIuz9L\nc6MCAwEAAQ==\n-----END PUBLIC KEY-----\n",
    ),
    (
        "a bit string with unused bits",
        "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8BMIIBCgKCAQEAtz228sa1z4u70xtqdZER\nBwxBvj+JP0V8SEK7SPdlULv1XbBcoI7T238LnlVGo4IN+C/TsW4tpwORACY7iOb2\npF8LZU0LBOFBV7U4fsd2JwwZa45m8U9wd5Q00xcqQoMZYfwQF3HonOlHwNt8DKpb\nmlKsH1WjhhCcjD5m1Q8LeEuGhVviBtrtAor81FzU106aGDYkMwcrUouxSPzDx8+I\nS7oicynJwyHgqqq8ImqZnqYGufaQuqxcOxNNL1GEMtwW6uylOyPIUb4aJ2wcoXTs\nt1dNhGrVFrSUYlad0nhHNuzF8n83Pj0cELoGmrBepQ8+LqI+Yj6sUIagTEi7P0tz\nowIDAQAB\n-----END PUBLIC KEY-----\n",
    ),
    (
        "a key cut short",
        "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAtz228sa1z4u70xtqdZER\nBwxBvj+JP0V8SEK7SPdlULv1XbBcoI7T238LnlVGo4IN+C/TsW4tpwORACY7iOb2\npF8LZU0LBOFBV7U4fsd2JwwZa45m8U9wd5Q00xcqQoMZYfwQF3HonOlHwNt8DKpb\nmlKsH1WjhhCcjD5m1Q8LeEuGhVviBtrtAor81FzU106aGDYkMwcrUouxSPzDx8+I\nS7oicynJwyHgqqq8ImqZnqYGufaQuqxcOxNNL1GEMtwW6uylOyPIUb4aJ2wcoXTs\nt1dNhGrVFrSUYlad0nhHNuzF8n83Pj0cELoGmrBepQ8+LqI+Yj6sUIagTEi7P0tz\now==\n-----END PUBLIC KEY-----\n",
    ),
];

#[test]
fn an_actors_pem_public_key_reads_as_its_pkcs1_der() {
    let der = key(Scheme::ActivityPub, PUBLIC_KEY).unwrap();
    assert_eq!(der.len(), 270);
    assert_eq!(der[..4], [0x30, 0x82, 0x01, 0x0a]);
    for written in [
        "",
        "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA",
        &PUBLIC_KEY.replace("PUBLIC KEY", "PRIVATE KEY"),
        &PUBLIC_KEY.replace("MIIBIjAN", "MIIBIjAM"),
        "-----BEGIN PUBLIC KEY-----\n!!\n-----END PUBLIC KEY-----",
    ] {
        assert_eq!(key(Scheme::ActivityPub, written), None, "{written}");
    }
    for (wrong, written) in NOT_KEYS {
        assert_eq!(key(Scheme::ActivityPub, written), None, "{wrong}");
    }
}

#[test]
fn a_der_length_reads_in_one_two_or_three_bytes() {
    let body = [7u8; 0x180];
    for (header, length) in [
        (vec![0x30, 0x05], 5),
        (vec![0x30, 0x81, 0x80], 0x80),
        (vec![0x30, 0x81, 0xff], 0xff),
        (vec![0x30, 0x82, 0x01, 0x80], 0x180),
    ] {
        let mut der = header;
        der.extend_from_slice(&body[..length]);
        der.push(0xaa);
        assert_eq!(
            element(&der, 0x30),
            Some((&body[..length], &[0xaa][..])),
            "{length}"
        );
    }
    assert_eq!(element(&[0x30, 0x81], 0x30), None);
    assert_eq!(element(&[0x30, 0x83, 0, 0, 1, 0], 0x30), None);
    assert_eq!(element(&[0x31, 0x00], 0x30), None);
    assert_eq!(element(&[0x30, 0x81, 0x80, 1], 0x30), None);
}

#[test]
fn an_activity_without_a_body_need_not_cover_a_digest() {
    let unsigned = SIGNED.replace(
        "(request-target) host date digest",
        "(request-target) host date",
    );
    let headers = activity_headers(&unsigned, DATE, "");
    assert!(addressed(&headers, b"").is_ok());
    assert_eq!(
        addressed(&headers, ACTIVITY),
        Err(Refusal::Uncovered { header: "digest" })
    );
}

#[test]
fn signature_pairs_are_apart_by_commas_and_nothing_else() {
    for written in [
        r#"keyId="a" algorithm="rsa-sha256""#,
        r#"keyId="a"algorithm="rsa-sha256""#,
        r#"keyId="a",algorithm="rsa-sha256" x"#,
    ] {
        assert_eq!(parameters(written), None, "{written}");
    }
    assert_eq!(
        parameters(r#"keyId="a" , algorithm="b""#),
        Some(vec![("keyId", "a"), ("algorithm", "b")])
    );
}

#[test]
fn blank_names_do_not_count_toward_the_most_an_activity_may_name() {
    let digest = digest();
    let padded = SIGNED.replace(
        "(request-target) host date digest",
        &format!(
            "(request-target) host date{}digest",
            " ".repeat(MOST_COVERED + 2)
        ),
    );
    let headers = activity_headers(&padded, DATE, &digest);
    assert!(activitypub(&headers).is_ok());
}

#[test]
fn each_new_refusal_reads_as_a_sentence() {
    assert_eq!(
        Refusal::Uncovered { header: "digest" }.to_string(),
        "the request's signature does not cover its digest"
    );
    assert_eq!(
        Refusal::Unmatched { header: "Digest" }.to_string(),
        "the request's Digest does not match its body"
    );
}

#[test]
fn an_activity_signed_for_another_host_is_misdirected() {
    let digest = digest();
    let mut headers = activity_headers(SIGNED, DATE, &digest);
    headers.retain(|(name, _)| *name != "Host");
    headers.push(("Host", "HERALD.example "));
    assert!(activitypub(&headers).is_ok());
    headers.retain(|(name, _)| *name != "Host");
    headers.push(("Host", "elsewhere.example"));
    assert_eq!(activitypub(&headers), Err(Refusal::Misdirected));
    assert_eq!(
        Refusal::Misdirected.to_string(),
        "the request was signed for another host"
    );
    assert_eq!(
        claim(Scheme::ActivityPub, &headers, ACTIVITY, activity_now()),
        Err(Refusal::Unaddressed)
    );
    assert_eq!(
        Refusal::Unaddressed.to_string(),
        "an ActivityPub activity is claimed with where it was sent, which no header says"
    );
}

#[test]
fn a_pseudo_header_the_sender_sends_is_never_read() {
    let digest = digest();
    let mut headers: Vec<(&str, &str)> = activity_headers(SIGNED, DATE, &digest)
        .into_iter()
        .filter(|(name, _)| !name.starts_with('('))
        .collect();
    headers.push(("(request-target)", "get /elsewhere"));
    headers.push(("(audience)", "elsewhere.example"));
    let to = Addressed {
        target: "post /inbox",
        audience: "herald.example",
    };
    let claim = activity_claim(to, &headers, ACTIVITY, activity_now()).unwrap();
    assert!(
        String::from_utf8(claim.message)
            .unwrap()
            .starts_with("(request-target): post /inbox\n")
    );
    let created = SIGNED.replace(
        "(request-target) host date digest",
        "(request-target) (created) host date digest",
    );
    let mut stamped = activity_headers(&created, DATE, &digest);
    stamped.push(("(created)", "1"));
    let to = Addressed {
        target: "post /inbox",
        audience: "herald.example",
    };
    assert_eq!(
        activity_claim(to, &stamped, ACTIVITY, activity_now()),
        Err(Refusal::Malformed {
            header: "Signature"
        })
    );
}

#[test]
fn an_activity_naming_more_headers_than_it_may_is_refused_unread() {
    let digest = digest();
    let extra = ["x-extra"; MOST_COVERED - 4].join(" ");
    let at_most = SIGNED.replace(
        "(request-target) host date digest",
        &format!("(request-target) host date digest {extra}"),
    );
    let mut headers = activity_headers(&at_most, DATE, &digest);
    headers.push(("X-Extra", "1"));
    assert!(activitypub(&headers).is_ok());
    let over = SIGNED.replace(
        "(request-target) host date digest",
        &format!("(request-target) host date digest {extra} x-extra"),
    );
    let headers = activity_headers(&over, DATE, &digest);
    assert_eq!(
        activitypub(&headers),
        Err(Refusal::Malformed {
            header: "Signature"
        })
    );
}

#[test]
fn a_bearer_token_is_read_whatever_case_its_scheme_is_written_in() {
    assert_eq!(bearer("Bearer abc"), Some("abc"));
    assert_eq!(bearer("bearer abc"), Some("abc"));
    assert_eq!(bearer("BEARER  abc "), Some("abc"));
    assert_eq!(bearer("Basic abc"), None);
    assert_eq!(bearer("Bearer "), None);
    assert_eq!(bearer("Bearer"), None);
}
