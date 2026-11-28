//! What Teams posts to a bot at herald's endpoint: each activity taken
//! only on the bearer token the Bot Framework vouches for it with, verified
//! before its body is parsed, and then only where the service URL it names
//! is the one its token names and lies on the bot's own service origin
//! (ADR-0007, ADR-0093).

use herald_core::{Answering, DestinationName, Markup, Replies, Room, Text, Wording, digest};
use herald_kinds::{Interacts, Post, Read, Replying, Said, Vouching};
use herald_ports::{Request, Response};
use jiff::SignedDuration;
use serde::Deserialize;
use url::Url;

use crate::cloud::Hosts;
use crate::teams::{MARKUP, bot_profile};

/// The channel id of Microsoft Teams, as its activities name it
/// (<https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/conversations/conversation-messages>):
/// each key signing a token must be endorsed for it and each activity must
/// name it
/// (<https://learn.microsoft.com/en-us/azure/bot-service/rest-api/bot-framework-rest-connector-authentication?view=azure-bot-service-4.0>,
/// "Step 3" and "Step 4").
pub const CHANNEL: &str = "msteams";

/// The claim naming the service URL a token vouches for.
const SERVICE_URL: &str = "serviceUrl";

/// How far a token's times may lie from herald's clock: the five minutes
/// the authentication page names as the industry's clock skew ("Step 4").
const SKEW: SignedDuration = SignedDuration::from_mins(5);

/// The most bytes an activity's body holds.
pub const ACTIVITY: usize = 256 * 1024;

/// The most bytes the metadata document and the key set are each read
/// from.
const KEYS_MOST: usize = 4 * 1024 * 1024;

/// How long the keys are used once read: the day within which every bot
/// is to read them again ("Step 3").
const KEYS_FOR: SignedDuration = SignedDuration::from_hours(24);

/// How long after the keys were last read a token naming a key they lack
/// has them read again.
const KEYS_RETRY: SignedDuration = SignedDuration::from_mins(5);

/// The namespace the activities a bot takes are admitted once under.
const ADMITTED: &str = "teams";

/// The activity types a bot takes and answers with nothing more than
/// `200 OK`, of those the Bot Framework's activity schema names
/// (<https://github.com/microsoft/botframework-sdk/blob/main/specs/botframework-activity/botframework-activity.md>).
const ACKNOWLEDGED: [&str; 8] = [
    "message",
    "conversationUpdate",
    "installationUpdate",
    "messageReaction",
    "messageUpdate",
    "messageDelete",
    "typing",
    "endOfConversation",
];

/// What herald answers an activity it takes.
const TAKEN: &str = "{}";

/// Why an activity is refused where no token vouched for it.
const UNVOUCHED: &str = "no token vouched for the activity";
/// Why an activity naming a service URL its token does not is refused.
const ELSEWHERE: &str = "the activity names a service URL its token does not vouch for";
/// Why an activity naming a service URL off the bot's service origin is
/// refused.
const OFF_ORIGIN: &str = "the activity names a service URL that is not on the bot's service origin";
/// Why a body that is not an activity is refused.
const UNREAD: &str = "the request is not a Bot Framework activity";
/// Why an activity of another channel is refused.
const CHANNELLED: &str = "the activity is not from Microsoft Teams";
/// Why an activity naming no id or conversation is refused.
const UNNAMED: &str = "the activity names no id or conversation";
/// Why an activity of a type the bot does not take is refused.
const UNTAKEN: &str = "the bot takes no activity of this type";

/// As much of an activity as herald reads.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Activity {
    #[serde(rename = "type")]
    kind: Option<String>,
    id: Option<String>,
    service_url: Option<String>,
    channel_id: Option<String>,
    conversation: Option<Conversation>,
}

/// As much of an activity's conversation as herald reads.
#[derive(Deserialize)]
struct Conversation {
    id: Option<String>,
}

/// A Teams bot taking what Teams posts to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interacting {
    service: String,
    origin: String,
    vouching: Vouching,
}

