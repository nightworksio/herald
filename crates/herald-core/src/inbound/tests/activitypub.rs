use super::*;

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
    activity_claim(to, &sent, body, activity_now(), &Inbound::DEFAULT)
}

fn activitypub(headers: &[(&str, &str)]) -> Result<Claim, Refusal> {
    addressed(headers, ACTIVITY)
}

#[test]
fn an_activity_claims_rsa_over_the_headers_its_signature_names() {
    let digest = digest();
    let claim = activitypub(&activity_headers(SIGNED, DATE, &digest)).unwrap();
    assert_eq!(claim.kind, Signature::Signed(Algorithm::RsaSha256));
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
    assert_eq!(
        probe(activitypub(&activity_headers(SIGNED, DATE, &both))),
        "Signed(RsaSha256) (request-target): post /inbox\nhost: herald.example\ndate: Fri, 15 Jan 2027 08:00:00 GMT\ndigest: SHA-512=xyz, sha-256=GYwYnH3BiO6aICFt0ThC5bUIJ4byvqdpWtR8m5fNkww="
    );
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
        Err(Refusal::Stale {
            window: Inbound::DEFAULT.replay_window,
        })
    );
    assert_eq!(
        probe(activitypub(&activity_headers(
            SIGNED,
            "Fri, 15 Jan 2027 07:55:00 GMT",
            &digest
        ))),
        "Signed(RsaSha256) (request-target): post /inbox\nhost: herald.example\ndate: Fri, 15 Jan 2027 07:55:00 GMT\ndigest: SHA-256=GYwYnH3BiO6aICFt0ThC5bUIJ4byvqdpWtR8m5fNkww="
    );
    assert_eq!(
        activitypub(&activity_headers(
            SIGNED,
            "Fri, 15 Jan 2027 08:05:01 GMT",
            &digest
        )),
        Err(Refusal::Stale {
            window: Inbound::DEFAULT.replay_window,
        })
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
    assert_eq!(
        probe(activitypub(&activity_headers(&hs2019, DATE, &digest))),
        "Signed(RsaSha256) (request-target): post /inbox\nhost: herald.example\ndate: Fri, 15 Jan 2027 08:00:00 GMT\ndigest: SHA-256=GYwYnH3BiO6aICFt0ThC5bUIJ4byvqdpWtR8m5fNkww="
    );
    let unnamed = SIGNED.replace(r#"algorithm="rsa-sha256","#, "");
    assert_eq!(
        probe(activitypub(&activity_headers(&unnamed, DATE, &digest))),
        "Signed(RsaSha256) (request-target): post /inbox\nhost: herald.example\ndate: Fri, 15 Jan 2027 08:00:00 GMT\ndigest: SHA-256=GYwYnH3BiO6aICFt0ThC5bUIJ4byvqdpWtR8m5fNkww="
    );
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
    let der = key(ACTIVITYPUB, PUBLIC_KEY).unwrap();
    assert_eq!(der.len(), 270);
    assert_eq!(der[..4], [0x30, 0x82, 0x01, 0x0a]);
    for written in [
        "",
        "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA",
        &PUBLIC_KEY.replace("PUBLIC KEY", "PRIVATE KEY"),
        &PUBLIC_KEY.replace("MIIBIjAN", "MIIBIjAM"),
        "-----BEGIN PUBLIC KEY-----\n!!\n-----END PUBLIC KEY-----",
    ] {
        assert_eq!(key(ACTIVITYPUB, written), None, "{written}");
    }
    for (wrong, written) in NOT_KEYS {
        assert_eq!(key(ACTIVITYPUB, written), None, "{wrong}");
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
    assert_eq!(
        probe(addressed(&headers, b"")),
        "Signed(RsaSha256) (request-target): post /inbox\nhost: herald.example\ndate: Fri, 15 Jan 2027 08:00:00 GMT"
    );
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
            " ".repeat(Inbound::DEFAULT.covered + 2)
        ),
    );
    let headers = activity_headers(&padded, DATE, &digest);
    assert_eq!(
        probe(activitypub(&headers)),
        "Signed(RsaSha256) (request-target): post /inbox\nhost: herald.example\ndate: Fri, 15 Jan 2027 08:00:00 GMT\ndigest: SHA-256=GYwYnH3BiO6aICFt0ThC5bUIJ4byvqdpWtR8m5fNkww="
    );
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
    assert_eq!(
        probe(activitypub(&headers)),
        "Signed(RsaSha256) (request-target): post /inbox\nhost: HERALD.example\ndate: Fri, 15 Jan 2027 08:00:00 GMT\ndigest: SHA-256=GYwYnH3BiO6aICFt0ThC5bUIJ4byvqdpWtR8m5fNkww="
    );
    headers.retain(|(name, _)| *name != "Host");
    headers.push(("Host", "elsewhere.example"));
    assert_eq!(activitypub(&headers), Err(Refusal::Misdirected));
    assert_eq!(
        Refusal::Misdirected.to_string(),
        "the request was signed for another host"
    );
    assert_eq!(
        claim(
            ACTIVITYPUB,
            &headers,
            ACTIVITY,
            activity_now(),
            &Inbound::DEFAULT
        ),
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
    let claim = activity_claim(to, &headers, ACTIVITY, activity_now(), &Inbound::DEFAULT).unwrap();
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
        activity_claim(to, &stamped, ACTIVITY, activity_now(), &Inbound::DEFAULT),
        Err(Refusal::Malformed {
            header: "Signature"
        })
    );
}

