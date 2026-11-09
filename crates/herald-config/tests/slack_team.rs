//! A Slack destination's `team`: the workspace an Enterprise Grid org-wide
//! app's calls name, refused unless written as a workspace id.

mod common;

#[cfg(test)]
mod slack_team {
    use herald_config::load;
    use herald_core::DestinationName;

    fn config(team: &str) -> Result<herald_config::Config, String> {
        let text = format!(
            r#"version = 1

[store]
kind = "markers"

[destinations.slack]
kind = "slack"
token = {{ secret = "HERALD_SLACK_TOKEN" }}
{team}

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "releases"
sources = ["gate"]
destination = "slack"
channel = "C0123456789"
template = "release"

[templates.release]
title = "{{{{ event.project }}}} {{{{ event.version }}}}"
body = "{{{{ event.body }}}}"
"#
        );
        load(
            "herald.toml",
            &text,
            &|name| Err(format!("{name}: no such file")),
            &crate::common::kinds(),
        )
        .map_err(|errors| errors.to_string())
    }

    #[test]
    fn a_workspace_id_is_kept_and_anything_else_is_refused() {
        let kept = config("team = \"T0123456789\"").unwrap();
        let Some(options) = kept
            .destinations
            .get(&DestinationName::new("slack"))
            .and_then(|destination| destination.of::<herald_dest_slack::DestinationOptions>())
        else {
            panic!("no Slack destination");
        };
        assert_eq!(options.team.as_deref(), Some("T0123456789"));
        for team in ["E0123456789", "T", "T0", "t0123456789", "T01234567-9"] {
            let refused = config(&format!("team = \"{team}\"")).unwrap_err();
            assert!(
                refused.contains(&format!("\"{team}\" is not a workspace id")),
                "{refused}"
            );
        }
        assert!(config("").is_ok());
    }
}
