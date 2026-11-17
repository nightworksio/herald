use std::num::NonZeroUsize;

use herald_core::{Duration, Size, Visibility};
use jiff::SignedDuration;

use super::super::options::{LISTED_PAGE_ALLOWED, MOST_TAGS_ALLOWED};
use super::*;

fn refused(options: &OciOptions) -> (&'static str, String) {
    options.check().unwrap_err()
}

fn many(count: usize) -> Option<NonZeroUsize> {
    NonZeroUsize::new(count)
}

#[test]
fn a_registry_is_a_web_address_naming_no_user() {
    assert_eq!(
        refused(&of("ftp://registry.example", "acme/app")).0,
        "registry"
    );
    assert_eq!(
        refused(&of("https://me:pw@registry.example", "acme/app")),
        (
            "registry",
            "the registry's URL names a user; give username and token instead".to_owned()
        )
    );
    assert_eq!(of("https://registry.example", "acme/app").check(), Ok(()));
}

#[test]
fn a_source_names_one_repository_or_the_catalog_and_the_catalog_s_keys_only_beside_it() {
    assert_eq!(
        refused(&OciOptions {
            catalog: true,
            ..of("https://registry.example", "acme/app")
        })
        .0,
        "catalog"
    );
    assert_eq!(
        refused(&OciOptions {
            registry: "https://registry.example".to_owned(),
            ..OciOptions::default()
        }),
        (
            "repository",
            "give repository, or catalog = true to take every repository the registry lists"
                .to_owned()
        )
    );
    for bad in ["", "acme//app", "acme/../app", "acme/app?x", "/acme"] {
        assert_eq!(
            refused(&of("https://registry.example", bad)).0,
            "repository",
            "{bad}"
        );
    }
    let one = of("https://registry.example", "acme/team/app");
    assert_eq!(one.check(), Ok(()));
    for (key, options) in [
        (
            "include",
            OciOptions {
                include: vec!["a*".to_owned()],
                ..one.clone()
            },
        ),
        (
            "exclude",
            OciOptions {
                exclude: vec!["a*".to_owned()],
                ..one.clone()
            },
        ),
        (
            "most_listed",
            OciOptions {
                most_listed: many(5),
                ..one.clone()
            },
        ),
        (
            "most_members",
            OciOptions {
                most_members: many(5),
                ..one.clone()
            },
        ),
        (
            "keep_members",
            OciOptions {
                keep_members: Some(Duration(SignedDuration::from_hours(1))),
                ..one.clone()
            },
        ),
    ] {
        assert_eq!(
            refused(&options),
            (
                key,
                format!(
                    "{key} chooses among the catalog's repositories; give catalog = true instead of repository, or remove {key}"
                )
            )
        );
    }
}

#[test]
fn a_catalog_s_selection_is_globs_keeping_at_most_the_most_members_allowed() {
    let catalog = catalogued("https://registry.example");
    assert_eq!(catalog.check(), Ok(()));
    assert_eq!(
        refused(&OciOptions {
            include: vec!["[".to_owned()],
            ..catalog.clone()
        })
        .0,
        "include"
    );
    assert_eq!(
        refused(&OciOptions {
            exclude: vec!["[".to_owned()],
            ..catalog.clone()
        })
        .0,
        "include"
    );
    assert_eq!(
        refused(&OciOptions {
            most_members: many(100_001),
            ..catalog.clone()
        }),
        (
            "most_members",
            "most_members is at most 100000; lower it".to_owned()
        )
    );
    assert_eq!(
        OciOptions {
            most_members: many(100_000),
            ..catalog.clone()
        }
        .check(),
        Ok(())
    );
    let selection = OciOptions {
        include: vec!["acme/*".to_owned()],
        exclude: vec!["acme/old".to_owned()],
        ..catalog
    }
    .selection()
    .unwrap();
    assert_eq!(selection.include.len(), 1);
    assert_eq!(selection.exclude.len(), 1);
    assert_eq!(
        selection.visibility,
        [
            Visibility::Public,
            Visibility::Internal,
            Visibility::Private
        ]
    );
}

#[test]
fn a_username_and_a_token_come_together() {
    let one = of("https://registry.example", "acme/app");
    assert_eq!(
        refused(&OciOptions {
            username: Some("me".to_owned()),
            ..one.clone()
        })
        .0,
        "username"
    );
    assert_eq!(
        refused(&OciOptions {
            token: Some(token()),
            ..one.clone()
        })
        .0,
        "token"
    );
    assert_eq!(
        OciOptions {
            username: Some("me".to_owned()),
            token: Some(token()),
            ..one
        }
        .check(),
        Ok(())
    );
}

