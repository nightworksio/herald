//! Setting a Matrix destination up to take commands: the registration its
//! homeserver installs (ADR-0016).

use herald_core::SecretRef;
use herald_kinds::{Made, Served, Setup, SetupNeeds, SetupStep, SetupTask};
use herald_ports::{Response, Secret};

use crate::config::KIND;
use crate::interact::Interacting;
use crate::interactions::{Interactions, Via};
use crate::options::DestinationOptions;

/// Whether the homeserver of the Matrix destination `name`, taking
/// commands as `interactions`, installs a registration.
///
/// # Errors
///
/// Where it reads commands by syncing, so the homeserver installs nothing.
fn installs(name: &str, interactions: &Interactions) -> Result<(), String> {
    if interactions.via == Via::Appservice {
        Ok(())
    } else {
        Err(format!(
            "{name} reads commands by syncing, so the homeserver installs nothing"
        ))
    }
}

/// What is said to set up the Matrix destination `name`, taking commands as
/// `interactions`: the registration its homeserver installs, reaching
/// herald at `url`, with the tokens `homeserver` and `appservice`.
///
/// # Errors
///
/// Where it reads commands by syncing, so the homeserver installs nothing.
fn registration(
    name: &str,
    options: &DestinationOptions,
    interactions: &Interactions,
    url: &str,
    tokens: (&str, &str),
) -> Result<Vec<String>, String> {
    installs(name, interactions)?;
    let (homeserver, appservice) = tokens;
    let interacting = Interacting::new(
        &options.homeserver,
        interactions.clone(),
        Secret::new("the application service's token", appservice),
    );
    let secret_of =
        |named: Option<&SecretRef>| named.map(|named| named.secret.clone()).unwrap_or_default();
    let mut said = vec![format!(
        "Keep the homeserver's token as {} and the application service's token as {}, then give the homeserver's administrator this registration:",
        secret_of(interactions.homeserver_token.as_ref()),
        secret_of(interactions.appservice_token.as_ref())
    )];
    said.extend(
        interacting
            .registration(
                name,
                &format!("{url}/matrix/{name}"),
                homeserver,
                appservice,
            )
            .lines()
            .map(str::to_owned),
    );
    said.push("herald prints these tokens only this once.".to_owned());
    Ok(said)
}

/// A Matrix destination that takes commands, as it is set up.
pub(crate) struct Setting<'a> {
    pub(crate) options: &'a DestinationOptions,
    pub(crate) interactions: &'a Interactions,
}

/// The tokens a registration hands out: the homeserver's and the
/// application service's.
const TOKENS: usize = 2;

/// Why nothing a homeserver answers is read: setting it up asks the
/// homeserver nothing.
const UNASKED: &str = "setting a Matrix destination up asks its homeserver nothing";

impl Setup for Setting<'_> {
    fn needs(&self, name: &str, task: SetupTask<'_>) -> Result<SetupNeeds<'_>, String> {
        served(task)?;
        installs(name, self.interactions)?;
        Ok(SetupNeeds {
            secrets: Vec::new(),
            tokens: TOKENS,
        })
    }

    fn steps(&self, name: &str, task: SetupTask<'_>, made: Made) -> Result<Vec<SetupStep>, String> {
        let url = served(task)?.address()?;
        let [homeserver, appservice] = <[String; TOKENS]>::try_from(made.tokens)
            .map_err(|_| format!("{name}'s registration needs {TOKENS} new tokens"))?;
        let said = registration(
            name,
            self.options,
            self.interactions,
            &url,
            (&homeserver, &appservice),
        )?;
        Ok(vec![SetupStep::Say(said)])
    }

    fn said(&self, _: SetupTask<'_>, _: &Response) -> Result<Vec<String>, String> {
        Err(UNASKED.to_owned())
    }
}

