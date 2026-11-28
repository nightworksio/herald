use herald_core::{SecretRef, Symbols, WaitOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How a Microsoft Teams destination is reached, as `[destinations.*]`
/// writes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DestinationOptions {
    /// The secret holding a Workflows webhook's URL: the URL of a workflow
    /// whose trigger is "When a Teams webhook request is received", set so
    /// that anyone may trigger it. It posts to the channel or chat its
    /// workflow names, and only posts.
    pub webhook: SecretRef,
    /// The characters its posts write where its markup has none: a task
    /// done and to do, a rule and a text list's bullet.
    pub symbols: Option<Symbols>,
    /// How long it waits before sending again where Teams answers that it
    /// is unwell or limited and names no wait.
    pub waits: Option<WaitOptions>,
}

/// What a route says about Microsoft Teams alone: nothing, as a Workflows
/// webhook posts where its workflow names.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteOptions {}
