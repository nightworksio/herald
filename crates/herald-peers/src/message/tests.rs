use super::*;

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn request(fields: &[(String, String)]) -> Message<'_> {
    Message {
        method: "POST",
        url: "https://Example.com:443/foo?param=Value&Pet=dog",
        status: None,
        fields,
    }
}

fn covered(components: Vec<Component>) -> Covered {
    Covered {
        components,
        created: 1_618_884_473,
        expires: 1_618_884_773,
        nonce: None,
        keyid: "z6Mkkey".to_owned(),
    }
}

const PARAMS_LINE: &str = r#""@signature-params": ("date" "@method" "@path" "@authority" "content-type" "content-length");created=1618884473;expires=1618884773;keyid="z6Mkkey";alg="ed25519";tag="herald""#;

#[test]
fn the_base_lists_each_part_then_the_params_as_the_rfc_s_example_does() {
    let held = fields(&[
        ("Date", "Tue, 20 Apr 2021 02:07:55 GMT"),
        ("Content-Type", "application/json"),
        ("Content-Length", "18"),
    ]);
    let signing = covered(vec![
        Component::Field("date".to_owned()),
        Component::Method,
        Component::Path,
        Component::Authority,
        Component::Field("content-type".to_owned()),
        Component::Field("content-length".to_owned()),
    ]);
    let base = signing.base(&request(&held)).unwrap_or_default();
    assert_eq!(
        String::from_utf8(base).unwrap_or_default(),
        format!(
            "\"date\": Tue, 20 Apr 2021 02:07:55 GMT\n\"@method\": POST\n\"@path\": /foo\n\"@authority\": example.com\n\"content-type\": application/json\n\"content-length\": 18\n{PARAMS_LINE}"
        )
    );
}

#[test]
fn derived_parts_read_the_url_as_rfc_9421_derives_them() {
    let held = Vec::new();
    let message = Message {
        method: "GET",
        url: "http://www.example.com:8080/path/to?a=1#frag",
        status: Some(200),
        fields: &held,
    };
    for (component, value) in [
        (
            Component::TargetUri,
            "http://www.example.com:8080/path/to?a=1",
        ),
        (Component::Authority, "www.example.com:8080"),
        (Component::Scheme, "http"),
        (Component::Path, "/path/to"),
        (Component::Query, "?a=1"),
        (Component::Status, "200"),
        (Component::Method, "GET"),
    ] {
        assert_eq!(
            message.value(&component),
            Ok(value.to_owned()),
            "{component:?}"
        );
    }
    let bare = Message {
        url: "https://example.com",
        ..message
    };
    assert_eq!(bare.value(&Component::Query), Ok("?".to_owned()));
    assert_eq!(bare.value(&Component::Path), Ok("/".to_owned()));
}

#[test]
fn a_field_s_values_are_trimmed_and_joined_in_order_whatever_its_case() {
    let held = fields(&[("X-A", " one\t"), ("x-b", "other"), ("x-a", "two ")]);
    assert_eq!(
        request(&held).value(&Component::Field("x-a".to_owned())),
        Ok("one, two".to_owned())
    );
}

#[test]
fn a_missing_part_a_line_break_or_a_hostless_url_makes_no_base() {
    let held = fields(&[("x-a", "one\r\nforged: line")]);
    let message = request(&held);
    for (component, refused, text) in [
        (
            Component::Status,
            Unsigned::Missing {
                component: "@status".to_owned(),
            },
            "the message has no @status, which the signature covers",
        ),
        (
            Component::Field("x-b".to_owned()),
            Unsigned::Missing {
                component: "x-b".to_owned(),
            },
            "the message has no x-b, which the signature covers",
        ),
        (
            Component::Field("x-a".to_owned()),
            Unsigned::Broken {
                component: "x-a".to_owned(),
            },
            "the message's x-a holds a line break, so it cannot be signed",
        ),
    ] {
        assert_eq!(
            covered(vec![component]).base(&message),
            Err(refused.clone())
        );
        assert_eq!(refused.to_string(), text);
    }
    let hostless = Message {
        url: "mailto:a@b",
        ..message
    };
    for component in [
        Component::TargetUri,
        Component::Authority,
        Component::Scheme,
        Component::Path,
        Component::Query,
    ] {
        let refused = covered(vec![component]).base(&hostless);
        assert_eq!(
            refused,
            Err(Unsigned::Address {
                url: "mailto:a@b".to_owned()
            })
        );
        assert_eq!(
            refused.err().map(|why| why.to_string()).unwrap_or_default(),
            "the message's URL mailto:a@b names no host"
        );
    }
}

