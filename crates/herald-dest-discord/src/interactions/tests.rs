use super::*;

const APPLICATION: &str = "123456789012345678";
const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn read(json: &str) -> Interactions {
    serde_json::from_str(json).unwrap()
}

fn written(extra: &str) -> Interactions {
    read(&format!(
        r#"{{"application": "{APPLICATION}", "key": "{KEY}"{extra}}}"#
    ))
}

#[test]
fn interactions_default_to_http_every_command_and_ephemeral_replies() {
    let interactions = written("");
    assert_eq!(interactions.via, Via::Http);
    assert_eq!(interactions.commands, Command::ALL);
    assert_eq!(interactions.check(), Ok(()));
    assert_eq!(
        interactions.answering(),
        Answering {
            commands: Command::ALL.to_vec(),
            replies: Replies::Ephemeral,
            prereleases: false,
            template: None,
            wording: herald_core::Wording::default(),
        }
    );
    let chosen = written(r#", "replies": "public", "prereleases": true, "template": "short""#);
    assert_eq!(
        chosen.answering(),
        Answering {
            commands: Command::ALL.to_vec(),
            replies: Replies::Public,
            prereleases: true,
            template: Some(TemplateName::new("short")),
            wording: herald_core::Wording::default(),
        }
    );
}

#[test]
fn a_gateway_session_connects_under_discord_gg_and_the_domains_added() {
    let proxied = written(r#", "domains": ["proxy.example"]"#);
    assert_eq!(proxied.check(), Ok(()));
    assert_eq!(proxied.session_domains(), [SESSION_DOMAIN, "proxy.example"]);
    assert_eq!(written("").session_domains(), [SESSION_DOMAIN]);
    assert_eq!(
        written(r#", "domains": ["https://proxy.example"]"#).check(),
        Err("\"https://proxy.example\" is not a domain; give a host such as \"proxy.example\", with no scheme, port or path".to_owned())
    );
}

#[test]
fn the_gateway_needs_no_key_and_http_does() {
    let gateway = read(&format!(
        r#"{{"application": "{APPLICATION}", "via": "gateway"}}"#
    ));
    assert_eq!(gateway.check(), Ok(()));
    let keyless = read(&format!(r#"{{"application": "{APPLICATION}"}}"#));
    assert!(keyless.check().unwrap_err().contains("public key"));
}

#[test]
fn ids_keys_groups_and_commands_written_wrongly_are_refused_with_the_fix() {
    let cases = [
        (
            read(&format!(r#"{{"application": "herald", "key": "{KEY}"}}"#)),
            "\"herald\" is not an application id; give the id from the application's General Information page",
        ),
        (
            read(&format!(
                r#"{{"application": "{APPLICATION}", "key": "beef"}}"#
            )),
            "\"beef\" is not a public key; give the 64 hex digits of the application's public key",
        ),
        (
            written(r#", "group": "Herald""#),
            "\"Herald\" cannot name a command; use 1 to 32 lowercase letters, digits, - or _",
        ),
        (
            written(r#", "guild": "12""#),
            "\"12\" is not a server id; give the server's id",
        ),
        (
            written(r#", "commands": []"#),
            "it answers no command; name at least one, or leave out the interactions",
        ),
    ];
    for (interactions, fix) in cases {
        assert_eq!(interactions.check(), Err(fix.to_owned()));
    }
    assert_eq!(written(r#", "group": "herald_2-x""#).check(), Ok(()));
    assert_eq!(
        written(&format!(r#", "guild": "{APPLICATION}""#)).check(),
        Ok(())
    );
}

#[test]
fn ids_are_seventeen_to_twenty_digits_and_names_up_to_thirty_two() {
    assert!(snowflake(&"1".repeat(17)));
    assert!(snowflake(&"1".repeat(20)));
    assert!(!snowflake(&"1".repeat(16)));
    assert!(!snowflake(&"1".repeat(21)));
    assert!(!snowflake("1234567890123456a"));
    assert!(command_name(&"a".repeat(32)));
    assert!(!command_name(&"a".repeat(33)));
    assert!(!command_name(""));
    assert!(!command_name("a b"));
    assert!(!public_key(&KEY[1..]));
    assert!(!public_key(&KEY.replace('a', "g")));
}

#[test]
fn a_subscribe_names_a_role_by_id_with_a_button_by_default() {
    let subscribe: Subscribe =
        serde_json::from_str(&format!(r#"{{"role": "{APPLICATION}"}}"#)).unwrap();
    assert!(subscribe.button);
    assert_eq!(subscribe.check(), Ok(()));
    let quiet: Subscribe = serde_json::from_str(r#"{"role": "updates", "button": false}"#).unwrap();
    assert!(!quiet.button);
    assert_eq!(
        quiet.check(),
        Err("\"updates\" is not a role id; give the role's id, or make one with herald discord roles".to_owned())
    );
}

#[test]
fn what_it_says_is_worded_as_the_config_words_it_and_refused_where_it_cannot_be_said() {
    let worded = written(r#", "wording": {"nothing": "Niets."}"#);
    assert_eq!(worded.check(), Ok(()));
    assert_eq!(worded.answering().wording.text(Phrase::Nothing), "Niets.");
    let refused = |wording: &str| {
        written(&format!(r#", "wording": {wording}"#))
            .check()
            .unwrap_err()
    };
    assert!(
        refused(r#"{"nothing": "{{ project"}"#).starts_with("wording.nothing is not a template")
    );
    assert!(refused(r#"{"date": "%Y-%"}"#).starts_with("wording.date is not a strftime pattern"));
    let long = "a".repeat(Phrase::DESCRIPTION + 1);
    assert_eq!(
        refused(&format!(r#"{{"describe_share": "{long}"}}"#)),
        format!(
            "wording.describe_share takes 1 to {} characters",
            Phrase::DESCRIPTION
        )
    );
}
