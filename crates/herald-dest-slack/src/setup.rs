//! Setting a Slack destination up to take commands: its app manifest
//! (ADR-0016).

use crate::interact::Interacting;
use crate::interactions::{Interactions, Via};
use crate::options::DestinationOptions;

/// What is said to set up the Slack destination `name`, taking commands as
/// `interactions`: its app manifest, its commands posted to `url` where it
/// takes them posted, and what to keep.
///
/// # Errors
///
/// Where it takes commands posted to herald and no `url` is given.
pub fn manifest(
    name: &str,
    options: &DestinationOptions,
    interactions: &Interactions,
    url: Option<&str>,
) -> Result<Vec<String>, String> {
    let posted = interactions.via == Via::Http;
    if posted && url.is_none() {
        return Err(
            "Slack posts commands to herald's address; give it with --url, such as --url https://herald.example".to_owned(),
        );
    }
    let interacting = Interacting::new(interactions.clone(), options.api.as_deref());
    let kept = if posted {
        interactions
            .signing
            .as_ref()
            .map(|named| format!("its signing secret as {}", named.secret))
    } else {
        interactions.app.as_ref().map(|named| {
            format!(
                "an app-level token with connections:write as {}",
                named.secret
            )
        })
    };
    Ok(vec![
        interacting.manifest(name, url.filter(|_| posted)),
        format!(
            "Make a Slack app from this manifest at https://api.slack.com/apps, install it, and keep {}.",
            kept.unwrap_or_default()
        ),
    ])
}

/// Whether the destination taking commands as `interactions` takes them
/// posted to herald's address, so its manifest names it.
#[must_use]
pub fn posted(interactions: &Interactions) -> bool {
    interactions.via == Via::Http
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn interactions(value: &serde_json::Value) -> Interactions {
        serde_json::from_value(value.clone()).unwrap()
    }

    fn options() -> DestinationOptions {
        serde_json::from_value(json!({ "token": { "secret": "HERALD_SLACK_TOKEN" } })).unwrap()
    }

    #[test]
    fn a_manifest_posts_commands_to_herald_or_opens_socket_mode_and_says_what_to_keep() {
        let posting = interactions(&json!({ "signing": { "secret": "HERALD_SLACK_SIGNING" } }));
        assert!(posted(&posting));
        assert_eq!(
            manifest("slack", &options(), &posting, None),
            Err("Slack posts commands to herald's address; give it with --url, such as --url https://herald.example".to_owned())
        );
        let interacting = Interacting::new(posting.clone(), None);
        assert_eq!(
            manifest("slack", &options(), &posting, Some("https://herald.example")),
            Ok(vec![
                interacting.manifest("slack", Some("https://herald.example")),
                "Make a Slack app from this manifest at https://api.slack.com/apps, install it, and keep its signing secret as HERALD_SLACK_SIGNING.".to_owned(),
            ])
        );
        let socketed =
            interactions(&json!({ "via": "socket", "app": { "secret": "HERALD_SLACK_APP" } }));
        assert!(!posted(&socketed));
        assert_eq!(
            manifest("slack", &options(), &socketed, Some("https://herald.example")),
            Ok(vec![
                Interacting::new(socketed.clone(), None).manifest("slack", None),
                "Make a Slack app from this manifest at https://api.slack.com/apps, install it, and keep an app-level token with connections:write as HERALD_SLACK_APP.".to_owned(),
            ])
        );
        assert_eq!(
            manifest("slack", &options(), &socketed, None),
            manifest(
                "slack",
                &options(),
                &socketed,
                Some("https://herald.example")
            )
        );
        assert_ne!(
            interacting.manifest("slack", Some("https://herald.example")),
            interacting.manifest("slack", None)
        );
    }
}