#[test]
fn a_nonce_is_listed_before_the_key() {
    let mut signing = covered(vec![Component::Method]);
    signing.nonce = Some("n-1".to_owned());
    assert_eq!(
        signing.params(),
        r#"("@method");created=1618884473;expires=1618884773;nonce="n-1";keyid="z6Mkkey";alg="ed25519";tag="herald""#
    );
}

fn signed(signing: &Covered, base: &[(String, String)], signature: &[u8]) -> Vec<(String, String)> {
    let mut all = base.to_vec();
    all.extend(signing.fields(signature));
    all
}

#[test]
fn a_signature_s_fields_read_back_as_what_it_covers_and_the_same_base() {
    let held = fields(&[("Content-Digest", "sha-256=:AAAA:")]);
    let mut signing = covered(vec![
        Component::Method,
        Component::TargetUri,
        Component::Field("content-digest".to_owned()),
    ]);
    signing.nonce = Some("once".to_owned());
    let all = signed(&signing, &held, b"sig");
    assert_eq!(
        all.get(1..),
        Some(
            &fields(&[
                ("signature-input", &format!("herald={}", signing.params())),
                ("signature", "herald=:c2ln:"),
            ])[..]
        )
    );
    let shown = presented(&request(&all), FIELD_MOST);
    assert_eq!(
        shown,
        Ok(Presented {
            covered: signing.clone(),
            signature: b"sig".to_vec(),
            base: signing.base(&request(&held)).unwrap_or_default(),
        })
    );
}

#[test]
fn herald_s_signature_is_found_by_its_tag_among_others() {
    let signing = covered(vec![Component::Method]);
    let all = fields(&[
        (
            "Signature-Input",
            &format!(
                "other=(\"@method\");created=1;tag=\"else\", mine={}",
                signing.params()
            ),
        ),
        ("Signature", "other=:AAAA:, mine=:c2ln:"),
    ]);
    assert_eq!(
        presented(&request(&all), FIELD_MOST).map(|shown| shown.signature),
        Ok(b"sig".to_vec())
    );
}

#[test]
fn a_signature_without_parameters_herald_needs_is_unreadable() {
    let complete = r#"("@method");created=1;expires=2;keyid="k";tag="herald""#;
    for (input, signature, refused) in [
        (
            "herald=(\"@method\");tag=\"else\"",
            "herald=:AAAA:",
            Refusal::Unsigned,
        ),
        ("herald=:AAAA:", "herald=:AAAA:", Refusal::Unsigned),
        (
            "herald=\"@method\";tag=\"herald\"",
            "herald=:AAAA:",
            Refusal::Unsigned,
        ),
        (
            "Herald=()",
            "herald=:AAAA:",
            Refusal::Unreadable("Signature-Input is no dictionary"),
        ),
        (
            r#"herald=("@request-target");created=1;expires=2;keyid="k";tag="herald""#,
            "herald=:AAAA:",
            Refusal::Unreadable("a covered part is not one herald reads"),
        ),
        (
            r#"herald=("Upper");created=1;expires=2;keyid="k";tag="herald""#,
            "herald=:AAAA:",
            Refusal::Unreadable("a covered part is not one herald reads"),
        ),
        (
            r#"herald=("");created=1;expires=2;keyid="k";tag="herald""#,
            "herald=:AAAA:",
            Refusal::Unreadable("a covered part is not one herald reads"),
        ),
        (
            r#"herald=(1);created=1;expires=2;keyid="k";tag="herald""#,
            "herald=:AAAA:",
            Refusal::Unreadable("a covered part is not one herald reads"),
        ),
        (
            r#"herald=();created="1";expires=2;keyid="k";tag="herald""#,
            "herald=:AAAA:",
            Refusal::Unreadable("it names no creation time"),
        ),
        (
            r#"herald=();created=1;keyid="k";tag="herald""#,
            "herald=:AAAA:",
            Refusal::Unreadable("it names no expiry"),
        ),
        (
            r#"herald=();created=1;expires=2;keyid=k;tag="herald""#,
            "herald=:AAAA:",
            Refusal::Unreadable("it names no key"),
        ),
        (
            r#"herald=();created=1;expires=2;keyid="k";alg="rsa-v1_5-sha256";tag="herald""#,
            "herald=:AAAA:",
            Refusal::Algorithm("rsa-v1_5-sha256".to_owned()),
        ),
    ] {
        let all = fields(&[("signature-input", input), ("signature", signature)]);
        assert_eq!(
            presented(&request(&all), FIELD_MOST),
            Err(refused),
            "{input} / {signature}"
        );
    }
    let unsigned = fields(&[("signature", "herald=:AAAA:")]);
    assert_eq!(
        presented(&request(&unsigned), FIELD_MOST),
        Err(Refusal::Unsigned)
    );
    let input_only = fields(&[("signature-input", &format!("herald={complete}"))]);
    assert_eq!(
        presented(&request(&input_only), FIELD_MOST),
        Err(Refusal::Unsigned)
    );
}

