//! What Teams posts to a bot at herald's endpoint: each activity taken
//! only on the bearer token the Bot Framework vouches for it with, verified
//! before its body is parsed, and then only where the service URL it names
//! is the one its token names and lies on the bot's own service origin;
//! the commands members give the bot in its messages, and herald's answers,
//! each a reply carrying the bot's token to that service URL alone
//! (ADR-0007, ADR-0016, ADR-0093).

use herald_core::{
    Answering, Asked, Command, DestinationName, Markup, Place, Room, Text, Waits, Worded, digest,
    segment, worded,
};
use herald_kinds::{Interacts, Post, Read, Replying, Said, Vouching};
use herald_ports::{Base, Boxed, DynHttp, Method, Request, Response, Secret, SecretText};
use jiff::{SignedDuration, Timestamp};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::bot::{BOT_MESSAGE, Bot};
use crate::card::{Measured, answer};
use crate::cloud::Hosts;
use crate::interactions::Interactions;
use crate::options::{RouteOptions, base_url, guid};
use crate::request::JSON;
use crate::teams::{MARKUP, TEAMS, bot_profile};

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

/// The activity types a bot takes, each answered `200 OK` at once and a
/// command a `message` gives worked after, of those the Bot Framework's
/// activity schema names
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

/// The type of an activity that carries a member's message.
const MESSAGE: &str = "message";

/// The type of an activity that asks the bot for an answer at once.
const INVOKE: &str = "invoke";

/// The name of the invoke a card's `Action.Execute` makes
/// (<https://learn.microsoft.com/en-us/adaptive-cards/authoring-cards/universal-action-model>,
/// "adaptiveCard/action Invoke activity").
const CARD_ACTION: &str = "adaptiveCard/action";

/// The type of the action herald's subscribe button is, as a press of it
/// names it.
pub(crate) const EXECUTE: &str = "Action.Execute";

/// The trigger of an action a member pressed, where the invoke names one.
const MANUAL: &str = "manual";

/// The verb of herald's subscribe button: a press of it subscribes the
/// member, or unsubscribes one already subscribed.
pub const SUBSCRIBE: &str = "herald_subscribe";

/// The most bytes of a project's name a press of the button takes.
const PRESSED_PROJECT: usize = 256;

/// The type of an invoke's answer that Teams shows the member as a message
/// ("Response format").
const SHOWN: &str = "application/vnd.microsoft.activity.message";

/// The type of an invoke's answer that says the press was not taken.
const ERROR: &str = "application/vnd.microsoft.error";

/// What a press herald cannot act on is answered with.
const UNPRESSED: &str =
    "herald cannot act on this press: Microsoft Teams named no member or message herald can take";

/// The type of an entity that mentions an account.
const MENTION: &str = "mention";

/// The type of a conversation between a member and the bot alone.
const PERSONAL: &str = "personal";

/// How a channel's conversation names a thread in it, before the id of the
/// thread's first message.
const THREAD: &str = ";messageid=";

/// The most bytes of a message's text herald reads a command from.
const SPOKEN: usize = 1_024;

/// Why an answer to a command is not sent where it goes off the bot's
/// service origin.
const UNSERVED: &str = "an answer to a command goes off the bot's service origin, so it carries no token and is not sent";

/// Why an activity is refused where no token vouched for it.
const UNVOUCHED: &str = "no token vouched for the activity";
/// Why an activity naming a service URL its token does not is refused.
const ELSEWHERE: &str = "the activity names a service URL its token does not vouch for";
/// Why an activity naming a service URL off the bot's service origin is
/// refused.
const OFF_ORIGIN: &str =
    "the activity names a service URL that is not an https URL on the bot's service origin";
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
    from: Option<Account>,
    recipient: Option<Account>,
    text: Option<String>,
    entities: Option<Vec<Entity>>,
    name: Option<String>,
    reply_to_id: Option<String>,
    value: Option<Value>,
}

