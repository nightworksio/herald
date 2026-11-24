use std::num::NonZeroU64;

use super::*;

fn named(secret: &str) -> SecretRef {
    SecretRef {
        secret: secret.to_owned(),
    }
}

#[test]
fn a_write_credential_is_one_token_or_one_app_by_its_name() {
    let token = Write {
        token: Some(named("HERALD_MERGE")),
        app: None,
        installation: None,
    };
    let app = Write {
        token: None,
        app: Some("merger".to_owned()),
        installation: NonZeroU64::new(7),
    };
    assert_eq!(token.check(), Ok(()));
    assert_eq!(app.check(), Ok(()));
    let one =
        "a write credential is a token or a GitHub App, one of them; give write.token or write.app";
    let both = Write {
        token: Some(named("HERALD_MERGE")),
        ..app.clone()
    };
    let neither = Write {
        app: None,
        ..app.clone()
    };
    assert_eq!(both.check(), Err(one.to_owned()));
    assert_eq!(neither.check(), Err(one.to_owned()));
    assert_eq!(
        Write {
            installation: NonZeroU64::new(7),
            ..token
        }
        .check(),
        Err("write.installation names the App's installation a token is minted for; give write.app, or remove write.installation".to_owned())
    );
    assert_eq!(
        Write {
            app: Some("Merger".to_owned()),
            ..app
        }
        .check(),
        Err(
            "\"Merger\" is not an App's name; give the name of its [github_apps.<name>] table"
                .to_owned()
        )
    );
}

#[test]
fn only_a_merge_and_a_squash_take_a_message_of_herald_s_choosing() {
    for (method, name, messaged) in [
        (Method::Merge, "merge", true),
        (Method::Squash, "squash", true),
        (Method::Rebase, "rebase", false),
        (Method::FastForward, "fast-forward", false),
        (Method::Repository, "repository", false),
    ] {
        assert_eq!(method.name(), name);
        assert_eq!(method.messaged(), messaged, "{name}");
    }
}

#[test]
fn a_queue_is_the_forge_s_entered_by_the_herald_merge_label_and_waits_two_hours_by_default() {
    assert_eq!(Runs::default(), Runs::Forge);
    assert_eq!(Runs::Forge.name(), "forge");
    assert_eq!(Runs::Herald.name(), "herald");
    assert_eq!(Enter::default(), Enter::Label("herald:merge".to_owned()));
    assert_eq!(Message::default(), Message::Forge);
    assert_eq!(AppliedBy::default(), AppliedBy::Command);
    assert_eq!(WAIT, SignedDuration::from_secs(7_200));
    assert_eq!(Update::Merge.name(), "merge");
    assert_eq!(Update::Rebase.name(), "rebase");
}