/// Where `task` says herald is served, where a Matrix destination is set
/// up with it.
fn served(task: SetupTask<'_>) -> Result<Served<'_>, String> {
    match task {
        SetupTask::Registration { served } => Ok(served),
        SetupTask::Commands | SetupTask::Roles { .. } | SetupTask::Manifest { .. } => {
            Err(task.unasked(KIND))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn interactions(via: &str) -> Interactions {
        serde_json::from_value(json!({
            "via": via,
            "homeserver_token": { "secret": "HERALD_MATRIX_HS" },
            "appservice_token": { "secret": "HERALD_MATRIX_AS" },
        }))
        .unwrap()
    }

    #[test]
    fn a_registration_reaches_herald_by_destination_and_names_the_tokens_to_keep() {
        let options: DestinationOptions = serde_json::from_value(json!({
            "homeserver": "https://matrix.example",
            "token": { "secret": "HERALD_MATRIX_TOKEN" },
        }))
        .unwrap();
        let pushed = interactions("appservice");
        assert_eq!(installs("matrix", &pushed), Ok(()));
        let said = registration(
            "matrix",
            &options,
            &pushed,
            "https://herald.example",
            ("hs-token", "as-token"),
        )
        .unwrap();
        let interacting = Interacting::new(
            "https://matrix.example",
            pushed.clone(),
            Secret::new("the application service's token", "as-token"),
        );
        let mut expected = vec!["Keep the homeserver's token as HERALD_MATRIX_HS and the application service's token as HERALD_MATRIX_AS, then give the homeserver's administrator this registration:".to_owned()];
        expected.extend(
            interacting
                .registration(
                    "matrix",
                    "https://herald.example/matrix/matrix",
                    "hs-token",
                    "as-token",
                )
                .lines()
                .map(str::to_owned),
        );
        expected.push("herald prints these tokens only this once.".to_owned());
        assert_eq!(said, expected);
        assert!(said.len() > 2);
        let refused = "matrix reads commands by syncing, so the homeserver installs nothing";
        assert_eq!(
            installs("matrix", &interactions("sync")),
            Err(refused.to_owned())
        );
        assert_eq!(
            registration(
                "matrix",
                &options,
                &interactions("sync"),
                "https://herald.example",
                ("hs-token", "as-token"),
            ),
            Err(refused.to_owned())
        );
    }

    #[test]
    fn a_destination_prints_its_registration_with_two_new_tokens_and_asks_nothing() {
        use herald_kinds::DestinationConfig as _;

        let options = |via: &str| -> DestinationOptions {
            serde_json::from_value(json!({
                "homeserver": "https://matrix.example",
                "token": { "secret": "HERALD_MATRIX_TOKEN" },
                "interactions": {
                    "via": via,
                    "homeserver_token": { "secret": "HERALD_MATRIX_HS" },
                    "appservice_token": { "secret": "HERALD_MATRIX_AS" },
                },
            }))
            .unwrap()
        };
        let task = SetupTask::Registration {
            served: Served {
                url: "https://herald.example/",
                base: "",
            },
        };
        let synced = options("sync");
        assert_eq!(
            synced.setup().unwrap().needs("matrix", task),
            Err("matrix reads commands by syncing, so the homeserver installs nothing".to_owned())
        );
        let pushed = options("appservice");
        let setup = pushed.setup().unwrap();
        assert_eq!(
            setup.needs("matrix", task),
            Ok(SetupNeeds {
                secrets: Vec::new(),
                tokens: 2
            })
        );
        assert_eq!(
            setup.needs("matrix", SetupTask::Commands),
            Err("a matrix destination is not set up with commands".to_owned())
        );
        assert_eq!(
            setup
                .steps("matrix", SetupTask::Commands, Made::default())
                .err(),
            Some("a matrix destination is not set up with commands".to_owned())
        );
        assert_eq!(
            setup.steps("matrix", task, Made::default()).err(),
            Some("matrix's registration needs 2 new tokens".to_owned())
        );
        let made = Made {
            secrets: Vec::new(),
            tokens: vec!["hs-token".to_owned(), "as-token".to_owned()],
        };
        let steps = setup.steps("matrix", task, made).unwrap();
        let [SetupStep::Say(said)] = steps.as_slice() else {
            panic!("{steps:?}");
        };
        let interactions = pushed.interactions.clone().unwrap();
        assert_eq!(
            Ok(said.clone()),
            registration(
                "matrix",
                &pushed,
                &interactions,
                "https://herald.example",
                ("hs-token", "as-token")
            )
        );
        let response = Response {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert_eq!(setup.said(task, &response), Err(UNASKED.to_owned()));
    }
}
