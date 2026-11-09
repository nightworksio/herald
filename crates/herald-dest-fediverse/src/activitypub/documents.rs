use herald_core::{ACTIVITY_JSON, Announcement, Dialect, digest, public_pem, safe_url};
use serde_json::{Value, json};

use super::options::{ActorRouteOptions, Audience};

/// The `ActivityStreams` context.
const STREAMS: &str = "https://www.w3.org/ns/activitystreams";
/// The security context, which names `publicKey`.
const SECURITY: &str = "https://w3id.org/security/v1";
/// Everyone, as `ActivityStreams` addresses them.
const PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";

/// The id of the actor `actor` served at `origin`: its domain, and the path
/// herald is served under.
pub(crate) fn actor_id(origin: &str, actor: &str) -> String {
    format!("https://{origin}/users/{actor}")
}

/// The id of the key the actor `actor` signs with.
pub(crate) fn key_id(origin: &str, actor: &str) -> String {
    format!("{}#main-key", actor_id(origin, actor))
}

/// The id of the Note an announcement under `key` is.
pub(crate) fn note_id(origin: &str, actor: &str, key: &str) -> String {
    format!("{}/notes/{key}", actor_id(origin, actor))
}

/// The actor `actor` served at `origin`, as its document is served: a `Service`,
/// with its inbox, its server's shared inbox, and its public key as
/// `public` (a DER `SubjectPublicKeyInfo`) publishes it.
#[must_use]
pub fn actor_document(origin: &str, actor: &str, public: &[u8]) -> Value {
    let id = actor_id(origin, actor);
    json!({
        "@context": [STREAMS, SECURITY],
        "id": id,
        "type": "Service",
        "preferredUsername": actor,
        "name": actor,
        "url": id,
        "inbox": format!("{id}/inbox"),
        "outbox": outbox_id(origin, actor),
        "followers": followers(&id),
        "manuallyApprovesFollowers": false,
        "discoverable": true,
        "endpoints": { "sharedInbox": format!("https://{origin}/inbox") },
        "publicKey": {
            "id": key_id(origin, actor),
            "owner": id,
            "publicKeyPem": public_pem(public),
        },
    })
}

/// What WebFinger answers for `acct:<actor>@<domain>`, of the actor served
/// at `origin`.
#[must_use]
pub fn webfinger(domain: &str, origin: &str, actor: &str) -> Value {
    let id = actor_id(origin, actor);
    json!({
        "subject": format!("acct:{actor}@{domain}"),
        "aliases": [id],
        "links": [{ "rel": "self", "type": ACTIVITY_JSON, "href": id }],
    })
}

/// The HTML of `announcement`: its title in bold, every part, and its link.
fn content(announcement: &Announcement) -> String {
    let title = announcement.title.shown(Dialect::Matrix);
    let mut html = vec![format!(
        "<p><strong>{}</strong></p>",
        title.html.as_deref().unwrap_or_default().trim_end()
    )];
    html.extend(
        announcement
            .parts
            .iter()
            .filter_map(|part| part.html.clone())
            .filter(|part| !part.is_empty()),
    );
    if let Some(link) = &announcement.link {
        let href = safe_url(link);
        html.push(format!("<p><a href=\"{href}\">{href}</a></p>"));
    }
    html.join("\n")
}

/// Who an activity of the actor `id` is addressed to, and who is copied:
/// the public and its followers, the public copied where `audience` keeps
/// it off the public timelines.
fn addressed(id: &str, audience: Audience) -> (Value, Value) {
    match audience {
        Audience::Public => (json!([PUBLIC]), json!([followers(id)])),
        Audience::Unlisted => (json!([followers(id)]), json!([PUBLIC])),
    }
}

/// `announcement` as the actor `route` names, served at `origin`,
/// publishes it: a `Note` seen as `route` says, in its language and behind
/// its content warning where it gives them.
#[must_use]
pub fn note(origin: &str, announcement: &Announcement, route: &ActorRouteOptions) -> Value {
    let id = actor_id(origin, &route.actor);
    let (to, cc) = addressed(&id, route.visibility);
    let content = content(announcement);
    let mut note = json!({
        "id": note_id(origin, &route.actor, &announcement.key.to_string()),
        "type": "Note",
        "attributedTo": id,
        "published": announcement.occurred.to_string(),
        "to": to,
        "cc": cc,
        "content": content,
        "sensitive": route.content_warning.is_some(),
    });
    note.as_object_mut().into_iter().for_each(|fields| {
        if let Some(link) = &announcement.link {
            fields.insert("url".to_owned(), json!(safe_url(link)));
        }
        if let Some(warning) = &route.content_warning {
            fields.insert("summary".to_owned(), json!(warning));
        }
        if let Some(language) = &route.language {
            fields.insert("contentMap".to_owned(), json!({ language: content }));
        }
    });
    note
}

fn followers(actor: &str) -> String {
    format!("{actor}/followers")
}

/// The `Create` of `announcement`'s Note, as each follower's inbox takes it.
#[must_use]
pub fn create(origin: &str, announcement: &Announcement, route: &ActorRouteOptions) -> Value {
    created(&note(origin, announcement, route))
}

