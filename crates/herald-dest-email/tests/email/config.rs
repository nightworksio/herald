//! An email table and route, read and checked as the config reads them:
//! each key of Amazon SES, the recipients a route names, the secrets it
//! reads.

use herald_core::{Delivery, Features, MentionStyle, Pins, Threads};
use herald_dest_email::{
    DestinationOptions, EmailKind, FEWEST_BYTES, KIND, MOST_BYTES, RouteOptions, SES_BYTES,
    UNCHECKED, profile, regional,
};
use herald_kinds::{
    Checking, DestinationConfig, DestinationKind, Fault, Placed, Placement, Placing,
};
use serde_json::{Value, json};

fn options(extra: &Value) -> DestinationOptions {
    let mut table = json!({
        "transport": "ses",
        "from": "herald <releases@example.org>",
        "region": "eu-west-1",
    });
    for (key, value) in extra.as_object().unwrap() {
        table[key] = value.clone();
    }
    serde_json::from_value(table).unwrap()
}

fn checked(extra: &Value) -> (DestinationOptions, Vec<Fault>) {
    EmailKind::checked(options(extra), &|_| Err(String::new())).unwrap()
}

fn faults(extra: &Value) -> Vec<Fault> {
    checked(extra).1
}

fn route(written: &Value) -> RouteOptions {
    serde_json::from_value(written.clone()).unwrap()
}

fn placed(written: &Value) -> Vec<Fault> {
    EmailKind::placed(
        &options(&json!({})),
        &route(written),
        Placing {
            destination: "mail",
            subscribers: false,
        },
    )
}

fn addresses(count: usize, role: &str) -> Value {
    Value::Array(
        (0..count)
            .map(|at| json!(format!("{role}{at}@example.org")))
            .collect(),
    )
}

#[test]
fn an_email_table_names_its_sender_its_region_and_the_secrets_aws_names() {
    let (options, faults) = checked(&json!({}));
    assert_eq!(faults, []);
    assert_eq!(options.kind(), KIND);
    assert_eq!(options.most(), MOST_BYTES);
    assert_eq!(
        options.url(),
        Ok("https://email.eu-west-1.amazonaws.com".to_owned())
    );
    let named = |reference: Option<&herald_core::SecretRef>| {
        reference.map(|reference| reference.secret.clone())
    };
    assert_eq!(
        named(options.credential()),
        Some("AWS_SECRET_ACCESS_KEY".to_owned())
    );
    let reads: Vec<&str> = options
        .reads()
        .iter()
        .map(|reference| reference.secret.as_str())
        .collect();
    assert_eq!(reads, ["AWS_ACCESS_KEY_ID"]);
    let (tokened, _) = checked(&json!({
        "access_key_id": { "secret": "MAIL_ID" },
        "secret_access_key": { "secret": "MAIL_KEY" },
        "session_token": { "secret": "AWS_SESSION_TOKEN" },
    }));
    let reads: Vec<&str> = tokened
        .reads()
        .iter()
        .map(|reference| reference.secret.as_str())
        .collect();
    assert_eq!(reads, ["MAIL_ID", "AWS_SESSION_TOKEN"]);
    let every: Vec<&str> = tokened
        .secret_refs()
        .iter()
        .map(|reference| reference.secret.as_str())
        .collect();
    assert_eq!(every, ["MAIL_ID", "MAIL_KEY", "AWS_SESSION_TOKEN"]);
    let placed = Placed::new(RouteOptions::default());
    assert_eq!(options.checking(&placed), Checking::Unchecked(UNCHECKED));
    assert_eq!(options.public(&placed, &Delivery::default()), None);
    assert_eq!(options.profile(&placed), Some((profile(MOST_BYTES), None)));
}

