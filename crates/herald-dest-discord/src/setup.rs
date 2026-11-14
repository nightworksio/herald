//! Setting a Discord destination up to take commands: its commands
//! registered and its opt-in roles listed or made (ADR-0016).

use herald_kinds::{Made, Setup, SetupNeeds, SetupStep, SetupTask};
use herald_ports::{Request, Response, Secret};

use crate::config::KIND;
use crate::interact::{Interacting, accepted, listed, made};
use crate::interactions::Interactions;
use crate::options::DestinationOptions;

/// A Discord destination that takes commands, as it is set up.
pub(crate) struct Setting<'a> {
    pub(crate) options: &'a DestinationOptions,
    pub(crate) interactions: &'a Interactions,
}

impl Setup for Setting<'_> {
    fn needs(&self, name: &str, task: SetupTask<'_>) -> Result<SetupNeeds<'_>, String> {
        asked(task)?;
        let token = self.options.token.as_ref().ok_or_else(|| {
            format!("{name} has no bot token, which Discord's commands and roles are set up with")
        })?;
        Ok(SetupNeeds {
            secrets: vec![token],
            tokens: 0,
        })
    }

    fn steps(&self, name: &str, task: SetupTask<'_>, made: Made) -> Result<Vec<SetupStep>, String> {
        let bot = made
            .secrets
            .into_iter()
            .next()
            .ok_or_else(|| format!("{name}'s bot token was not read"))?;
        let (base, request) = request(self.options, self.interactions, bot, task)?;
        Ok(vec![SetupStep::Send { base, request }])
    }

    fn reported(&self, task: SetupTask<'_>, response: &Response) -> Result<Vec<String>, String> {
        reported(self.interactions, task, response)
    }
}

/// `task`, where a Discord destination is set up with it.
fn asked(task: SetupTask<'_>) -> Result<(), String> {
    match task {
        SetupTask::Commands | SetupTask::Roles { .. } => Ok(()),
        SetupTask::Manifest { .. } | SetupTask::Registration { .. } => Err(task.unasked(KIND)),
    }
}

/// The base URL `task` is asked at and its request, of the destination
/// `options`, which takes commands as `interactions`, asked as its bot
/// `bot`.
///
/// # Errors
///
/// Where an id it names is not written as Discord writes one, or a Discord
/// destination is not set up with it.
fn request(
    options: &DestinationOptions,
    interactions: &Interactions,
    bot: Secret,
    task: SetupTask<'_>,
) -> Result<(String, Request), String> {
    asked(task)?;
    let interacting = Interacting::new(interactions.clone(), Some(bot), options.api.as_deref());
    let request = match task {
        SetupTask::Roles {
            guild,
            create: Some(role),
        } => interacting.make_role(guild, role),
        SetupTask::Roles {
            guild,
            create: None,
        } => interacting.roles(guild),
        SetupTask::Commands | SetupTask::Manifest { .. } | SetupTask::Registration { .. } => {
            interacting.register()
        }
    }
    .ok_or_else(|| "the server's id is not written as Discord writes one".to_owned())?;
    Ok((interacting.api().to_owned(), request))
}