/// As much of an activity's conversation as herald reads.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Conversation {
    id: Option<String>,
    conversation_type: Option<String>,
}

/// As much of an account as herald reads: its id in the channel, and its
/// Microsoft Entra object id.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    id: Option<String>,
    aad_object_id: Option<String>,
}

/// As much of an entity of a message as herald reads.
#[derive(Deserialize)]
struct Entity {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
    mentioned: Option<Account>,
}

/// Where herald's answer to a command goes: a reply to the member's
/// message, through the service URL its verified activity named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The service URL, with no trailing `/`.
    pub service: String,
    /// The conversation the message was given in.
    pub conversation: String,
    /// The message's activity id.
    pub activity: String,
}

/// A Teams bot taking what Teams posts to it, and signing in to answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interacting {
    service: String,
    origin: String,
    vouching: Vouching,
    bot: Bot,
    waits: Waits,
    answering: Answering,
}

impl Interacting {
    /// The bot `app_id`, signing in with its client secret `password` as
    /// `hosts` say to answer, its activities vouched for as `hosts` say and
    /// taken where they name a service URL on the origin of `hosts`'
    /// service; none where that service is not an `https` URL of a host.
    #[must_use]
    pub fn new(app_id: &str, password: Secret, hosts: Hosts<'_>) -> Option<Self> {
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
            bot: Bot::new(app_id, password, hosts, &RouteOptions::default()),
            waits: Waits::DEFAULT,
            answering: Interactions::default().answering(),
        })
    }

    /// It, answering commands as `answering` says.
    #[must_use]
    pub fn taking(self, answering: Answering) -> Self {
        Self { answering, ..self }
    }

    /// It, waiting `waits` to sign in again where Microsoft's sign-in
    /// service names no wait.
    #[must_use]
    pub fn waiting(self, waits: Waits) -> Self {
        Self { waits, ..self }
    }

    /// What an activity `post` carries says to `destination`, its token's
    /// claims verified.
    fn activity(&self, destination: &DestinationName, post: Post<'_>) -> Result<Read, Read> {
        let vouched = post.vouched.ok_or(Read::Unvouched(UNVOUCHED.to_owned()))?;
        let activity = serde_json::from_slice::<Activity>(post.body)
            .map_err(|_| Read::Refused(UNREAD.to_owned()))?;
        let named = activity.service_url.as_deref();
        if named.is_none() || named != vouched.claim(SERVICE_URL) {
            return Err(Read::Unvouched(ELSEWHERE.to_owned()));
        }
        let Some(service) = named
            .filter(|named| base_url(named))
            .filter(|named| origin(named).as_deref() == Some(self.origin.as_str()))
        else {
            return Err(Read::Unvouched(OFF_ORIGIN.to_owned()));
        };
        if activity.channel_id.as_deref() != Some(CHANNEL) {
            return Err(Read::Refused(CHANNELLED.to_owned()));
        }
        let conversation = activity
            .conversation
            .as_ref()
            .and_then(|conversation| conversation.id.as_deref());
        let (Some(id), Some(conversation)) = (
            activity.id.as_deref().filter(|id| !id.is_empty()),
            conversation.filter(|conversation| !conversation.is_empty()),
        ) else {
            return Err(Read::Refused(UNNAMED.to_owned()));
        };
        let kind = activity.kind.as_deref();
        let service = service.trim_end_matches('/');
        let (asked, answer) = if kind == Some(INVOKE) {
            let project = pressed(&activity).ok_or(Read::Refused(UNTAKEN.to_owned()))?;
            let asked = activity.reply_to_id.as_deref().and_then(|message| {
                asked(
                    destination,
                    &activity,
                    Reply {
                        service: service.to_owned(),
                        conversation: conversation.to_owned(),
                        activity: message.to_owned(),
                    },
                    Some(project),
                )
            });
            let answer = match asked {
                Some(_) => json!({
                    "statusCode": 200,
                    "type": SHOWN,
                    "value": self.answering.wording.pressed(),
                }),
                None => json!({
                    "statusCode": 400,
                    "type": ERROR,
                    "value": { "code": "BadRequest", "message": UNPRESSED },
                }),
            };
            (asked, Said::ok(answer.to_string()))
        } else if kind.is_some_and(|kind| ACKNOWLEDGED.contains(&kind)) {
            let reply = Reply {
                service: service.to_owned(),
                conversation: conversation.to_owned(),
                activity: id.to_owned(),
            };
            let asked = (kind == Some(MESSAGE))
                .then(|| asked(destination, &activity, reply, None))
                .flatten();
            (asked, Said::ok(TAKEN))
        } else {
            return Err(Read::Refused(UNTAKEN.to_owned()));
        };
        let then = match asked {
            Some(asked) => Read::Asked {
                asked: vec![asked],
                answer: answer.clone(),
            },
            None => Read::Said(answer.clone()),
        };
        Ok(Read::Delivered {
            delivery: digest(&format!("{conversation}\n{id}")),
            replayed: answer,
            then: Box::new(then),
        })
    }
}