#[test]
fn a_signature_field_without_bytes_under_herald_s_label_is_unreadable() {
    let complete = r#"("@method");created=1;expires=2;keyid="k";tag="herald""#;
    for (input, signature, refused) in [
        (
            &format!("herald={complete}"),
            "herald=:AAAA",
            Refusal::Unreadable("Signature is no dictionary"),
        ),
        (
            &format!("herald={complete}"),
            "other=:AAAA:",
            Refusal::Unreadable("Signature carries no bytes under the label"),
        ),
        (
            &format!("herald={complete}"),
            "herald=\"text\"",
            Refusal::Unreadable("Signature carries no bytes under the label"),
        ),
        (
            &format!("herald={complete}"),
            "herald=(:AAAA:)",
            Refusal::Unreadable("Signature carries no bytes under the label"),
        ),
        (
            &format!("herald={complete}"),
            "herald=:AAAA:\nx",
            Refusal::Unreadable("a signature field holds a line break"),
        ),
    ] {
        let all = fields(&[("signature-input", input), ("signature", signature)]);
        assert_eq!(
            presented(&request(&all), FIELD_MOST),
            Err(refused),
            "{input} / {signature}"
        );
    }
}

#[test]
fn a_signature_over_a_part_the_message_lacks_has_no_base() {
    let signing = covered(vec![Component::Status]);
    let all = signed(&signing, &[], b"sig");
    assert_eq!(
        presented(&request(&all), FIELD_MOST),
        Err(Refusal::Base(Unsigned::Missing {
            component: "@status".to_owned()
        }))
    );
}

fn shown(created: i64, expires: i64, nonced: bool) -> Presented {
    let mut signing = covered(vec![Component::Method, Component::TargetUri]);
    signing.created = created;
    signing.expires = expires;
    signing.nonce = nonced.then(|| "n".to_owned());
    Presented {
        covered: signing,
        signature: Vec::new(),
        base: Vec::new(),
    }
}

const DEMANDED: [Component; 2] = [Component::Method, Component::TargetUri];

fn demands(single_use: bool) -> Demands<'static> {
    Demands {
        components: &DEMANDED,
        single_use,
        now: 1000,
        skew: 60,
        longest: 300,
    }
}

#[test]
fn a_live_signature_covering_what_is_demanded_meets_the_demands() {
    for presented in [
        shown(1000, 1300, false),
        shown(1060, 1360, false),
        shown(640, 940, false),
    ] {
        assert_eq!(presented.meets(demands(false)), Ok(()));
    }
    assert_eq!(shown(1000, 1300, true).meets(demands(true)), Ok(()));
}

#[test]
fn each_failed_demand_is_named() {
    let partial = Presented {
        covered: covered(vec![Component::Method]),
        signature: Vec::new(),
        base: Vec::new(),
    };
    for (presented, single_use, refused) in [
        (partial, false, Refusal::Uncovered("@target-uri".to_owned())),
        (shown(1000, 1300, false), true, Refusal::Unnonced),
        (shown(1061, 1300, false), false, Refusal::Early),
        (shown(600, 939, false), false, Refusal::Expired),
        (shown(1000, 1301, false), false, Refusal::Lasting),
    ] {
        assert_eq!(presented.meets(demands(single_use)), Err(refused));
    }
}

#[test]
fn every_refusal_reads_as_a_sentence() {
    for (refused, text) in [
        (
            Refusal::Unsigned,
            "the message carries no signature tagged herald",
        ),
        (
            Refusal::Unreadable("it names no key"),
            "the signature cannot be read: it names no key",
        ),
        (
            Refusal::Algorithm("hmac-sha256".to_owned()),
            "the signature is by hmac-sha256, not ed25519",
        ),
        (
            Refusal::Uncovered("@method".to_owned()),
            "the signature does not cover @method",
        ),
        (Refusal::Early, "the signature was made later than now"),
        (Refusal::Expired, "the signature has expired"),
        (Refusal::Lasting, "the signature lasts longer than allowed"),
        (Refusal::Unnonced, "the signature carries no nonce"),
        (
            Refusal::Base(Unsigned::Missing {
                component: "@status".to_owned(),
            }),
            "the message has no @status, which the signature covers",
        ),
    ] {
        assert_eq!(refused.to_string(), text);
    }
}

