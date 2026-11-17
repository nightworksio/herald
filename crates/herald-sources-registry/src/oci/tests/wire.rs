use herald_wire::SourceError;
use jiff::Timestamp;

use super::super::wire::{
    Challenge, Manifested, catalog, challenge, created, issued, manifested, names_digest,
    names_tag, next, tags,
};

const LIMIT: usize = 65_536;

const DIGEST: &str = "sha256:6ddf74dfb6187e856dab57d2bf87ab6ae10f093dd7ced7ec1f6d485eff7e7649";

fn reason(error: SourceError) -> String {
    match error {
        SourceError::Unreadable { reason, .. } => reason,
        other => panic!("not unreadable: {other:?}"),
    }
}

#[test]
fn a_bearer_challenge_names_its_realm_service_and_scope_and_basic_asks_for_a_password() {
    assert_eq!(
        challenge(
            r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:library/caddy:pull""#
        ),
        Some(Challenge::Bearer {
            realm: "https://auth.docker.io/token".to_owned(),
            service: Some("registry.docker.io".to_owned()),
            scope: Some("repository:library/caddy:pull".to_owned()),
        })
    );
    assert_eq!(
        challenge(r#"  bearer Realm = "https://r.example/t?a=\"b\"" , SERVICE=svc, scope="a,b""#),
        Some(Challenge::Bearer {
            realm: "https://r.example/t?a=\"b\"".to_owned(),
            service: Some("svc".to_owned()),
            scope: Some("a,b".to_owned()),
        })
    );
    assert_eq!(
        challenge("Bearer realm=https://r.example/t"),
        Some(Challenge::Bearer {
            realm: "https://r.example/t".to_owned(),
            service: None,
            scope: None,
        })
    );
    assert_eq!(
        challenge(r#"Bearer realm="https://r.example/t"#),
        Some(Challenge::Bearer {
            realm: "https://r.example/t".to_owned(),
            service: None,
            scope: None,
        })
    );
    assert_eq!(
        challenge(r#"Basic realm="Registry""#),
        Some(Challenge::Basic)
    );
    assert_eq!(challenge("BASIC"), Some(Challenge::Basic));
    assert_eq!(challenge(r#"Bearer service="svc""#), None);
    assert_eq!(challenge("Bearer"), None);
    assert_eq!(challenge(r#"Digest realm="r""#), None);
}

#[test]
fn a_token_service_issues_its_token_or_access_token() {
    assert_eq!(issued(br#"{"token":"t","access_token":"a"}"#).unwrap(), "t");
    assert_eq!(issued(br#"{"token":"","access_token":"a"}"#).unwrap(), "a");
    assert_eq!(
        reason(issued(br#"{"expires_in":300}"#).unwrap_err()),
        "it issues no token"
    );
    assert!(issued(b"[]").is_err());
}

#[test]
fn a_tag_listing_names_tags_as_registries_give_them() {
    assert_eq!(
        tags(
            LIMIT,
            br#"{"name":"acme/app","tags":["1.0","v2_x","latest"]}"#
        )
        .unwrap(),
        ["1.0", "v2_x", "latest"]
    );
    assert_eq!(
        tags(LIMIT, br#"{"name":"acme/app","tags":null}"#).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        tags(LIMIT, br#"{"name":"acme/app"}"#).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        reason(tags(LIMIT, br#"{"tags":["ok","../x"]}"#).unwrap_err()),
        "it names \"../x\", which is not a tag: up to 128 letters, digits, '_', '.' and '-', not starting with '.' or '-'"
    );
    assert!(tags(4, br#"{"tags":[]}"#).is_err());
}

#[test]
fn a_catalog_names_repositories_as_registries_give_them() {
    assert_eq!(
        catalog(LIMIT, br#"{"repositories":["acme/app","tool"]}"#).unwrap(),
        ["acme/app", "tool"]
    );
    assert_eq!(
        catalog(LIMIT, br#"{"repositories":null}"#).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(catalog(LIMIT, b"{}").unwrap(), Vec::<String>::new());
    assert_eq!(
        reason(catalog(LIMIT, br#"{"repositories":["acme/../x"]}"#).unwrap_err()),
        "it names \"acme/../x\", which is not a repository: names of letters, digits, '.', '_' and '-' joined by '/'"
    );
    assert!(catalog(LIMIT, b"[]").is_err());
}

#[test]
fn a_link_names_a_next_page_by_its_rel() {
    assert!(next(Some(
        r#"</v2/library/caddy/tags/list?last=2-builder&n=3>; rel="next""#
    )));
    assert!(next(Some(
        r#"<a>; rel="prev", <b>; title="x"; REL = "first next""#
    )));
    assert!(next(Some("<b>; rel=next")));
    assert!(!next(Some(r#"<a>; rel="prev""#)));
    assert!(!next(Some(r#"<a>; title="next""#)));
    assert!(!next(Some("next")));
    assert!(!next(None));
}

#[test]
fn a_tag_and_a_digest_are_written_as_the_distribution_spec_writes_them() {
    for tag in ["1.0", "_x", "V2-rc.1", &"a".repeat(128)] {
        assert!(names_tag(tag), "{tag}");
    }
    for tag in ["", ".x", "-x", "a/b", "a b", &"a".repeat(129)] {
        assert!(!names_tag(tag), "{tag}");
    }
    for digest in [DIGEST, "sha256+b64u:abc_-=", "a.b:Z9"] {
        assert!(names_digest(digest), "{digest}");
    }
    for digest in [
        "sha256",
        ":abc",
        "sha256:",
        "SHA256:abc",
        "sha256:a/b",
        "sha..256:abc",
        "sha256-:abc",
    ] {
        assert!(!names_digest(digest), "{digest}");
    }
}

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

#[test]
fn a_manifest_says_when_its_image_was_created_or_where_to_read_on() {
    let annotated = format!(
        r#"{{"annotations":{{"org.opencontainers.image.created":"2025-07-15T19:29:15Z"}},"config":{{"digest":"{DIGEST}"}}}}"#
    );
    assert_eq!(
        manifested(LIMIT, annotated.as_bytes()).unwrap(),
        Manifested::Created(at("2025-07-15T19:29:15Z"))
    );
    let index = format!(
        r#"{{"manifests":[
            {{"digest":"sha256:a1","platform":{{"os":"linux"}},"annotations":{{"vnd.docker.reference.type":"attestation-manifest"}}}},
            {{"digest":"sha256:a2","platform":{{"os":"unknown","architecture":"unknown"}}}},
            {{"digest":"{DIGEST}","platform":{{"os":"linux","architecture":"amd64"}},"annotations":{{"org.opencontainers.image.created":"2025-07-15T19:29:15Z"}}}}
        ]}}"#
    );
    assert_eq!(
        manifested(LIMIT, index.as_bytes()).unwrap(),
        Manifested::Created(at("2025-07-15T19:29:15Z"))
    );
    let unannotated = format!(
        r#"{{"manifests":[{{"digest":"{DIGEST}"}}],"annotations":{{"org.opencontainers.image.created":"1970-01-01T00:00:00Z"}}}}"#
    );
    assert_eq!(
        manifested(LIMIT, unannotated.as_bytes()).unwrap(),
        Manifested::Platform(DIGEST.to_owned())
    );
    let image = format!(
        r#"{{"config":{{"digest":"{DIGEST}"}},"annotations":{{"org.opencontainers.image.created":"not a time"}}}}"#
    );
    assert_eq!(
        manifested(LIMIT, image.as_bytes()).unwrap(),
        Manifested::Config(DIGEST.to_owned())
    );
}

#[test]
fn a_manifest_naming_no_image_or_a_digest_no_registry_gives_is_unreadable() {
    assert_eq!(
        reason(
            manifested(
                LIMIT,
                br#"{"manifests":[{"digest":"sha256:a","platform":{"os":"unknown"}}]}"#
            )
            .unwrap_err()
        ),
        "it names no image: neither a platform's manifest nor a config"
    );
    assert_eq!(
        reason(manifested(LIMIT, br#"{"config":{"digest":"../x"}}"#).unwrap_err()),
        "it names \"../x\", which is not a digest"
    );
    assert_eq!(
        reason(manifested(LIMIT, br#"{"manifests":[{"digest":"x"}]}"#).unwrap_err()),
        "it names \"x\", which is not a digest"
    );
    assert!(manifested(LIMIT, b"[]").is_err());
}

#[test]
fn an_image_config_says_when_it_was_created_unless_at_the_epoch() {
    assert_eq!(
        created(
            LIMIT,
            br#"{"created":"2025-07-15T19:29:15.5Z","os":"linux"}"#
        )
        .unwrap(),
        Some(at("2025-07-15T19:29:15.5Z"))
    );
    assert_eq!(
        created(LIMIT, br#"{"created":"1970-01-01T00:00:00Z"}"#).unwrap(),
        None
    );
    assert_eq!(
        created(LIMIT, br#"{"created":"1970-01-01T00:00:01Z"}"#).unwrap(),
        Some(at("1970-01-01T00:00:01Z"))
    );
    assert_eq!(created(LIMIT, b"{}").unwrap(), None);
    assert!(created(LIMIT, b"[]").is_err());
}
