use super::*;

fn read(json: &str) -> Interactions {
    serde_json::from_str(json).unwrap()
}

const SIGNED: &str = r#"{"signing": {"secret": "HERALD_SLACK_SIGNING"}"#;

#[test]
fn a_socket_mode_session_connects_under_slack_com_and_the_domains_added() {
    let proxied = read(&format!(r#"{SIGNED}, "domains": ["proxy.example"]}}"#));
    assert_eq!(proxied.check(), Ok(()));
    assert_eq!(
        proxied.session_domains(Domain::Slack),
        [Domain::Slack.host(), "proxy.example"]
    );
    assert_eq!(
        read(&format!("{SIGNED}}}")).session_domains(Domain::Slack),
        [Domain::Slack.host()]
    );
    assert_eq!(
        read(&format!(r#"{SIGNED}, "domains": ["https://proxy.example"]}}"#)).check(),
        Err("\"https://proxy.example\" is not a domain; give a host such as \"proxy.example\", with no scheme, port or path".to_owned())
    );
}

#[test]
fn interactions_default_to_http_the_herald_command_and_every_command() {
    let interactions = read(&format!("{SIGNED}}}"));
    assert_eq!(interactions.via, Via::Http);
    assert_eq!(interactions.command, "/herald");
    assert_eq!(interactions.commands, Command::ALL);
    assert_eq!(interactions.check(), Ok(()));
    assert_eq!(
        read(&format!(
            r#"{SIGNED}, "replies": "public", "prereleases": true, "template": "t"}}"#
        ))
        .answering(),
        Answering {
            commands: Command::ALL.to_vec(),
            replies: Replies::Public,
            prereleases: true,
            template: Some(TemplateName::new("t")),
        }
    );
}

#[test]
fn each_transport_needs_its_own_secret() {
    assert!(read("{}").check().unwrap_err().contains("signing secret"));
    assert!(
        read(r#"{"via": "socket"}"#)
            .check()
            .unwrap_err()
            .contains("app-level token")
    );
    assert_eq!(
        read(r#"{"via": "socket", "app": {"secret": "HERALD_SLACK_APP"}}"#).check(),
        Ok(())
    );
}

#[test]
fn an_app_name_slack_would_refuse_is_refused() {
    let longest = "a".repeat(35);
    for name in ["acme.releases_1-x", longest.as_str()] {
        assert_eq!(
            read(&format!(r#"{SIGNED}, "name": "{name}"}}"#)).check(),
            Ok(())
        );
    }
    let longer = "a".repeat(36);
    for name in ["", "Acme", "acme releases", longer.as_str()] {
        assert_eq!(
            read(&format!(r#"{SIGNED}, "name": "{name}"}}"#)).check(),
            Err(format!(
                "\"{name}\" is not an app name; give up to 35 lowercase letters, digits, ., - or _, such as \"herald\""
            ))
        );
    }
}

#[test]
fn a_slash_command_and_commands_written_wrongly_are_refused() {
    for command in ["herald", "/", "/Herald", &format!("/{}", "a".repeat(32))] {
        assert!(
            read(&format!(r#"{SIGNED}, "command": "{command}"}}"#))
                .check()
                .unwrap_err()
                .starts_with(&format!("\"{command}\" is not a slash command"))
        );
    }
    assert_eq!(
        read(&format!(
            r#"{SIGNED}, "command": "/{}"}}"#,
            "a_-1".repeat(7)
        ))
        .check(),
        Ok(())
    );
    assert!(
        read(&format!(r#"{SIGNED}, "commands": []}}"#))
            .check()
            .is_err()
    );
}

#[test]
fn a_subscribe_carries_a_button_unless_it_says_not() {
    assert!(Subscribe::default().button);
    let quiet: Subscribe = serde_json::from_str(r#"{"button": false}"#).unwrap();
    assert!(!quiet.button);
    let plain: Subscribe = serde_json::from_str("{}").unwrap();
    assert!(plain.button);
}