#[test]
fn every_part_s_name_reads_back_as_the_part() {
    for component in [
        Component::Method,
        Component::TargetUri,
        Component::Authority,
        Component::Scheme,
        Component::Path,
        Component::Query,
        Component::Status,
        Component::Field("content-digest".to_owned()),
    ] {
        assert_eq!(Component::named(component.name()), Some(component));
    }
}

#[test]
fn a_shown_signature_s_base_ends_in_its_own_params_in_their_order() {
    let input = r#"herald=("@method");keyid="k";foo=1;created=1;expires=2;tag="herald""#;
    let all = fields(&[("signature-input", input), ("signature", "herald=:c2ln:")]);
    assert_eq!(
        presented(&request(&all), FIELD_MOST).map(|shown| String::from_utf8(shown.base).unwrap_or_default()),
        Ok(
            "\"@method\": POST\n\"@signature-params\": (\"@method\");keyid=\"k\";foo=1;created=1;expires=2;tag=\"herald\""
                .to_owned()
        )
    );
}

#[test]
fn a_signature_field_longer_than_its_bound_is_refused_before_it_is_read() {
    let signing = covered(vec![Component::Method]);
    let all = signed(&signing, &[], b"sig");
    let input = all[0].1.len();
    let signature = all[1].1.len();
    assert!(presented(&request(&all), input.max(signature)).is_ok());
    assert_eq!(
        presented(&request(&all), input - 1),
        Err(Refusal::Oversized {
            field: SIGNATURE_INPUT,
            most: input - 1
        })
    );
    let flood = fields(&[
        ("signature-input", &format!("herald={}", signing.params())),
        ("signature", &format!("herald=:{}:", "A".repeat(FIELD_MOST))),
    ]);
    assert_eq!(
        presented(&request(&flood), FIELD_MOST),
        Err(Refusal::Oversized {
            field: SIGNATURE,
            most: FIELD_MOST
        })
    );
    assert_eq!(
        Refusal::Oversized {
            field: SIGNATURE,
            most: 10
        }
        .to_string(),
        "the signature field holds more than the 10 bytes read"
    );
}

fn response(fields: &[(String, String)]) -> Message<'_> {
    Message {
        status: Some(200),
        ..request(fields)
    }
}

#[test]
fn a_response_covers_the_request_it_answers_through_req() {
    let held = fields(&[("Content-Type", "application/json")]);
    let signing = covered(vec![
        Component::Status,
        Component::Field("content-type".to_owned()),
        Component::Request(Box::new(Component::Method)),
        Component::Request(Box::new(Component::TargetUri)),
    ]);
    let base = String::from_utf8(signing.base(&response(&held)).unwrap()).unwrap();
    assert_eq!(
        base,
        concat!(
            "\"@status\": 200\n",
            "\"content-type\": application/json\n",
            "\"@method\";req: POST\n",
            "\"@target-uri\";req: https://example.com/foo?param=Value&Pet=dog\n",
            "\"@signature-params\": (\"@status\" \"content-type\" \"@method\";req \"@target-uri\";req)",
            ";created=1618884473;expires=1618884773;keyid=\"z6Mkkey\";alg=\"ed25519\";tag=\"herald\""
        )
    );
    let all = signed(&signing, &held, b"sig");
    let presented = presented(&response(&all), FIELD_MOST).unwrap();
    assert_eq!(presented.covered, signing);
    assert_eq!(presented.base, base.into_bytes());
    assert_eq!(
        signing.base(&request(&held)),
        Err(Unsigned::Missing {
            component: "@status".to_owned()
        })
    );
    let requested = covered(vec![Component::Request(Box::new(Component::Path))]);
    assert_eq!(
        requested.base(&request(&held)),
        Err(Unsigned::Missing {
            component: "@path".to_owned()
        })
    );
    assert_eq!(
        Component::Request(Box::new(Component::Path)).name(),
        "@path"
    );
}

#[test]
fn a_part_marked_otherwise_than_as_the_request_s_is_unreadable() {
    for covered in [
        r#""@method";req=?0"#,
        r#""@method";bs"#,
        r#""@status";req"#,
        r#""content-type";req"#,
        r#""@method" "@path";req;bs"#,
    ] {
        let input = format!(r#"herald=({covered});created=1;expires=2;keyid="k";tag="herald""#);
        let all = fields(&[("signature-input", &input), ("signature", "herald=:AAAA:")]);
        assert_eq!(
            presented(&response(&all), FIELD_MOST),
            Err(Refusal::Unreadable(
                "a covered part is not one herald reads"
            )),
            "{covered}"
        );
    }
}