#[test]
fn an_email_profile_replies_to_its_first_message_and_fills_a_twelfth_of_it() {
    let shown = profile(120_000);
    assert_eq!(shown.limits.text, 10_000);
    assert_eq!(shown.limits.followup, 10_000);
    assert_eq!(shown.limits.title, 256);
    assert_eq!(
        shown.features,
        Features {
            threads: Threads::Replies,
            crosspost: false,
            pins: Pins::None,
            reads_back: false,
            edits: false,
            mentions: MentionStyle::None,
            interactions: false,
        }
    );
    let (bounded, _) = checked(&json!({ "max_bytes": 8192 }));
    assert_eq!(bounded.most(), 8192);
    assert_eq!(
        bounded.profile(&Placed::new(RouteOptions::default())),
        Some((profile(8192), None))
    );
}

#[test]
fn each_region_is_reached_at_the_endpoint_aws_lists_for_it() {
    for (region, fips, dual, url) in [
        (
            "eu-west-1",
            false,
            false,
            "https://email.eu-west-1.amazonaws.com",
        ),
        ("eu-west-1", false, true, "https://email.eu-west-1.api.aws"),
        (
            "us-east-1",
            true,
            false,
            "https://email-fips.us-east-1.amazonaws.com",
        ),
        (
            "us-gov-west-1",
            true,
            true,
            "https://email-fips.us-gov-west-1.api.aws",
        ),
        (
            "ca-west-1",
            true,
            false,
            "https://email-fips.ca-west-1.amazonaws.com",
        ),
    ] {
        assert_eq!(regional(region, fips, dual), Ok(url.to_owned()), "{region}");
    }
    assert_eq!(
        regional("eu-west-1", true, false).map_err(|(key, _)| key),
        Err("fips")
    );
    assert_eq!(
        regional("cn-north-1", false, false).map_err(|(key, _)| key),
        Err("region")
    );
    let (given, faults) = checked(&json!({
        "region": "eu-central-3",
        "endpoint": "https://vpce-1.email.eu-west-1.vpce.amazonaws.com",
    }));
    assert_eq!(faults, []);
    assert_eq!(
        given.url(),
        Ok("https://vpce-1.email.eu-west-1.vpce.amazonaws.com".to_owned())
    );
}

#[test]
fn a_key_amazon_ses_cannot_take_is_refused_at_load_by_its_name() {
    let at = |extra: Value| {
        faults(&extra)
            .into_iter()
            .map(|fault| fault.key.unwrap_or_default())
            .collect::<Vec<_>>()
    };
    assert_eq!(at(json!({ "region": "cn-north-1" })), ["region"]);
    assert_eq!(at(json!({ "fips": true })), ["fips"]);
    assert_eq!(at(json!({ "fips": false })), Vec::<String>::new());
    assert_eq!(
        at(json!({ "endpoint": "https://ses.example", "fips": false })),
        ["fips"]
    );
    assert_eq!(
        at(json!({ "endpoint": "https://ses.example", "dual_stack": true })),
        ["dual_stack"]
    );
    for endpoint in [
        "http://ses.example",
        "https://u@ses.example",
        "https://u:p@ses.example",
        "https://ses.example/?q",
        "https://ses.example/#f",
        "ses.example",
    ] {
        assert_eq!(
            at(json!({ "endpoint": endpoint })),
            ["endpoint"],
            "{endpoint}"
        );
    }
    for region in ["", "EU-WEST-1", "eu west 1"] {
        assert_eq!(
            at(json!({ "region": region, "endpoint": "https://ses.example" })),
            ["region"],
            "{region}"
        );
    }
    let unregioned: DestinationOptions =
        serde_json::from_value(json!({ "transport": "ses", "from": "a@example.org" })).unwrap();
    let (_, unregioned) = EmailKind::checked(unregioned, &|_| Err(String::new())).unwrap();
    assert_eq!(
        unregioned,
        [Fault::at(
            "region",
            "Amazon SES sends from a region; give it, such as \"eu-west-1\""
        )]
    );
    for most in [FEWEST_BYTES, SES_BYTES] {
        assert_eq!(at(json!({ "max_bytes": most })), Vec::<String>::new());
    }
    for most in [FEWEST_BYTES - 1, SES_BYTES + 1] {
        assert_eq!(at(json!({ "max_bytes": most })), ["max_bytes"], "{most}");
    }
    for set in ["herald", "a-b_C9", &"s".repeat(64)] {
        assert_eq!(
            at(json!({ "configuration_set": set })),
            Vec::<String>::new()
        );
    }
    for set in ["", "a b", "a.b", &"s".repeat(65)] {
        assert_eq!(
            at(json!({ "configuration_set": set })),
            ["configuration_set"],
            "{set}"
        );
    }
}

