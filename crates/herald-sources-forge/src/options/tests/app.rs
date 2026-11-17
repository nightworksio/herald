use std::num::NonZeroU64;

use herald_kinds::{Installation, Installed};

use super::*;

fn reading(app: &str) -> SourceOptions {
    SourceOptions {
        app: Some(app.to_owned()),
        ..owner("lemonfiber")
    }
}

#[test]
fn an_app_is_named_on_github_in_place_of_a_token_with_an_installation_only_beside_it() {
    assert_eq!(reading("herald").check(Forge::GitHub), Ok(()));
    assert_eq!(
        reading("herald").check(Forge::GitLab),
        Err("app is a GitHub source's; remove it, or give token".to_owned())
    );
    let both = SourceOptions {
        token: Some(SecretRef {
            secret: "GITHUB_TOKEN".to_owned(),
        }),
        ..reading("herald")
    };
    assert_eq!(
        both.check(Forge::GitHub),
        Err("a source reads with its App's tokens or with token, not both; remove one".to_owned())
    );
    assert_eq!(
        reading("Herald").check(Forge::GitHub),
        Err(
            "\"Herald\" is not an App's name; give the name of its [github_apps.<name>] table"
                .to_owned()
        )
    );
    let unread = SourceOptions {
        installation: NonZeroU64::new(42),
        ..owner("lemonfiber")
    };
    assert_eq!(
        unread.check(Forge::GitHub),
        Err("installation names the App's installation a token is minted for; give app, or remove installation".to_owned())
    );
    let fixed = SourceOptions {
        installation: NonZeroU64::new(42),
        ..reading("herald")
    };
    assert_eq!(fixed.check(Forge::GitHub), Ok(()));
    assert!(reading("herald").reads_with_token());
    assert!(!owner("lemonfiber").reads_with_token());
}

fn installed(options: &SourceOptions) -> Option<Installation<'_>> {
    options
        .installed(Forge::GitHub)
        .map(|installed| installed.installation)
}

#[test]
fn an_app_s_installation_is_the_one_named_or_else_the_owner_s_or_the_repository_s() {
    assert_eq!(
        reading("herald").installed(Forge::GitHub),
        Some(Installed {
            app: "herald",
            api: "https://api.github.com".to_owned(),
            installation: Installation::Owner("lemonfiber"),
        })
    );
    let repository = SourceOptions {
        app: Some("herald".to_owned()),
        repository: Some("nightworksio/herald".to_owned()),
        ..SourceOptions::default()
    };
    assert_eq!(
        installed(&repository),
        Some(Installation::Repository("nightworksio/herald"))
    );
    let fixed = SourceOptions {
        installation: NonZeroU64::new(42),
        ..reading("herald")
    };
    assert_eq!(installed(&fixed), Some(Installation::Fixed(42)));
    assert_eq!(installed(&owner("lemonfiber")), None);
    let nameless = SourceOptions {
        app: Some("herald".to_owned()),
        ..SourceOptions::default()
    };
    assert_eq!(installed(&nameless), None);
}