/// What is said of `task` done for a destination taking commands as
/// `interactions`, from Discord's `response`.
///
/// # Errors
///
/// What Discord said where it did not do it, or where a Discord destination
/// is not set up with `task`.
fn reported(
    interactions: &Interactions,
    task: SetupTask<'_>,
    response: &Response,
) -> Result<Vec<String>, String> {
    asked(task)?;
    Ok(match task {
        SetupTask::Roles {
            create: Some(_), ..
        } => {
            let (id, made) = made(response)?;
            vec![
                format!("Made the role {made}. Offer it on a route with:"),
                format!("subscribe = {{ role = \"{id}\" }}"),
            ]
        }
        SetupTask::Roles { create: None, .. } => listed(response)?
            .into_iter()
            .map(|(id, role)| format!("{id}  {role}"))
            .collect(),
        SetupTask::Commands | SetupTask::Manifest { .. } | SetupTask::Registration { .. } => {
            accepted(response)?;
            let registered: Vec<&str> = interactions
                .commands
                .iter()
                .map(|command| command.name())
                .collect();
            let place = interactions.guild.as_ref().map_or_else(
                || "in every server the application is in".to_owned(),
                |guild| format!("in the server {guild}"),
            );
            vec![format!(
                "Registered {} with Discord {place}.",
                registered.join(", ")
            )]
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const GUILD: &str = "223456789012345678";

    fn interactions(guild: Option<&str>) -> Interactions {
        serde_json::from_value(json!({
            "application": "123456789012345678",
            "via": "gateway",
            "commands": ["latest", "subscribe"],
            "guild": guild,
        }))
        .unwrap()
    }

    fn response(status: u16, body: &serde_json::Value) -> Response {
        Response {
            status,
            headers: Vec::new(),
            body: body.to_string().into_bytes(),
        }
    }

    #[test]
    fn what_is_said_of_a_task_names_the_commands_and_roles_discord_answered_with() {
        let done = response(200, &json!([]));
        assert_eq!(
            reported(&interactions(None), SetupTask::Commands, &done),
            Ok(vec![
                "Registered latest, subscribe with Discord in every server the application is in."
                    .to_owned()
            ])
        );
        assert_eq!(
            reported(&interactions(Some(GUILD)), SetupTask::Commands, &done),
            Ok(vec![format!(
                "Registered latest, subscribe with Discord in the server {GUILD}."
            )])
        );
        assert_eq!(
            reported(
                &interactions(None),
                SetupTask::Commands,
                &response(403, &json!({ "message": "Missing Access" }))
            ),
            Err("Discord refused it with 403: Missing Access; check what herald's credential is allowed to do there".to_owned())
        );
        let role = json!({ "id": "523456789012345678", "name": "releases" });
        assert_eq!(
            reported(
                &interactions(None),
                SetupTask::Roles {
                    guild: GUILD,
                    create: Some("releases")
                },
                &response(200, &role)
            ),
            Ok(vec![
                "Made the role releases. Offer it on a route with:".to_owned(),
                "subscribe = { role = \"523456789012345678\" }".to_owned(),
            ])
        );
        assert_eq!(
            reported(
                &interactions(None),
                SetupTask::Roles {
                    guild: GUILD,
                    create: None
                },
                &response(200, &json!([role]))
            ),
            Ok(vec!["523456789012345678  releases".to_owned()])
        );
    }

    fn options(token: bool) -> DestinationOptions {
        let mut options = json!({
            "interactions": {
                "application": "123456789012345678",
                "via": "gateway",
                "commands": ["latest"],
            },
        });
        if token {
            options["token"] = json!({ "secret": "DISCORD_BOT" });
        }
        serde_json::from_value(options).unwrap()
    }

    #[test]
    fn a_destination_reads_its_bot_token_and_sends_one_request_for_its_task() {
        use herald_kinds::DestinationConfig as _;

        let unset = options(false);
        assert!(
            serde_json::from_value::<DestinationOptions>(json!({}))
                .unwrap()
                .setup()
                .is_none()
        );
        assert_eq!(
            unset
                .setup()
                .unwrap()
                .needs("chat", SetupTask::Commands)
                .err(),
            Some(
                "chat has no bot token, which Discord's commands and roles are set up with"
                    .to_owned()
            )
        );
        let set = options(true);
        let setup = set.setup().unwrap();
        let needs = setup.needs("chat", SetupTask::Commands).unwrap();
        assert_eq!(needs.tokens, 0);
        assert_eq!(
            needs
                .secrets
                .iter()
                .map(|named| named.secret.as_str())
                .collect::<Vec<_>>(),
            ["DISCORD_BOT"]
        );
        let manifest = SetupTask::Manifest { served: None };
        assert_eq!(
            setup.needs("chat", manifest).err(),
            Some("a discord destination is not set up with a manifest".to_owned())
        );
        assert_eq!(
            setup
                .steps("chat", SetupTask::Commands, Made::default())
                .err(),
            Some("chat's bot token was not read".to_owned())
        );
        let made = || Made {
            secrets: vec![Secret::new("DISCORD_BOT", "bot")],
            tokens: Vec::new(),
        };
        let registration = SetupTask::Registration {
            served: herald_kinds::Served {
                url: "https://herald.example",
                base: "",
            },
        };
        assert_eq!(
            setup.steps("chat", registration, made()).err(),
            Some("a discord destination is not set up with a registration".to_owned())
        );
        let unwritten = SetupTask::Roles {
            guild: "guild",
            create: None,
        };
        assert_eq!(
            setup.steps("chat", unwritten, made()).err(),
            Some("the server's id is not written as Discord writes one".to_owned())
        );
        let (base, request) = request(
            &set,
            set.interactions.as_ref().unwrap(),
            Secret::new("DISCORD_BOT", "bot"),
            SetupTask::Commands,
        )
        .unwrap();
        assert_eq!(base, "https://discord.com/api/v10");
        assert_eq!(
            request.url.expose(),
            "https://discord.com/api/v10/applications/123456789012345678/commands"
        );
        assert_eq!(
            setup.steps("chat", SetupTask::Commands, made()),
            Ok(vec![SetupStep::Send { base, request }])
        );
        assert_eq!(
            setup.reported(SetupTask::Commands, &response(200, &json!([]))),
            Ok(vec![
                "Registered latest with Discord in every server the application is in.".to_owned()
            ])
        );
        assert_eq!(
            setup.reported(manifest, &response(200, &json!([]))).err(),
            Some("a discord destination is not set up with a manifest".to_owned())
        );
    }
}