#[test]
fn a_table_whose_address_is_not_one_herald_writes_is_refused_without_repeating_it() {
    for (from, why) in [
        ("releases", "is no address"),
        ("a\r\nBcc: x@example.org", "holds a control character"),
        ("a..b@example.org", "has a local part herald does not write"),
    ] {
        let error = serde_json::from_value::<DestinationOptions>(
            json!({ "transport": "ses", "from": from, "region": "eu-west-1" }),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(why), "{error}");
        assert!(!error.contains(from), "{error}");
    }
    assert!(
        serde_json::from_value::<DestinationOptions>(
            json!({ "transport": "smtp", "from": "a@example.org" })
        )
        .is_err()
    );
}

#[test]
fn a_route_names_its_recipients_as_a_list_or_one_address_each_once() {
    assert_eq!(placed(&json!({ "to": "team@example.org" })), []);
    assert_eq!(
        route(&json!({ "to": "Team <team@example.org>" })),
        route(&json!({ "to": ["Team <team@example.org>"] }))
    );
    assert_eq!(
        placed(&json!({})),
        [Fault::here(
            "the route names no recipient: give `to`, `cc` or `bcc` at least one address"
        )]
    );
    assert_eq!(
        placed(&json!({ "to": "a@example.org", "bcc": "a@EXAMPLE.org" })),
        [Fault::here(
            "the route names one address twice among `to`, `cc` and `bcc`"
        )]
    );
    assert_eq!(
        placed(
            &json!({ "to": addresses(20, "t"), "cc": addresses(20, "c"), "bcc": addresses(10, "b") })
        ),
        []
    );
    assert_eq!(
        placed(
            &json!({ "to": addresses(20, "t"), "cc": addresses(20, "c"), "bcc": addresses(11, "b") })
        ),
        [Fault::here(
            "the route names more than the 50 recipients Amazon SES takes in one message, `to`, `cc` and `bcc` together"
        )]
    );
    assert_eq!(
        placed(&json!({ "to": addresses(101, "t") })),
        [Fault::here(
            "the route names more than the 100 recipients every mail server takes in one message"
        )]
    );
    let error = serde_json::from_value::<RouteOptions>(json!({ "to": 7 }))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("an address or a list of addresses"),
        "{error}"
    );
    let error = serde_json::from_value::<RouteOptions>(json!({ "cc": ["a@b@c"] }))
        .unwrap_err()
        .to_string();
    assert!(error.contains("the address"), "{error}");
    assert!(!error.contains("a@b@c"), "{error}");
}

#[test]
fn a_route_is_written_and_recalled_whole_and_a_recalled_route_naming_none_is_refused() {
    let options = options(&json!({}));
    let written = route(&json!({
        "to": ["Team <team@example.org>"],
        "bcc": "hidden@example.org",
    }));
    let text = written.written().unwrap();
    assert_eq!(
        text,
        r#"{"to":["Team <team@example.org>"],"bcc":["hidden@example.org"]}"#
    );
    assert_eq!(written.place(), None);
    assert_eq!(
        options
            .recalled(&text)
            .and_then(|placed| placed.of::<RouteOptions>().cloned()),
        Some(written)
    );
    assert!(options.recalled("{}").is_none());
    let placed = Placed::new(route(&json!({ "to": "team@example.org" })));
    let key = herald_ports::Secret::new("AWS_SECRET_ACCESS_KEY", "k".repeat(40));
    assert!(options.reach(&placed, "", key.clone()).is_none());
    let id = herald_ports::Secret::new("AWS_ACCESS_KEY_ID", "i".repeat(20));
    assert!(
        options
            .reach_reading(&placed, "", (key, vec![id]))
            .is_some()
    );
    assert!(options.recalled("not json").is_none());
}