/// The project a press of herald's subscribe button names, where the
/// invoke `activity` is one: the card action a member pressed, of herald's
/// verb, naming a project of at most [`PRESSED_PROJECT`] bytes with no
/// control character.
fn pressed(activity: &Activity) -> Option<String> {
    if activity.name.as_deref() != Some(CARD_ACTION) {
        return None;
    }
    let value = activity.value.as_ref()?;
    let trigger = value.get("trigger");
    if trigger.is_some_and(|trigger| trigger.as_str() != Some(MANUAL)) {
        return None;
    }
    let action = value.get("action")?;
    let field = |name: &str| action.get(name).and_then(Value::as_str);
    if field("type") != Some(EXECUTE) || field("verb") != Some(SUBSCRIBE) {
        return None;
    }
    action
        .get("data")?
        .get("project")?
        .as_str()
        .filter(|project| {
            (1..=PRESSED_PROJECT).contains(&project.len())
                && !project.chars().any(char::is_control)
                && project.trim() == *project
        })
        .map(ToOwned::to_owned)
}

/// The command a member gave the bot in the message `activity`, or the
/// subscribe button they pressed for `pressed`, to `destination`, as the
/// member its sender names by Microsoft Entra object id, with its answer
/// going as `reply` says; none where it gives none, was not addressed to
/// the bot, or names no such member.
fn asked(
    destination: &DestinationName,
    activity: &Activity,
    reply: Reply,
    pressed: Option<String>,
) -> Option<(Asked, Replying)> {
    let bot = activity.recipient.as_ref()?.id.as_deref()?;
    let from = activity.from.as_ref()?;
    if from.id.as_deref() == Some(bot) {
        return None;
    }
    let member = from
        .aad_object_id
        .as_deref()
        .filter(|member| guid(member))?
        .to_ascii_lowercase();
    if reply.activity.is_empty() {
        return None;
    }
    let toggles = pressed.is_some();
    let Worded {
        command,
        project,
        share,
    } = pressed.map_or_else(
        || given(activity, bot),
        |project| {
            Some(Worded {
                command: Command::Subscribe,
                project: Some(project),
                share: false,
            })
        },
    )?;
    let place = match reply.conversation.split_once(THREAD) {
        Some((channel, _)) if !channel.is_empty() => Place {
            id: reply.conversation.clone(),
            parent: Some(channel.to_owned()),
        },
        _ => Place::new(reply.conversation.clone()),
    };
    Some((
        Asked {
            destination: destination.clone(),
            place,
            user: member,
            command,
            project,
            share,
            toggles,
            member: None,
        },
        Replying::new(reply),
    ))
}

