use schemars::JsonSchema;
use serde::Deserialize;

/// The Microsoft cloud a Teams bot is registered in: each names the hosts
/// its bot signs in at and posts through, as Microsoft's documentation
/// gives them (ADR-0093).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Cloud {
    /// Microsoft's public cloud.
    #[default]
    Public,
    /// Office 365 GCC High, on Azure Government.
    GccHigh,
    /// The Department of Defense's environment, on Azure Government.
    Dod,
}

/// The hosts a bot signs in at and posts through, and those that vouch for
/// what Teams posts to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hosts<'a> {
    /// The sign-in service's base URL.
    pub login: &'a str,
    /// The tenant a bot registered for every tenant signs in under.
    pub tenant: &'a str,
    /// The resource a bot's token is for, which its scope names.
    pub scope: &'a str,
    /// The service URL a bot posts through where no message from Teams
    /// named another.
    pub service: &'a str,
    /// The address of the Bot Framework's metadata document, which names
    /// the keys that sign each token vouching for what Teams posts to a bot.
    pub openid: &'a str,
    /// The issuer each such token names.
    pub issuer: &'a str,
}

/// Microsoft's public cloud: sign-in, tenant and scope from
/// <https://learn.microsoft.com/en-us/azure/bot-service/rest-api/bot-framework-rest-connector-authentication?view=azure-bot-service-4.0>
/// ("Step 1: Request an access token", multi-tenant), the service URL from
/// <https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/conversations/send-proactive-messages>
/// ("Create the conversation", "Public"), the metadata document and the
/// issuer from the first page ("Authenticate requests from the Bot
/// Connector service to your bot", steps 2 and 4).
const PUBLIC: Hosts<'static> = Hosts {
    login: "https://login.microsoftonline.com",
    tenant: "botframework.com",
    scope: "https://api.botframework.com",
    service: "https://smba.trafficmanager.net/teams/",
    openid: "https://login.botframework.com/v1/.well-known/openidconfiguration",
    issuer: "https://api.botframework.com",
};

/// Office 365 GCC High: sign-in, tenant and scope from
/// <https://learn.microsoft.com/en-us/azure/bot-service/how-to-deploy-gov-cloud-high?view=azure-bot-service-4.0>
/// ("Configure for Office 365 GCC High": `ToChannelFromBotLoginUrl`,
/// `ToChannelFromBotOAuthScope`, `ToBotFromChannelOpenIdMetadataUrl`,
/// `ToBotFromChannelTokenIssuer`), the service URL from the proactive
/// messages page ("GCC High").
const GCC_HIGH: Hosts<'static> = Hosts {
    login: "https://login.microsoftonline.us",
    tenant: "MicrosoftServices.onmicrosoft.us",
    scope: "https://api.botframework.us",
    service: "https://smba.infra.gov.teams.microsoft.us/teams",
    openid: "https://login.botframework.azure.us/v1/.well-known/openidconfiguration",
    issuer: "https://api.botframework.us",
};

/// The Department of Defense's environment: sign-in, tenant and scope
/// from the same page (its section on that environment), the service URL
/// from the proactive messages page (its `dod` line).
const DOD: Hosts<'static> = Hosts {
    service: "https://smba.infra.dod.teams.microsoft.us/teams",
    ..GCC_HIGH
};

impl Cloud {
    /// The hosts its bots sign in at and post through.
    #[must_use]
    pub const fn hosts(self) -> Hosts<'static> {
        match self {
            Self::Public => PUBLIC,
            Self::GccHigh => GCC_HIGH,
            Self::Dod => DOD,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_cloud_names_the_hosts_microsoft_documents_for_it() {
        assert_eq!(Cloud::default(), Cloud::Public);
        assert_eq!(Cloud::Public.hosts(), PUBLIC);
        assert_eq!(
            (
                Cloud::GccHigh.hosts().login,
                Cloud::GccHigh.hosts().tenant,
                Cloud::GccHigh.hosts().scope
            ),
            (
                "https://login.microsoftonline.us",
                "MicrosoftServices.onmicrosoft.us",
                "https://api.botframework.us"
            )
        );
        assert_eq!(
            Cloud::GccHigh.hosts().service,
            "https://smba.infra.gov.teams.microsoft.us/teams"
        );
        assert_eq!(
            (Cloud::GccHigh.hosts().openid, Cloud::GccHigh.hosts().issuer),
            (
                "https://login.botframework.azure.us/v1/.well-known/openidconfiguration",
                "https://api.botframework.us"
            )
        );
        assert_eq!(
            Cloud::Dod.hosts(),
            Hosts {
                service: "https://smba.infra.dod.teams.microsoft.us/teams",
                ..Cloud::GccHigh.hosts()
            }
        );
        for (written, cloud) in [
            ("public", Cloud::Public),
            ("gcc-high", Cloud::GccHigh),
            ("dod", Cloud::Dod),
        ] {
            assert_eq!(
                serde_json::from_str::<Cloud>(&format!("\"{written}\"")).unwrap(),
                cloud
            );
        }
    }
}
