//! Setting a Slack destination up to take commands: its app manifest
//! (ADR-0016).

use herald_kinds::{Made, Served, Setup, SetupNeeds, SetupStep, SetupTask};
use herald_ports::Response;

use crate::config::KIND;
use crate::interact::Interacting;
use crate::interactions::{Interactions, Via};
use crate::options::DestinationOptions;

/// What is said to set up the Slack destination `name`, taking commands as
/// `interactions`: its app manifest, its commands posted where `served` says
/// where it takes them posted, and what to keep.
///
/// # Errors
///
/// Where it takes commands posted to herald and no sound address is given.
fn manifest(
    name: &str,
    options: &DestinationOptions,
    interactions: &Interactions,
    served: Option<Served<'_>>,
) -> Result<Vec<String>, String> {
    let posted = interactions.via == Via::Http;
    let address = match served.filter(|_| posted) {
        Some(served) => Some(served.address()?),
        None => None,
    };
    if posted && address.is_none() {
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
        interacting.manifest(name, address.as_deref()),
        format!(
            "Make a Slack app from this manifest at https://api.slack.com/apps, install it, and keep {}.",
            kept.unwrap_or_default()
        ),
    ])
}

/// A Slack destination that takes commands, as it is set up.
pub(crate) struct Setting<'a> {
    pub(crate) options: &'a DestinationOptions,
    pub(crate) interactions: &'a Interactions,
}

impl Setup for Setting<'_> {
    fn needs(&self, _: &str, task: SetupTask<'_>) -> Result<SetupNeeds<'_>, String> {
        served(task).map(|_| SetupNeeds::default())
    }

    fn steps(&self, name: &str, task: SetupTask<'_>, _: Made) -> Result<Vec<SetupStep>, String> {
        let said = manifest(name, self.options, self.interactions, served(task)?)?;
        Ok(vec![SetupStep::Say(said)])
    }

    fn reported(&self, _: SetupTask<'_>, _: &Response) -> Result<Vec<String>, String> {
        Err(UNASKED.to_owned())
    }
}

/// Why nothing Slack answers is read: setting it up asks Slack nothing.
const UNASKED: &str = "setting a Slack destination up asks Slack nothing";

/// Where `task` says herald is served, where a Slack destination is set up
/// with it.
fn served(task: SetupTask<'_>) -> Result<Option<Served<'_>>, String> {
    match task {
        SetupTask::Manifest { served } => Ok(served),
        SetupTask::Commands | SetupTask::Roles { .. } | SetupTask::Registration { .. } => {
            Err(task.unasked(KIND))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn interactions(value: &serde_json::Value) -> Interactions {
        serde_json::from_value(value.clone()).unwrap()
    }

    const HERALD: Served<'static> = Served {
        url: "https://herald.example/",
        base: "",
    };

    fn options() -> DestinationOptions {
        serde_json::from_value(json!({ "token": { "secret": "HERALD_SLACK_TOKEN" } })).unwrap()
    }

    #[test]
    fn a_manifest_posts_commands_to_herald_or_opens_socket_mode_and_says_what_to_keep() {
        let posting = interactions(&json!({ "signing": { "secret": "HERALD_SLACK_SIGNING" } }));
        assert_eq!(
            manifest("slack", &options(), &posting, None),
            Err("Slack posts commands to herald's address; give it with --url, such as --url https://herald.example".to_owned())
        );
        let interacting = Interacting::new(posting.clone(), None);
        assert_eq!(
            manifest("slack", &options(), &posting, Some(HERALD)),
            Ok(vec![
                interacting.manifest("slack", Some("https://herald.example")),
                "Make a Slack app from this manifest at https://api.slack.com/apps, install it, and keep its signing secret as HERALD_SLACK_SIGNING.".to_owned(),
            ])
        );
        let unsound = Served {
            url: "herald.example",
            base: "",
        };
        assert_eq!(
            manifest("slack", &options(), &posting, Some(unsound)),
            Err(unsound.address().unwrap_err())
        );
        let socketed =
            interactions(&json!({ "via": "socket", "app": { "secret": "HERALD_SLACK_APP" } }));
        assert_eq!(
            manifest("slack", &options(), &socketed, Some(HERALD)),
            Ok(vec![
                Interacting::new(socketed.clone(), None).manifest("slack", None),
                "Make a Slack app from this manifest at https://api.slack.com/apps, install it, and keep an app-level token with connections:write as HERALD_SLACK_APP.".to_owned(),
            ])
        );
        assert_eq!(
            manifest("slack", &options(), &socketed, None),
            manifest("slack", &options(), &socketed, Some(HERALD))
        );
        assert_ne!(
            interacting.manifest("slack", Some("https://herald.example")),
            interacting.manifest("slack", None)
        );
    }

    #[test]
    fn a_destination_prints_its_manifest_and_asks_slack_nothing() {
        use herald_kinds::DestinationConfig as _;

        let socketed: DestinationOptions = serde_json::from_value(json!({
            "token": { "secret": "HERALD_SLACK_TOKEN" },
            "interactions": { "via": "socket", "app": { "secret": "HERALD_SLACK_APP" } },
        }))
        .unwrap();
        assert!(options().setup().is_none());
        let setup = socketed.setup().unwrap();
        let task = SetupTask::Manifest { served: None };
        assert_eq!(setup.needs("slack", task), Ok(SetupNeeds::default()));
        assert_eq!(
            setup.needs("slack", SetupTask::Commands),
            Err("a slack destination is not set up with commands".to_owned())
        );
        let interactions = socketed.interactions.clone().unwrap();
        assert_eq!(
            setup.steps("slack", task, Made::default()),
            Ok(vec![SetupStep::Say(
                manifest("slack", &socketed, &interactions, None).unwrap()
            )])
        );
        assert_eq!(
            setup
                .steps("slack", SetupTask::Commands, Made::default())
                .err(),
            Some("a slack destination is not set up with commands".to_owned())
        );
        let response = Response {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert_eq!(setup.reported(task, &response), Err(UNASKED.to_owned()));
    }
}
