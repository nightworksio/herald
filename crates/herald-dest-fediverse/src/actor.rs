use std::fmt;

use herald_ports::{Method, Request, SecretText};
use serde_json::Value;
use url::{Host, Url};

/// What an ActivityPub server is asked for an actor's document in.
const ACTIVITY_JSON: &str = "application/activity+json";
/// The profile of JSON-LD that is `ActivityStreams`.
const STREAMS: &str = "https://www.w3.org/ns/activitystreams";
/// The types of the actors whose documents list their keys.
const ACTORS: [&str; 5] = ["Person", "Service", "Application", "Group", "Organization"];

/// The public key an ActivityPub signature names by its `keyId`, and the
/// actor that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorKey {
    /// The id of the actor the key belongs to.
    pub actor: String,
    /// The key, as a PEM `PUBLIC KEY`.
    pub pem: String,
}

/// What a key's document says of the key a signature names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOwner {
    /// The actor's own document lists the key, so the actor holds it.
    Held(ActorKey),
    /// The key's own document names its owner, whose document must list the
    /// key before the owner is taken to hold it.
    Claimed {
        /// The actor the key's document names as its owner.
        owner: String,
        /// The key, as a PEM `PUBLIC KEY`.
        pem: String,
    },
}

/// Why a document names no key that `keyId` may be checked with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorError {
    /// The document is not a JSON object.
    Unreadable,
    /// The document holds no key with that id and a PEM, or a key's own
    /// document names no owner.
    NoKey,
    /// The document is not the one it was read from, or names another
    /// actor as the key's owner.
    Misnamed,
    /// The owner a key's own document names does not list the key.
    Unconfirmed,
}

impl fmt::Display for ActorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable => f.write_str("the actor's document is not a JSON object"),
            Self::NoKey => {
                f.write_str("the actor's document holds no key under the signature's keyId")
            }
            Self::Misnamed => f.write_str(
                "the document is not the one it was read from, or names another actor as the key's owner",
            ),
            Self::Unconfirmed => {
                f.write_str("the actor the key names as its owner does not list the key")
            }
        }
    }
}

/// The document a `keyId` is read from: the id without its fragment.
#[must_use]
pub fn document(key_id: &str) -> &str {
    key_id.split('#').next().unwrap_or(key_id)
}

/// The request for the document of `key_id`, where it is written as
/// [`origin`] reads it, reading at most `most` bytes of it. The `Http` it is
/// sent through reaches only public addresses (ADR-0007).
#[must_use]
pub fn key_request(key_id: &str, most: usize) -> Option<Request> {
    origin(key_id)?;
    Some(
        Request::new(Method::Get, SecretText::plain(document(key_id)))
            .header("Accept", SecretText::plain(ACTIVITY_JSON))
            .identified()
            .at_most(most),
    )
}

/// The origin of `id`, `https://` and its host, where `id` is an HTTPS URL
/// already written as the URL parser reads it, naming a host by its name,
/// with no port, no user and no empty label: so the id herald compares is
/// the URL it fetches.
#[must_use]
pub fn origin(id: &str) -> Option<String> {
    let url = Url::parse(id).ok()?;
    let Some(Host::Domain(host)) = url.host() else {
        return None;
    };
    let sound = url.scheme() == "https"
        && url.as_str() == id
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && host.split('.').all(|label| !label.is_empty());
    sound.then(|| format!("https://{host}"))
}

/// Whether `one` and `other` are on one origin, both written as [`origin`]
/// reads them.
#[must_use]
pub fn same_origin(one: &str, other: &str) -> bool {
    origin(one).is_some_and(|origin| Some(origin) == self::origin(other))
}

/// Whether a document is served as `ActivityPub` JSON, by the
/// `Content-Type` it was answered with.
#[must_use]
pub fn activity_json(content_type: Option<&str>) -> bool {
    let Some((media, parameters)) =
        content_type.map(|value| value.split_once(';').unwrap_or((value, "")))
    else {
        return false;
    };
    match media.trim().to_ascii_lowercase().as_str() {
        "application/activity+json" => true,
        "application/ld+json" => parameters.split(';').any(|parameter| {
            parameter
                .trim()
                .strip_prefix("profile=")
                .is_some_and(|profile| {
                    profile
                        .trim_matches('"')
                        .split(' ')
                        .any(|named| named == STREAMS)
                })
        }),
        _ => false,
    }
}

fn id(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("id").and_then(Value::as_str))
}