/// The `Create` of `note`, a Note as herald writes it: by its author, to
/// its audience, when it was published.
#[must_use]
pub(crate) fn created(note: &Value) -> Value {
    let field = |name: &str| note.get(name).cloned().unwrap_or(Value::Null);
    let id = note.get("id").and_then(Value::as_str).unwrap_or_default();
    json!({
        "@context": STREAMS,
        "id": format!("{id}/activity"),
        "type": "Create",
        "actor": field("attributedTo"),
        "published": field("published"),
        "to": field("to"),
        "cc": field("cc"),
        "object": note,
    })
}

/// The outbox of the actor `actor` served at `origin`, holding `total`
/// Notes: a collection that names its first page.
#[must_use]
pub(crate) fn outbox(origin: &str, actor: &str, total: usize) -> Value {
    let id = outbox_id(origin, actor);
    json!({
        "@context": STREAMS,
        "id": id,
        "type": "OrderedCollection",
        "totalItems": total,
        "first": format!("{id}?page={FIRST}"),
    })
}

/// The page `page` of that outbox: `notes`, newest first, each as its
/// `Create`, and the page after it where `next` names one.
#[must_use]
pub(crate) fn outbox_page(
    origin: &str,
    actor: &str,
    page: &str,
    notes: &[Value],
    next: Option<&str>,
) -> Value {
    let outbox = outbox_id(origin, actor);
    let mut shown = json!({
        "@context": STREAMS,
        "id": format!("{outbox}?page={page}"),
        "type": "OrderedCollectionPage",
        "partOf": outbox,
        "orderedItems": notes.iter().map(created).collect::<Vec<_>>(),
    });
    if let (Some(next), Some(fields)) = (next, shown.as_object_mut()) {
        fields.insert("next".to_owned(), json!(format!("{outbox}?page={next}")));
    }
    shown
}

/// The followers collection of the actor `actor` served at `origin`: how
/// many follow it, and never who.
#[must_use]
pub(crate) fn followers_collection(origin: &str, actor: &str, total: usize) -> Value {
    json!({
        "@context": STREAMS,
        "id": followers(&actor_id(origin, actor)),
        "type": "OrderedCollection",
        "totalItems": total,
    })
}

/// What an outbox's first page is asked for as.
pub const FIRST: &str = "true";

fn outbox_id(origin: &str, actor: &str) -> String {
    format!("{}/outbox", actor_id(origin, actor))
}

/// The `Accept` of `follow`, a `Follow` of the actor `actor` served at `origin`:
/// its id is the follow's own, digested, so a follow accepted twice is
/// accepted as one.
#[must_use]
pub fn accept(origin: &str, actor: &str, follow: &Value) -> Value {
    let id = actor_id(origin, actor);
    let followed = follow.get("id").and_then(Value::as_str).unwrap_or_default();
    json!({
        "@context": STREAMS,
        "id": format!("{id}/accepts/{}", digest(followed)),
        "type": "Accept",
        "actor": id,
        "object": follow,
    })
}

/// What an activity sent to herald's inbox asks of one of its actors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// A `Follow` of the actor `target`, as the follow itself.
    Follow {
        /// The id of the actor followed.
        target: String,
        /// The `Follow`, as an `Accept` answers it.
        follow: Value,
    },
    /// An `Undo` of `follower`'s follow of the actor `target`.
    Undo {
        /// The actor whose follow is undone.
        follower: String,
        /// The id of the actor no longer followed.
        target: String,
    },
    /// Nothing herald takes, and why.
    Nothing(String),
}

fn named(value: Option<&Value>) -> Option<&str> {
    value.and_then(|value| {
        value
            .as_str()
            .or_else(|| value.get("id").and_then(Value::as_str))
    })
}

fn nothing(why: &str) -> Asked {
    Asked::Nothing(why.to_owned())
}

/// What `body`, an activity whose actor is `sender`, asks: a follow, or the
/// undoing of `sender`'s own follow, carried whole.
#[must_use]
pub fn asked(body: &[u8], sender: &str) -> Asked {
    let Ok(activity) = serde_json::from_slice::<Value>(body) else {
        return nothing("it is not JSON");
    };
    match activity.get("type").and_then(Value::as_str) {
        Some("Follow") if activity.get("id").and_then(Value::as_str).is_none() => {
            nothing("it is a follow with no id")
        }
        Some("Follow") => match named(activity.get("object")) {
            Some(target) => Asked::Follow {
                target: target.to_owned(),
                follow: activity.clone(),
            },
            None => nothing("it follows no one"),
        },
        Some("Undo") => {
            let Some(follow) = activity
                .get("object")
                .filter(|follow| follow.get("type").and_then(Value::as_str) == Some("Follow"))
            else {
                return nothing("it undoes no follow it carries");
            };
            if named(follow.get("actor")) != Some(sender) {
                return nothing("it undoes another actor's follow");
            }
            match named(follow.get("object")) {
                Some(target) => Asked::Undo {
                    follower: sender.to_owned(),
                    target: target.to_owned(),
                },
                None => nothing("it undoes a follow of no one"),
            }
        }
        Some(kind) => Asked::Nothing(format!("herald takes no {kind}")),
        None => nothing("it names no type"),
    }
}
