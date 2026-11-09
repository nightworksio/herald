use schemars::JsonSchema;
use serde::Deserialize;

/// The Slack a workspace lives on: Slack's own, or Slack's government cloud.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
pub enum Domain {
    /// Slack's own, at `slack.com`.
    #[default]
    #[serde(rename = "slack.com")]
    Slack,
    /// Slack's government cloud, at `slack-gov.com`.
    #[serde(rename = "slack-gov.com")]
    Gov,
}

impl Domain {
    /// The domain its hosts lie under, Socket Mode's among them.
    #[must_use]
    pub const fn host(self) -> &'static str {
        match self {
            Self::Slack => "slack.com",
            Self::Gov => "slack-gov.com",
        }
    }

    /// The base URL of its Web API.
    #[must_use]
    pub fn api(self) -> String {
        format!("https://{}/api", self.host())
    }

    /// The base URL of its incoming webhooks and response URLs.
    #[must_use]
    pub fn hooks(self) -> String {
        format!("https://hooks.{}", self.host())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_slack_names_its_api_hooks_and_session_domain() {
        assert_eq!(Domain::default(), Domain::Slack);
        assert_eq!(
            (
                Domain::Slack.api(),
                Domain::Slack.hooks(),
                Domain::Slack.host()
            ),
            (
                "https://slack.com/api".to_owned(),
                "https://hooks.slack.com".to_owned(),
                "slack.com"
            )
        );
        assert_eq!(
            (Domain::Gov.api(), Domain::Gov.hooks(), Domain::Gov.host()),
            (
                "https://slack-gov.com/api".to_owned(),
                "https://hooks.slack-gov.com".to_owned(),
                "slack-gov.com"
            )
        );
        assert_eq!(
            serde_json::from_str::<Domain>("\"slack-gov.com\"").unwrap(),
            Domain::Gov
        );
    }
}
