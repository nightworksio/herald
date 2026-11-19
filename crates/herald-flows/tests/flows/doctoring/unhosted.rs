//! What a layer with no endpoint names: the instance's discovery document,
//! each herald feed, unless a static signed log serves them, and an owner
//! discovering by push.

use super::*;

#[test]
fn a_layer_with_no_endpoint_names_the_instance_s_discovery_document() {
    let mut federated = served();
    assert!(
        on(&federated, &actions(), &Here::default())
            .gaps
            .iter()
            .all(|gap| gap.key != "federation.domain")
    );
    federated.federation.domain = Some("herald.example".to_owned());
    let findings = on(&federated, &actions(), &Here::default());
    let gap = findings
        .gaps
        .iter()
        .find(|gap| gap.key == "federation.domain")
        .unwrap();
    assert_eq!(gap.feature, "the instance's discovery document");
    assert_eq!(gap.first_layer, Some(Tier::Serverless));
    assert!(gap.why.ends_with(", so no peer reads the instance's keys"));
    assert!(
        on(&federated, &server(), &Here::default())
            .gaps
            .iter()
            .all(|gap| gap.key != "federation.domain")
    );
}

#[test]
fn a_layer_with_no_endpoint_names_each_herald_feed() {
    let mut fed = served();
    fed.destinations.insert(
        herald_core::DestinationName::new("releases"),
        herald_kinds::Destination::new(herald_dest_herald::FeedOptions::default()),
    );
    let findings = on(&fed, &actions(), &Here::default());
    let gap = findings
        .gaps
        .iter()
        .find(|gap| gap.key == "destinations.releases")
        .unwrap();
    assert_eq!(gap.feature, "a herald feed");
    assert_eq!(gap.first_layer, Some(Tier::Serverless));
    assert!(gap.why.ends_with(", so no peer reads its envelopes"));
    assert!(
        on(&fed, &server(), &Here::default())
            .gaps
            .iter()
            .all(|gap| gap.key != "destinations.releases")
    );
}

#[test]
fn a_static_signed_log_serves_the_discovery_document_and_each_feed_with_no_endpoint() {
    let mut logged = served();
    logged.federation.domain = Some("herald.example".to_owned());
    logged.destinations.insert(
        herald_core::DestinationName::new("releases"),
        herald_kinds::Destination::new(herald_dest_herald::FeedOptions::default()),
    );
    logged.federation.pages = Some(herald_config::Pages {
        url: "https://herald.example".to_owned(),
        base: String::new(),
        branch: Some(herald_config::Pages::BRANCH.to_owned()),
        repository: None,
        token: None,
        api: None,
        artifact: None,
        refresh: herald_config::Pages::REFRESH,
    });
    let findings = on(&logged, &actions(), &Here::default());
    assert!(
        findings
            .gaps
            .iter()
            .all(|gap| gap.key != "federation.domain" && gap.key != "destinations.releases"),
        "{:?}",
        findings.gaps
    );
}

#[test]
fn a_layer_with_no_endpoint_names_an_owner_discovering_by_push() {
    let pushed = crate::owner::owned_in("", |options| {
        options.discovery = Some(herald_sources_forge::Discovery::Push);
    });
    let findings = on(&pushed, &actions(), &Here::default());
    let gap = findings
        .gaps
        .iter()
        .find(|gap| gap.key == "sources.gate.discovery")
        .unwrap();
    assert_eq!(gap.feature, "discovery by push");
    assert_eq!(gap.first_layer, Some(Tier::Serverless));
    assert!(gap.why.ends_with(
        ", so the owner is listed only where the source holds no listing or its selection changed; choose discovery \"list\""
    ));
    let mut installed = crate::owner::owned_in("", |_| ());
    installed.github_apps.insert(
        "herald".to_owned(),
        herald_config::GithubApp {
            issuer: "7".to_owned(),
            private_key: herald_core::SecretRef {
                secret: "K".to_owned(),
            },
            api: "https://api.github.com".to_owned(),
            webhook: Some(herald_core::SecretRef {
                secret: "H".to_owned(),
            }),
            store_token: false,
        },
    );
    let gaps = on(&installed, &actions(), &Here::default()).gaps;
    let app = gaps
        .iter()
        .find(|gap| gap.key == "github_apps.herald.webhook")
        .unwrap();
    assert_eq!(app.feature, "a GitHub App's webhooks");
    assert!(app.why.ends_with("; poll the sources reading as the App"));
    assert!(
        on(&installed, &server(), &Here::default())
            .gaps
            .iter()
            .all(|gap| gap.key != "github_apps.herald.webhook")
    );
    let listing = crate::owner::owned_in("", |_| ());
    assert!(
        on(&listing, &actions(), &Here::default())
            .gaps
            .iter()
            .all(|gap| gap.key != "sources.gate.discovery")
    );
}