fn object(document: &[u8]) -> Result<Value, ActorError> {
    serde_json::from_slice(document)
        .ok()
        .filter(Value::is_object)
        .ok_or(ActorError::Unreadable)
}

/// The keys `actor` lists under `publicKey`, where it lists any.
fn listed(actor: &Value) -> Vec<&Value> {
    match actor.get("publicKey") {
        Some(Value::Array(keys)) => keys.iter().collect(),
        Some(key) => vec![key],
        None => Vec::new(),
    }
}

fn pem(key: &Value) -> Result<String, ActorError> {
    key.get("publicKeyPem")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or(ActorError::NoKey)
}

/// The key `key_id` names in `document`, the document its `keyId` names
/// and its id must be: the actor's own document, listing its keys under
/// `publicKey`, or the key's own document naming its owner, which
/// [`confirms`] must then hold to.
///
/// # Errors
///
/// When the document is not an object, holds no such key, is not the one
/// it was read from, or names another owner.
pub fn actor_key(document: &[u8], key_id: &str) -> Result<KeyOwner, ActorError> {
    let value = object(document)?;
    if origin(key_id).is_none() || id(&value) != Some(self::document(key_id)) {
        return Err(ActorError::Misnamed);
    }
    if value.get("publicKey").is_none() {
        let owner = value.get("owner").and_then(id).ok_or(ActorError::NoKey)?;
        if !same_origin(owner, key_id) {
            return Err(ActorError::Misnamed);
        }
        return Ok(KeyOwner::Claimed {
            owner: owner.to_owned(),
            pem: pem(&value)?,
        });
    }
    if !acting(&value) {
        return Err(ActorError::Misnamed);
    }
    let actor = self::document(key_id);
    let key = listed(&value)
        .into_iter()
        .find(|key| id(key) == Some(key_id))
        .ok_or(ActorError::NoKey)?;
    if key
        .get("owner")
        .and_then(id)
        .is_some_and(|owner| owner != actor)
    {
        return Err(ActorError::Misnamed);
    }
    Ok(KeyOwner::Held(ActorKey {
        actor: actor.to_owned(),
        pem: pem(key)?,
    }))
}

/// Whether `value` is an actor, by its `type`.
fn acting(value: &Value) -> bool {
    value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| ACTORS.contains(&kind))
}

/// Whether `document` is the actor `owner`'s own, listing `key_id` among
/// its keys, so the owner a key's own document names holds the key.
#[must_use]
pub fn confirms(document: &[u8], owner: &str, key_id: &str) -> bool {
    object(document).is_ok_and(|actor| {
        id(&actor) == Some(owner)
            && acting(&actor)
            && listed(&actor).iter().any(|key| {
                id(key) == Some(key_id)
                    && key
                        .get("owner")
                        .and_then(id)
                        .is_none_or(|named| named == owner)
            })
    })
}

/// The actor an activity names, and its own id where it has one, each a
/// string, read only from a body whose signature held. An activity whose
/// actor or id is anything but a string names none.
#[must_use]
pub fn actor_of(activity: &[u8]) -> Option<(String, Option<String>)> {
    let value: Value = serde_json::from_slice(activity).ok()?;
    let actor = value.get("actor").and_then(Value::as_str)?;
    let own = match value.get("id") {
        None => None,
        Some(own) => Some(own.as_str()?.to_owned()),
    };
    Some((actor.to_owned(), own))
}

/// Where a follower is delivered to, as its own document says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inboxes {
    /// Its own inbox, which an `Accept` of its follow goes to.
    pub personal: String,
    /// Its server's shared inbox, where its server has one.
    pub shared: Option<String>,
}

impl Inboxes {
    /// Where announcements are delivered to it: its server's shared inbox,
    /// or else its own.
    #[must_use]
    pub fn delivered(&self) -> &str {
        self.shared.as_deref().unwrap_or(&self.personal)
    }
}

/// The inboxes of the actor `follower`, read from `document`, its own: each
/// on the follower's own origin, so no actor can have herald deliver
/// elsewhere. None where the document is not the follower's, or names no
/// inbox of its own origin.
#[must_use]
pub fn inboxes(document: &[u8], follower: &str) -> Option<Inboxes> {
    let actor = object(document).ok()?;
    if id(&actor) != Some(follower) || !acting(&actor) {
        return None;
    }
    let own = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|inbox| same_origin(inbox, follower))
            .map(ToOwned::to_owned)
    };
    Some(Inboxes {
        personal: own(actor.get("inbox"))?,
        shared: own(actor
            .get("endpoints")
            .and_then(|endpoints| endpoints.get("sharedInbox"))),
    })
}
