//! The Slack Web API methods herald calls with its bot token, each with the
//! bot scopes Slack's method page lists for it in public and private
//! channels. The app manifest asks for every scope they name.

/// A Web API method and the bot scopes it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WebMethod {
    /// The method's name, as its URL ends.
    pub(crate) name: &'static str,
    /// The bot scopes it needs.
    pub(crate) scopes: &'static [&'static str],
}

const HISTORY: &[&str] = &["channels:history", "groups:history"];

/// Who the token is.
pub(crate) const AUTH_TEST: WebMethod = WebMethod {
    name: "auth.test",
    scopes: &[],
};
/// The bot user behind a bot id.
pub(crate) const BOTS_INFO: WebMethod = WebMethod {
    name: "bots.info",
    scopes: &["users:read"],
};
/// A message's permalink.
pub(crate) const CHAT_GET_PERMALINK: WebMethod = WebMethod {
    name: "chat.getPermalink",
    scopes: &[],
};
/// A message posted.
pub(crate) const CHAT_POST_MESSAGE: WebMethod = WebMethod {
    name: "chat.postMessage",
    scopes: &["chat:write"],
};
/// A channel's messages.
pub(crate) const CONVERSATIONS_HISTORY: WebMethod = WebMethod {
    name: "conversations.history",
    scopes: HISTORY,
};
/// A thread's replies.
pub(crate) const CONVERSATIONS_REPLIES: WebMethod = WebMethod {
    name: "conversations.replies",
    scopes: HISTORY,
};
/// A message pinned.
pub(crate) const PINS_ADD: WebMethod = WebMethod {
    name: "pins.add",
    scopes: &["pins:write"],
};
/// A channel's pins.
pub(crate) const PINS_LIST: WebMethod = WebMethod {
    name: "pins.list",
    scopes: &["pins:read"],
};
/// A message unpinned.
pub(crate) const PINS_REMOVE: WebMethod = WebMethod {
    name: "pins.remove",
    scopes: &["pins:write"],
};

/// Every method herald calls with its bot token.
pub(crate) const METHODS: [WebMethod; 9] = [
    AUTH_TEST,
    BOTS_INFO,
    CHAT_GET_PERMALINK,
    CHAT_POST_MESSAGE,
    CONVERSATIONS_HISTORY,
    CONVERSATIONS_REPLIES,
    PINS_ADD,
    PINS_LIST,
    PINS_REMOVE,
];

/// The scope its slash command needs.
const COMMANDS: &str = "commands";

/// The bot scopes the app manifest asks for: every method's, and the slash
/// command's, each once and in order.
pub(crate) fn bot_scopes() -> Vec<&'static str> {
    let mut scopes: Vec<&str> = METHODS
        .iter()
        .flat_map(|method| method.scopes.iter().copied())
        .chain([COMMANDS])
        .collect();
    scopes.sort_unstable();
    scopes.dedup();
    scopes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_asks_for_every_scope_each_method_needs_once() {
        assert_eq!(
            bot_scopes(),
            [
                "channels:history",
                "chat:write",
                "commands",
                "groups:history",
                "pins:read",
                "pins:write",
                "users:read",
            ]
        );
    }
}