#[test]
fn an_activity_naming_more_headers_than_it_may_is_refused_unread() {
    let digest = digest();
    let extra = ["x-extra"; Inbound::DEFAULT.covered - 4].join(" ");
    let at_most = SIGNED.replace(
        "(request-target) host date digest",
        &format!("(request-target) host date digest {extra}"),
    );
    let mut headers = activity_headers(&at_most, DATE, &digest);
    headers.push(("X-Extra", "1"));
    assert_eq!(
        probe(activitypub(&headers)),
        "Signed(RsaSha256) (request-target): post /inbox\nhost: herald.example\ndate: Fri, 15 Jan 2027 08:00:00 GMT\ndigest: SHA-256=GYwYnH3BiO6aICFt0ThC5bUIJ4byvqdpWtR8m5fNkww=\nx-extra: 1\nx-extra: 1\nx-extra: 1\nx-extra: 1\nx-extra: 1\nx-extra: 1\nx-extra: 1\nx-extra: 1\nx-extra: 1\nx-extra: 1\nx-extra: 1\nx-extra: 1"
    );
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

#[test]
fn a_claim_debugs_without_its_message_or_signatures() {
    let claim = Claim {
        kind: Signature::Signed(Algorithm::HmacSha256),
        message: b"hs-token".to_vec(),
        signatures: vec![b"hs-token".to_vec()],
        delivery: Some("txn-1".to_owned()),
        signer: None,
    };
    let shown = format!("{claim:?}");
    assert!(!shown.contains("hs-token"), "{shown}");
    assert_eq!(
        shown,
        r#"Claim { kind: Signed(HmacSha256), message: 8 bytes, signatures: 1 held back, delivery: Some("txn-1"), signer: None }"#
    );
}

#[test]
fn a_request_is_held_to_the_bounds_it_is_claimed_within() {
    let narrow = Inbound {
        replay_window: SignedDuration::from_mins(1),
        signatures: 1,
        covered: 4,
        ..Inbound::DEFAULT
    };
    let stamp = (1_800_000_000 + 61).to_string();
    let discord = [
        ("X-Signature-Ed25519", "00"),
        ("X-Signature-Timestamp", stamp.as_str()),
    ];
    assert_eq!(
        probe(claimed(DISCORD, &discord)),
        "Signed(Ed25519) 1800000061{\"action\":\"published\"}"
    );
    assert_eq!(
        claim(DISCORD, &discord, BODY, now(), &narrow),
        Err(Refusal::Stale {
            window: SignedDuration::from_mins(1),
        })
    );
    let rotating = [
        ("webhook-id", "msg_2KWPBgLlAfxdpx2AI54pPJ85f4W"),
        ("webhook-timestamp", "1800000000"),
        ("webhook-signature", "v1,AAE= v1,/w=="),
    ];
    assert_eq!(
        probe(claimed(GITLAB, &rotating)),
        "Signed(HmacSha256) msg_2KWPBgLlAfxdpx2AI54pPJ85f4W.1800000000.{\"action\":\"published\"}"
    );
    assert_eq!(
        claim(GITLAB, &rotating, BODY, now(), &narrow),
        Err(Refusal::Malformed {
            header: "webhook-signature"
        })
    );
    let digest = digest();
    let headers = activity_headers(SIGNED, DATE, &digest);
    let to = Addressed {
        target: "post /inbox",
        audience: "herald.example",
    };
    assert_eq!(
        probe(activity_claim(
            to,
            &headers,
            ACTIVITY,
            activity_now(),
            &Inbound::DEFAULT
        )),
        "Signed(RsaSha256) (request-target): post /inbox\nhost: herald.example\ndate: Fri, 15 Jan 2027 08:00:00 GMT\ndigest: SHA-256=GYwYnH3BiO6aICFt0ThC5bUIJ4byvqdpWtR8m5fNkww="
    );
    assert_eq!(
        activity_claim(
            to,
            &headers,
            ACTIVITY,
            activity_now(),
            &Inbound {
                covered: 3,
                ..narrow
            }
        ),
        Err(Refusal::Malformed {
            header: "Signature"
        })
    );
    assert_eq!(
        Refusal::Stale {
            window: SignedDuration::from_mins(20),
        }
        .to_string(),
        "the request was signed more than 20 minutes from now, so it is taken for a replay; check that the sender's clock and herald's are set right"
    );
    assert_eq!(
        Inbound::MOST,
        Inbound {
            replay_window: SignedDuration::from_mins(20),
            signatures: 16,
            covered: 64,
            document: 1_048_576,
        }
    );
}
