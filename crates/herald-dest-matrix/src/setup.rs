//! Setting a Matrix destination up to take commands: the registration its
//! homeserver installs (ADR-0016).

use herald_core::SecretRef;
use herald_ports::Secret;

use crate::interact::Interacting;
use crate::interactions::{Interactions, Via};
use crate::options::DestinationOptions;

/// Whether the homeserver of the Matrix destination `name`, taking
/// commands as `interactions`, installs a registration.
///
/// # Errors
///
/// Where it reads commands by syncing, so the homeserver installs nothing.
pub fn installs(name: &str, interactions: &Interactions) -> Result<(), String> {
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
pub fn registration(
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
}