/// The command the message `activity` gives `bot`, where it gives one.
fn given(activity: &Activity, bot: &str) -> Option<Worded> {
    let entities = activity.entities.as_deref().unwrap_or_default();
    let personal = activity
        .conversation
        .as_ref()
        .and_then(|conversation| conversation.conversation_type.as_deref())
        == Some(PERSONAL);
    let spoken = spoken(activity.text.as_deref()?, bot, entities, personal)?;
    worded(spoken.split_whitespace())
}

/// `text` as a command speaks it to `bot`, every mention of the bot left
/// out: where it fits [`SPOKEN`], and it mentions the bot or is given in a
/// conversation with the bot alone.
fn spoken(text: &str, bot: &str, entities: &[Entity], personal: bool) -> Option<String> {
    if text.len() > SPOKEN {
        return None;
    }
    let mentions: Vec<&str> = entities
        .iter()
        .filter(|entity| entity.kind.as_deref() == Some(MENTION))
        .filter(|entity| {
            entity
                .mentioned
                .as_ref()
                .and_then(|mentioned| mentioned.id.as_deref())
                == Some(bot)
        })
        .filter_map(|entity| entity.text.as_deref())
        .filter(|mention| !mention.is_empty() && text.contains(mention))
        .collect();
    if mentions.is_empty() && !personal {
        return None;
    }
    Some(
        mentions
            .into_iter()
            .fold(text.to_owned(), |spoken, mention| {
                spoken.replace(mention, " ")
            }),
    )
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

    fn reaches(&self) -> Base<'_> {
        Base::Among(self.bot.bases())
    }

    fn authorised<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        request: Request,
        now: Timestamp,
    ) -> Boxed<'a, Result<Request, String>> {
        Box::pin(async move {
            if origin(&request.url.expose()).as_deref() != Some(self.origin.as_str()) {
                return Err(UNSERVED.to_owned());
            }
            let token = self.bot.answering(http, now, self.waits).await?;
            Ok(request.bearer(token))
        })
    }

    fn answering(&self) -> Answering {
        self.answering.clone()
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

    fn answered(&self, replying: &Replying, parts: &[Text], _: bool) -> Vec<Request> {
        let Some(reply) = replying.of::<Reply>() else {
            return Vec::new();
        };
        let (Some(conversation), Some(activity)) =
            (segment(&reply.conversation), segment(&reply.activity))
        else {
            return Vec::new();
        };
        let url = format!(
            "{}/v3/conversations/{conversation}/activities/{activity}",
            reply.service
        );
        parts
            .iter()
            .filter_map(|part| {
                Measured::Utf16(BOT_MESSAGE)
                    .fitted(answer(&part.body).to_string())
                    .ok()
            })
            .map(|body| {
                Request::new(Method::Post, SecretText::plain(url.clone()))
                    .identified()
                    .header("Content-Type", SecretText::plain(JSON))
                    .body(body)
            })
            .collect()
    }

    fn accepted(&self, response: &Response) -> Result<(), String> {
        if response.succeeded() {
            return Ok(());
        }
        Err(match coded(response) {
            Some(code) => format!("{TEAMS} answered {}: {code}", response.status),
            None => format!("{TEAMS} answered {}", response.status),
        })
    }

    fn vouching(&self) -> Option<&Vouching> {
        Some(&self.vouching)
    }

    fn read(&self, destination: &DestinationName, post: Post<'_>) -> Read {
        if !post.path.is_empty() {
            return Read::Missing("such path");
        }
        if post.method != "POST" {
            return Read::Unmethodical;
        }
        self.activity(destination, post)
            .unwrap_or_else(|refused| refused)
    }
}

/// The code of the Bot Framework's error envelope `response` holds, where
/// it is a name of letters and digits alone, so no text of the request it
/// answers is said back.
fn coded(response: &Response) -> Option<String> {
    const MOST: usize = 64;
    serde_json::from_slice::<serde_json::Value>(&response.body)
        .ok()?
        .get("error")?
        .get("code")?
        .as_str()
        .filter(|code| {
            (1..=MOST).contains(&code.len())
                && code.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .map(ToOwned::to_owned)
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