#[test]
fn auth_and_blobs_are_web_addresses_and_the_registry_s_name_a_lowercase_host() {
    let one = of("https://registry.example", "acme/app");
    assert_eq!(
        refused(&OciOptions {
            auth: Some("auth.example".to_owned()),
            ..one.clone()
        })
        .0,
        "auth"
    );
    assert_eq!(
        refused(&OciOptions {
            blobs: Some(vec![
                "https://cdn.example".to_owned(),
                "cdn.example".to_owned()
            ]),
            ..one.clone()
        }),
        (
            "blobs",
            "\"cdn.example\" is not a web address naming no user; give the base URL a blob is redirected to, such as https://cdn.registry.example".to_owned()
        )
    );
    assert_eq!(
        refused(&OciOptions {
            auth: Some("https://me:pw@auth.example".to_owned()),
            ..one.clone()
        })
        .0,
        "auth"
    );
    assert_eq!(
        refused(&OciOptions {
            blobs: Some(vec!["https://me@cdn.example".to_owned()]),
            ..one.clone()
        })
        .0,
        "blobs"
    );
    for bad in ["Docker.io", "docker.io/x", "", "a b"] {
        assert_eq!(
            refused(&OciOptions {
                registry_name: Some(bad.to_owned()),
                ..one.clone()
            })
            .0,
            "registry_name",
            "{bad}"
        );
    }
    let sound = OciOptions {
        auth: Some("https://gitlab.example".to_owned()),
        blobs: Some(vec!["https://cdn.example".to_owned()]),
        registry_name: Some("registry.example:5000".to_owned()),
        ..one
    };
    assert_eq!(sound.check(), Ok(()));
}

#[test]
fn moving_tags_are_globs_latest_where_absent() {
    let one = of("https://registry.example", "acme/app");
    let moving = one.moving().unwrap();
    assert_eq!(moving.len(), 1);
    assert!(moving[0].matches("latest"));
    let none = OciOptions {
        moving: Some(Vec::new()),
        ..one.clone()
    };
    assert_eq!(none.moving().unwrap().len(), 0);
    let bad = OciOptions {
        moving: Some(vec!["[".to_owned()]),
        ..one
    };
    assert_eq!(refused(&bad).0, "moving");
    assert!(bad.moving().unwrap_err().starts_with("moving: "));
}

#[test]
fn a_listing_s_page_and_tags_are_bounded_and_the_cursor_remembers_them_all() {
    let one = of("https://registry.example", "acme/app");
    assert_eq!(
        refused(&OciOptions {
            listed_page: many(LISTED_PAGE_ALLOWED + 1),
            ..one.clone()
        }),
        (
            "listed_page",
            "listed_page is at most 10000; lower it".to_owned()
        )
    );
    assert_eq!(
        OciOptions {
            listed_page: many(LISTED_PAGE_ALLOWED),
            ..one.clone()
        }
        .check(),
        Ok(())
    );
    assert_eq!(
        refused(&OciOptions {
            most_tags: many(MOST_TAGS_ALLOWED + 1),
            remembered: many(MOST_TAGS_ALLOWED + 1),
            ..one.clone()
        }),
        (
            "most_tags",
            "most_tags is at most 100000; lower it".to_owned()
        )
    );
    assert_eq!(
        OciOptions {
            most_tags: many(MOST_TAGS_ALLOWED),
            ..one.clone()
        }
        .check(),
        Ok(())
    );
    assert_eq!(
        refused(&OciOptions {
            remembered: many(9_999),
            ..one.clone()
        }),
        (
            "remembered",
            "the cursor remembers every tag a repository lists, so remembered is at least most_tags, 10000; raise it, or lower most_tags".to_owned()
        )
    );
    assert_eq!(
        OciOptions {
            remembered: many(10_000),
            ..one.clone()
        }
        .check(),
        Ok(())
    );
    assert_eq!(
        refused(&OciOptions {
            most_tags: many(10),
            page: many(11),
            ..one.clone()
        })
        .0,
        "page"
    );
    assert_eq!(
        refused(&OciOptions {
            parse: Some(Size(herald_wire::PARSE_LIMIT * 4 + 1)),
            ..one.clone()
        })
        .0,
        "parse"
    );
    assert_eq!(
        refused(&OciOptions {
            most_tags: many(30),
            page: many(30),
            parse: Some(Size(herald_wire::PARSE_LIMIT * 4 + 1)),
            ..one
        })
        .0,
        "parse"
    );
}