impl Interacting {
    /// The bot `app_id`, its activities vouched for as `hosts` say and
    /// taken where they name a service URL on the origin of `hosts`'
    /// service; none where that service is not an `https` URL of a host.
    #[must_use]
    pub fn new(app_id: &str, hosts: Hosts<'_>) -> Option<Self> {
        Some(Self {
            service: hosts.service.to_owned(),
            origin: origin(hosts.service)?,
            vouching: Vouching {
                name: ADMITTED,
                metadata: hosts.openid.to_owned(),
                issuer: hosts.issuer.to_owned(),
                audience: app_id.to_owned(),
                skew: SKEW,
                endorsement: CHANNEL,
                claims: &[SERVICE_URL],
                body: ACTIVITY,
                keys_most: KEYS_MOST,
                keys_for: KEYS_FOR,
                keys_retry: KEYS_RETRY,
            },
        })
    }

    /// What an activity `post` carries says, its token's claims verified.
    fn activity(&self, post: Post<'_>) -> Result<Read, Read> {
        let vouched = post.vouched.ok_or(Read::Unvouched(UNVOUCHED.to_owned()))?;
        let activity = serde_json::from_slice::<Activity>(post.body)
            .map_err(|_| Read::Refused(UNREAD.to_owned()))?;
        let named = activity.service_url.as_deref();
        if named.is_none() || named != vouched.claim(SERVICE_URL) {
            return Err(Read::Unvouched(ELSEWHERE.to_owned()));
        }
        if named.and_then(origin).as_deref() != Some(self.origin.as_str()) {
            return Err(Read::Unvouched(OFF_ORIGIN.to_owned()));
        }
        if activity.channel_id.as_deref() != Some(CHANNEL) {
            return Err(Read::Refused(CHANNELLED.to_owned()));
        }
        let conversation = activity
            .conversation
            .and_then(|conversation| conversation.id);
        let (Some(id), Some(conversation)) = (
            activity.id.filter(|id| !id.is_empty()),
            conversation.filter(|conversation| !conversation.is_empty()),
        ) else {
            return Err(Read::Refused(UNNAMED.to_owned()));
        };
        if !activity
            .kind
            .is_some_and(|kind| ACKNOWLEDGED.contains(&kind.as_str()))
        {
            return Err(Read::Refused(UNTAKEN.to_owned()));
        }
        Ok(Read::Delivered {
            delivery: digest(&format!("{conversation}\n{id}")),
            replayed: Said::ok(TAKEN),
            then: Box::new(Read::Said(Said::ok(TAKEN))),
        })
    }
}

/// The origin of `url`, where it is an `https` URL of a host.
fn origin(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .filter(|url| url.scheme() == "https" && url.host_str().is_some())
        .map(|url| url.origin().ascii_serialization())
}

impl Interacts for Interacting {
    fn api(&self) -> &str {
        &self.service
    }

    fn answering(&self) -> Answering {
        Answering {
            commands: Vec::new(),
            replies: Replies::default(),
            prereleases: false,
            template: None,
            wording: Wording::of(None),
        }
    }

    fn room(&self) -> (Markup, Room) {
        let profile = bot_profile(false);
        (
            MARKUP,
            Room {
                measure: profile.measure,
                first: profile.limits.text,
                rest: profile.limits.followup,
            },
        )
    }

    fn answered(&self, _: &Replying, _: &[Text], _: bool) -> Vec<Request> {
        Vec::new()
    }

    fn accepted(&self, response: &Response) -> Result<(), String> {
        if response.succeeded() {
            Ok(())
        } else {
            Err(format!("Microsoft Teams answered {}", response.status))
        }
    }

    fn vouching(&self) -> Option<&Vouching> {
        Some(&self.vouching)
    }

    fn read(&self, _: &DestinationName, post: Post<'_>) -> Read {
        if !post.path.is_empty() {
            return Read::Missing("such path");
        }
        if post.method != "POST" {
            return Read::Unmethodical;
        }
        self.activity(post).unwrap_or_else(|refused| refused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_origin_is_an_https_url_s_alone() {
        assert_eq!(
            origin("https://smba.trafficmanager.net/emea/"),
            Some("https://smba.trafficmanager.net".to_owned())
        );
        for unorigined in [
            "http://smba.trafficmanager.net/",
            "smba",
            "data:text/plain,x",
        ] {
            assert_eq!(origin(unorigined), None, "{unorigined}");
        }
    }
}
