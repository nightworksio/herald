use super::*;

fn read(json: &str) -> Interactions {
    serde_json::from_str(json).unwrap()
}

const TOKENS: &str =
    r#""homeserver_token": {"secret": "HS"}, "appservice_token": {"secret": "AS"}"#;

#[test]
fn interactions_default_to_an_appservice_answering_herald_s_prefix() {
    let interactions = read(&format!("{{{TOKENS}}}"));
    assert_eq!(interactions.via, Via::Appservice);
    assert_eq!(interactions.prefix, "!herald");
    assert_eq!(interactions.sender, "herald");
    assert_eq!(interactions.check(), Ok(()));
    assert_eq!(
        read(&format!(
            r#"{{{TOKENS}, "replies": "public", "prereleases": true, "template": "t", "commands": ["latest"]}}"#
        ))
        .answering(),
        Answering {
            commands: vec![Command::Latest],
            replies: Replies::Public,
            prereleases: true,
            template: Some(TemplateName::new("t")),
            wording: herald_core::Wording::default(),
        }
    );
}

#[test]
fn an_appservice_needs_both_tokens_and_syncing_needs_neither() {
    for json in [
        "{}",
        r#"{"homeserver_token": {"secret": "HS"}}"#,
        r#"{"appservice_token": {"secret": "AS"}}"#,
    ] {
        assert!(read(json).check().unwrap_err().contains("two tokens"));
    }
    assert_eq!(read(r#"{"via": "sync"}"#).check(), Ok(()));
}

#[test]
fn prefixes_senders_and_commands_written_wrongly_are_refused() {
    for prefix in ["", "! herald"] {
        assert!(
            read(&format!(r#"{{"via": "sync", "prefix": "{prefix}"}}"#))
                .check()
                .unwrap_err()
                .contains("cannot start a command")
        );
    }
    for sender in ["", "Herald", "her ald"] {
        assert!(
            read(&format!(r#"{{"via": "sync", "sender": "{sender}"}}"#))
                .check()
                .unwrap_err()
                .contains("is not a localpart")
        );
    }
    assert_eq!(
        read(r#"{"via": "sync", "sender": "a.b_c=d-e/f9"}"#).check(),
        Ok(())
    );
    assert!(read(r#"{"via": "sync", "commands": []}"#).check().is_err());
}
