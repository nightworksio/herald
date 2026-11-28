use herald_core::{SecretRef, Symbols, WaitOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::cloud::{Cloud, Hosts};
use crate::interactions::Interactions;
use crate::teams::TEAMS;

/// How a Microsoft Teams destination is reached, as `[destinations.*]`
/// writes it: by a Workflows webhook, or as a bot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DestinationOptions {
    /// The secret holding a Workflows webhook's URL: the URL of a workflow
    /// whose trigger is "When a Teams webhook request is received", set so
    /// that anyone may trigger it. It posts to the channel or chat its
    /// workflow names, and only posts.
    pub webhook: Option<SecretRef>,
    /// A bot's Microsoft App ID: the application (client) ID of its
    /// Microsoft Entra app registration. A bot edits its posts, and replies
    /// in their threads where a route says so.
    pub app_id: Option<String>,
    /// The secret holding the bot's client secret, which its app
    /// registration made.
    pub app_password: Option<SecretRef>,
    /// The id of the Microsoft Entra tenant the bot's app is registered in,
    /// for a bot registered for one tenant; where unset, the cloud's own
    /// tenant for bots registered for every tenant: `botframework.com` in
    /// the public cloud.
    pub tenant: Option<String>,
    /// The Microsoft cloud the bot is registered in: `public` where unset,
    /// `gcc-high` or `dod`. It names the hosts below where they are unset.
    pub cloud: Option<Cloud>,
    /// The base URL of the sign-in service the bot asks for its token, in
    /// place of its cloud's, such as `https://login.microsoftonline.com`.
    pub login: Option<String>,
    /// The resource the bot's token is for, in place of its cloud's, such
    /// as `https://api.botframework.com`.
    pub scope: Option<String>,
    /// The service URL the bot posts through, in place of its cloud's, such
    /// as `https://smba.trafficmanager.net/teams/`. An activity Teams posts
    /// to the bot is taken only where the service URL it names is on this
    /// URL's origin.
    pub service: Option<String>,
    /// The address of the Bot Framework's metadata document, in place of
    /// its cloud's, such as
    /// `https://login.botframework.com/v1/.well-known/openidconfiguration`:
    /// it names the keys that sign each token vouching for an activity Teams
    /// posts to the bot.
    pub openid: Option<String>,
    /// The issuer each token vouching for an activity names, in place of its
    /// cloud's, such as `https://api.botframework.com`.
    pub issuer: Option<String>,
    /// How the bot takes what Teams posts to it at herald's endpoint.
    pub interactions: Option<Interactions>,
    /// The characters its posts write where its markup has none: a task
    /// done and to do, a rule and a text list's bullet.
    pub symbols: Option<Symbols>,
    /// How long it waits before sending again where Teams answers that it
    /// is unwell or limited and names no wait.
    pub waits: Option<WaitOptions>,
}

/// What a route says about Microsoft Teams alone.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteOptions {
    /// The id of the channel or chat the bot posts to, such as
    /// `19:…@thread.tacv2`, where its app is installed. A bot needs one; a
    /// webhook posts where its workflow names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<String>,
    /// Whether the bot posts each overflow part as a reply in its post's
    /// thread, through Microsoft's `replyChains`, which Microsoft marks as
    /// a preview; each part is posted on its own where unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threads: Option<bool>,
}

/// What a bot's table names, each host its cloud's where the table names
/// none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Registered<'a> {
    pub(crate) app_id: &'a str,
    pub(crate) hosts: Hosts<'a>,
}

impl DestinationOptions {
    /// Whether it posts as a bot, or else through a webhook.
    ///
    /// # Errors
    ///
    /// When it names neither a webhook nor a bot, both, half a bot, or a
    /// bot's keys beside a webhook.
    pub fn bot(&self) -> Result<bool, String> {
        let bot = [self.app_id.is_some(), self.app_password.is_some()];
        let hosts = [
            ("tenant", self.tenant.is_some()),
            ("cloud", self.cloud.is_some()),
            ("login", self.login.is_some()),
            ("scope", self.scope.is_some()),
            ("service", self.service.is_some()),
            ("openid", self.openid.is_some()),
            ("issuer", self.issuer.is_some()),
            ("interactions", self.interactions.is_some()),
        ];
        match (&self.webhook, bot) {
            (None, [true, true]) => Ok(true),
            (None, [false, false]) => Err(format!(
                "a {TEAMS} destination needs a webhook, or a bot's app_id and app_password; give one"
            )),
            (None, [true, false]) => Err(
                "a bot needs app_password, the secret holding its client secret; give it"
                    .to_owned(),
            ),
            (None, [false, true]) => {
                Err("a bot needs app_id, its app registration's application id; give it".to_owned())
            }
            (Some(_), [false, false]) => hosts.iter().find(|(_, given)| *given).map_or(
                Ok(false),
                |(key, _)| {
                    Err(format!(
                        "a Workflows webhook signs in nowhere, so {key} is a bot's alone; drop it"
                    ))
                },
            ),
            (Some(_), _) => Err(format!(
                "a {TEAMS} destination takes a webhook or a bot, not both; keep one"
            )),
        }
    }

