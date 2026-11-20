//! The keys that let an operator re-run a planned job.

use super::*;
use crate::options::write::web_of;
use crate::options::{SignIn, Write};

fn named(secret: &str) -> SecretRef {
    SecretRef {
        secret: secret.to_owned(),
    }
}

fn written(write: Write) -> SourceOptions {
    SourceOptions {
        repository: Some("o/r".to_owned()),
        token: Some(named("HERALD_READ")),
        write: Some(write),
        ..SourceOptions::default()
    }
}

fn token(secret: &str) -> Write {
    Write {
        token: Some(named(secret)),
        app: None,
        installation: None,
    }
}

#[test]
fn a_write_credential_is_one_token_or_one_app_never_the_one_the_source_reads_with() {
    assert_eq!(written(token("HERALD_RERUN")).check(Forge::GitHub), Ok(()));
    let refused = [
        (written(token("HERALD_READ")), Forge::GitHub, "reads with"),
        (
            written(Write {
                token: Some(named("HERALD_RERUN")),
                app: Some("rerun".to_owned()),
                installation: None,
            }),
            Forge::GitHub,
            "one of them",
        ),
        (
            written(Write {
                token: None,
                app: None,
                installation: None,
            }),
            Forge::GitHub,
            "one of them",
        ),
        (
            written(Write {
                token: Some(named("HERALD_RERUN")),
                app: None,
                installation: std::num::NonZeroU64::new(1),
            }),
            Forge::GitHub,
            "write.installation",
        ),
        (
            written(Write {
                token: None,
                app: Some("rerun".to_owned()),
                installation: None,
            }),
            Forge::GitLab,
            "GitHub source's",
        ),
        (
            written(Write {
                token: None,
                app: Some("Not An App".to_owned()),
                installation: None,
            }),
            Forge::GitHub,
            "not an App's name",
        ),
        (written(token("HERALD_RERUN")), Forge::Forgejo, "Forgejo"),
    ];
    for (options, forge, said) in refused {
        let error = options.check(forge).unwrap_err();
        assert!(error.contains(said), "{error}");
    }
    let mut same_app = written(Write {
        token: None,
        app: Some("reader".to_owned()),
        installation: None,
    });
    same_app.token = None;
    same_app.app = Some("reader".to_owned());
    let error = same_app.check(Forge::GitHub).unwrap_err();
    assert!(error.contains("the source reads as"), "{error}");
}

#[test]
fn a_sign_in_names_a_client_id_and_a_web_root() {
    let signed = |client_id: &str, web: Option<&str>| SourceOptions {
        repository: Some("o/r".to_owned()),
        sign_in: Some(SignIn {
            client_id: client_id.to_owned(),
            scopes: None,
            web: web.map(ToOwned::to_owned),
            least: None,
        }),
        ..SourceOptions::default()
    };
    assert_eq!(signed("Iv23liClient", None).check(Forge::GitHub), Ok(()));
    assert!(signed("", None).check(Forge::GitHub).is_err());
    assert!(signed("a b", None).check(Forge::GitLab).is_err());
    assert!(
        signed("Iv23liClient", Some("not a url"))
            .check(Forge::GitHub)
            .is_err()
    );
    assert!(signed("c", None).check(Forge::Forgejo).is_err());
}

#[test]
fn a_sign_in_asks_for_scopes_that_only_read() {
    let scoped = |scopes: &[&str]| SourceOptions {
        repository: Some("o/r".to_owned()),
        sign_in: Some(SignIn {
            client_id: "Iv23liClient".to_owned(),
            scopes: Some(scopes.iter().map(|scope| (*scope).to_owned()).collect()),
            web: None,
            least: None,
        }),
        ..SourceOptions::default()
    };
    assert_eq!(
        scoped(&["read:user", "read:org"]).check(Forge::GitHub),
        Ok(())
    );
    assert_eq!(
        scoped(&["read_api", "read_user", "openid", "profile", "email"]).check(Forge::GitLab),
        Ok(())
    );
    for (scopes, forge) in [
        (["read:org", "repo"], Forge::GitHub),
        (["read:user", "workflow"], Forge::GitHub),
        (["read_api", "api"], Forge::GitLab),
        (["read_api", "read_api"], Forge::GitHub),
    ] {
        let refused = scoped(&scopes).check(forge).unwrap_err();
        assert!(refused.contains("can do more than read"), "{refused}");
        assert!(refused.contains(scopes[1]), "{refused}");
    }
    assert_eq!(unread(Forge::GitHub, ["read:org"]), None);
    assert_eq!(unread(Forge::GitLab, ["read:org"]), Some("read:org"));
}

#[test]
fn a_web_root_is_the_api_s_without_its_api_part_alone() {
    for (api, web) in [
        ("https://api.github.com", "https://github.com"),
        ("https://api.acme.ghe.com", "https://acme.ghe.com"),
        ("https://ghes.example/api/v3", "https://ghes.example"),
        (
            "https://api.git.example.com/api/v3",
            "https://api.git.example.com",
        ),
        (
            "https://api.git.example.com/api/v3/",
            "https://api.git.example.com",
        ),
    ] {
        assert_eq!(web_of(Forge::GitHub, api), web, "{api}");
    }
}

#[test]
fn an_app_writes_for_the_installation_it_names_or_the_repository_s() {
    let app = |installation| {
        let options = written(Write {
            token: None,
            app: Some("rerun".to_owned()),
            installation,
        });
        match options.writing(Forge::GitHub) {
            Some(herald_kinds::WriteCredential::App(installed)) => (
                installed.app.to_owned(),
                format!("{:?}", installed.installation),
            ),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(app(None).0, "rerun");
    assert_ne!(app(None).1, app(std::num::NonZeroU64::new(9)).1);
    assert_eq!(SourceOptions::default().writing(Forge::GitHub), None);
}