#[test]
fn docker_hub_s_registry_trusts_its_auth_redirects_to_its_cdns_and_names_its_images_docker_io() {
    for host in [
        "registry-1.docker.io",
        "index.docker.io",
        "registry.hub.docker.com",
        "docker.io",
    ] {
        let hub = of(&format!("https://{host}/"), "caddy");
        assert!(hub.docker_hub(), "{host}");
        assert_eq!(hub.registry(), format!("https://{host}"));
        assert_eq!(hub.host(), host);
        assert_eq!(hub.auth().as_deref(), Some("https://auth.docker.io"));
        assert_eq!(
            hub.blobs(),
            [
                "https://production.cloudfront.docker.com",
                "https://production.cloudflare.docker.com"
            ]
        );
        assert_eq!(hub.registry_name(), "docker.io");
        assert_eq!(hub.repository_of("caddy"), "library/caddy");
        assert_eq!(hub.repository_of("acme/app"), "acme/app");
    }
}

#[test]
fn ghcr_redirects_to_its_cdn_and_another_registry_trusts_only_what_its_options_name() {
    let ghcr = of("https://ghcr.io", "acme/app");
    assert!(!ghcr.docker_hub());
    assert_eq!(ghcr.auth(), None);
    assert_eq!(
        ghcr.blobs(),
        ["https://pkg-containers.githubusercontent.com"]
    );
    assert_eq!(ghcr.registry_name(), "ghcr.io");
    let other = of("https://Registry.Example:5000", "app");
    assert_eq!(other.host(), "registry.example:5000");
    assert_eq!(other.auth(), None);
    assert_eq!(other.blobs(), Vec::<String>::new());
    assert_eq!(other.registry_name(), "registry.example:5000");
    assert_eq!(other.repository_of("app"), "app");
    let named = OciOptions {
        auth: Some("https://gitlab.example/".to_owned()),
        blobs: Some(vec!["https://cdn.example".to_owned()]),
        registry_name: Some("docker.io".to_owned()),
        ..of("https://mirror.example", "library/caddy")
    };
    assert_eq!(named.auth().as_deref(), Some("https://gitlab.example"));
    assert_eq!(named.blobs(), ["https://cdn.example"]);
    assert_eq!(named.registry_name(), "docker.io");
    assert_eq!(OciOptions::default().host(), "");
}

#[test]
fn defaults_read_ten_thousand_tags_in_pages_of_a_thousand_remembering_them_all() {
    let one = of("https://registry.example", "acme/app");
    assert_eq!(one.listed_page(), 1_000);
    assert_eq!(one.most_tags().get(), 10_000);
    assert_eq!(one.most_listed().get(), 10_000);
    assert_eq!(one.most_members().get(), 1_000);
    assert_eq!(one.keep_members(), SignedDuration::from_hours(30 * 24));
    let reading = one.reading();
    assert_eq!(reading.page, 30);
    assert_eq!(reading.remembered, 10_000);
    assert_eq!(reading.parse, herald_wire::PARSE_LIMIT);
    assert_eq!(one.remembering_key(), "most_tags");
    let set = OciOptions {
        listed_page: many(50),
        most_tags: many(200),
        most_listed: many(7),
        most_members: many(6),
        keep_members: Some(Duration(SignedDuration::from_hours(2))),
        remembered: many(300),
        ..one
    };
    assert_eq!(set.listed_page(), 50);
    assert_eq!(set.most_tags().get(), 200);
    assert_eq!(set.most_listed().get(), 7);
    assert_eq!(set.most_members().get(), 6);
    assert_eq!(set.keep_members(), SignedDuration::from_hours(2));
    assert_eq!(set.reading().remembered, 300);
    assert_eq!(set.remembering_key(), "remembered");
}

#[test]
fn repositories_are_public_unless_a_token_reads_them_undeclared() {
    let one = of("https://registry.example", "acme/app");
    assert_eq!(one.visibility(), Visibility::Public);
    let tokened = OciOptions {
        token: Some(token()),
        ..one
    };
    assert_eq!(tokened.visibility(), Visibility::Private);
    assert_eq!(
        OciOptions {
            visibility: Some(Declared::Public),
            ..tokened
        }
        .visibility(),
        Visibility::Public
    );
}
