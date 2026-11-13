//! The commands as Discord registers them.

use herald_core::Phrase;
use herald_ports::{Method, Request};
use serde_json::{Value, json};

use super::{Interacting, PROJECT, SHARE, SUB_COMMAND};

/// An option holding text.
const STRING: u64 = 3;
/// An option holding true or false.
const BOOLEAN: u64 = 5;

impl Interacting {
    /// The commands as Discord registers them, each with the options it
    /// takes: as commands of their own, or as subcommands of the group.
    #[must_use]
    pub fn commands(&self) -> Value {
        let wording = self.interactions.wording();
        let described: Vec<Value> = self
            .interactions
            .commands
            .iter()
            .map(|command| {
                let mut options = vec![json!({
                    "type": STRING,
                    "name": PROJECT,
                    "description": wording.text(Phrase::DescribeProject),
                    "required": false,
                    "autocomplete": true,
                })];
                if command.shares() {
                    options.push(json!({
                        "type": BOOLEAN,
                        "name": SHARE,
                        "description": wording.text(Phrase::DescribeShare),
                        "required": false,
                    }));
                }
                json!({
                    "name": command.name(),
                    "description": wording.describe(*command),
                    "options": options,
                })
            })
            .collect();
        match &self.interactions.group {
            Some(group) => json!([{
                "name": group,
                "description": wording.text(Phrase::DescribeGroup),
                "options": described
                    .into_iter()
                    .map(|mut sub| {
                        if let Some(object) = sub.as_object_mut() {
                            object.insert("type".to_owned(), json!(SUB_COMMAND));
                        }
                        sub
                    })
                    .collect::<Vec<Value>>(),
            }]),
            None => Value::Array(described),
        }
    }

    /// The request that registers the commands, in the server the
    /// interactions name or in every server, replacing any the application
    /// registered before; none without a bot token.
    #[must_use]
    pub(crate) fn register(&self) -> Option<Request> {
        let application = &self.interactions.application;
        let path = match &self.interactions.guild {
            Some(guild) => format!("/applications/{application}/guilds/{guild}/commands"),
            None => format!("/applications/{application}/commands"),
        };
        self.json(Method::Put, &path, &self.commands())
    }
}