    /// What its bot's table names, each host checked.
    ///
    /// # Errors
    ///
    /// The first key that is not written as it must be, naming it.
    pub(crate) fn registered(&self) -> Result<Option<Registered<'_>>, (&'static str, String)> {
        let Some(app_id) = self.app_id.as_deref() else {
            return Ok(None);
        };
        if !guid(app_id) {
            return Err((
                "app_id",
                "it is not an application id; give the app registration's application (client) id, such as \"00000000-0000-0000-0000-000000000000\"".to_owned(),
            ));
        }
        let cloud = self.cloud.unwrap_or_default().hosts();
        let tenant = self.tenant.as_deref().unwrap_or(cloud.tenant);
        if !tenant_name(tenant) {
            return Err((
                "tenant",
                "it is not a tenant; give the tenant's id or its domain, such as \"contoso.onmicrosoft.com\"".to_owned(),
            ));
        }
        Ok(Some(Registered {
            app_id,
            hosts: Hosts {
                login: based("login", self.login.as_deref(), cloud.login)?,
                tenant,
                scope: based("scope", self.scope.as_deref(), cloud.scope)?,
                service: based("service", self.service.as_deref(), cloud.service)?,
                openid: based("openid", self.openid.as_deref(), cloud.openid)?,
                issuer: based("issuer", self.issuer.as_deref(), cloud.issuer)?,
            },
        }))
    }
}

impl RouteOptions {
    /// Why its conversation, where it names one, is not written as an id.
    #[must_use]
    pub fn unwritten(&self) -> Option<&'static str> {
        self.conversation
            .as_deref()
            .filter(|conversation| {
                conversation.is_empty()
                    || conversation
                        .chars()
                        .any(|character| character.is_whitespace() || character.is_control())
            })
            .map(|_| "it is not a conversation's id; give the channel's or chat's id, such as \"19:…@thread.tacv2\"")
    }
}

/// `given`, or else the cloud's own `base`, where it is an https URL of a
/// host; the key `key` refused otherwise.
fn based<'a>(
    key: &'static str,
    given: Option<&'a str>,
    base: &'static str,
) -> Result<&'a str, (&'static str, String)> {
    let written = given.unwrap_or(base);
    if base_url(written) {
        Ok(written)
    } else {
        Err((
            key,
            format!("it is not an https URL of a host alone; give one, such as \"{base}\""),
        ))
    }
}

/// Whether `id` is written as a GUID: five groups of 8, 4, 4, 4 and 12
/// hexadecimal digits.
fn guid(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(group, length)| {
            group.len() == length && group.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

/// Whether `tenant` is written as a tenant's id or domain: letters,
/// digits, dots and hyphens.
fn tenant_name(tenant: &str) -> bool {
    !tenant.is_empty()
        && tenant
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        && !tenant.starts_with('.')
}

/// Whether `base` is an https URL of a host, with a path at most: no
/// credentials, query or fragment.
fn base_url(base: &str) -> bool {
    Url::parse(base).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_application_id_is_a_guid_and_a_tenant_an_id_or_a_domain() {
        assert!(guid("0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9"));
        for written in [
            "",
            "0a1b2c3d4e5f60718293a4b5c6d7e8f9",
            "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f",
            "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8fg",
            "0a1b2c3d-4e5f-6071-8293-a4b5-c6d7e8f9",
        ] {
            assert!(!guid(written), "{written}");
        }
        for written in ["botframework.com", "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9"] {
            assert!(tenant_name(written), "{written}");
        }
        for written in ["", ".com", "a/b", "a b", "a?b"] {
            assert!(!tenant_name(written), "{written}");
        }
    }

    #[test]
    fn a_host_is_an_https_url_with_no_credential_query_or_fragment() {
        for written in [
            "https://login.microsoftonline.com",
            "https://smba.trafficmanager.net/teams/",
        ] {
            assert!(base_url(written), "{written}");
        }
        for written in [
            "http://login.example",
            "https://a:b@login.example",
            "https://u@login.example",
            "https://login.example/?q=1",
            "https://login.example/#f",
            "login.example",
            "file:///etc",
        ] {
            assert!(!base_url(written), "{written}");
        }
    }
}
