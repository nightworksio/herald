use herald_core::{ACTIVITY_JSON, Announcement, Dialect, literal, public_pem, safe_url};
use serde_json::{Value, json};

/// The `ActivityStreams` context.
const STREAMS: &str = "https://www.w3.org/ns/activitystreams";
/// The security context, which names `publicKey`.
const SECURITY: &str = "https://w3id.org/security/v1";
/// Everyone, as `ActivityStreams` addresses them.
const PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";

/// The id of the actor `actor` on `domain`.
pub(crate) fn actor_id(domain: &str, actor: &str) -> String {
    format!("https://{domain}/users/{actor}")
}

/// The id of the key the actor `actor` signs with.
pub(crate) fn key_id(domain: &str, actor: &str) -> String {
    format!("{}#main-key", actor_id(domain, actor))
}

/// The id of the Note an announcement under `key` is.
pub(crate) fn note_id(domain: &str, actor: &str, key: &str) -> String {
    format!("{}/notes/{key}", actor_id(domain, actor))
}

/// The actor `actor` on `domain`, as its document is served: a `Service`,
/// with its inbox, its server's shared inbox, and its public key as
/// `public` (a DER `SubjectPublicKeyInfo`) publishes it.
#[must_use]
pub fn actor_document(domain: &str, actor: &str, public: &[u8]) -> Value {
    let id = actor_id(domain, actor);
    json!({
        "@context": [STREAMS, SECURITY],
        "id": id,
        "type": "Service",
        "preferredUsername": actor,
        "name": actor,
        "url": id,
        "inbox": format!("{id}/inbox"),
        "outbox": format!("{id}/outbox"),
        "followers": followers(&id),
        "manuallyApprovesFollowers": false,
        "discoverable": true,
        "endpoints": { "sharedInbox": format!("https://{domain}/inbox") },
        "publicKey": {
            "id": key_id(domain, actor),
            "owner": id,
            "publicKeyPem": public_pem(public),
        },
    })
}

/// What WebFinger answers for `acct:<actor>@<domain>`.
#[must_use]
pub fn webfinger(domain: &str, actor: &str) -> Value {
    let id = actor_id(domain, actor);
    json!({
        "subject": format!("acct:{actor}@{domain}"),
        "aliases": [id],
        "links": [{ "rel": "self", "type": ACTIVITY_JSON, "href": id }],
    })
}

/// The HTML of `announcement`: its title in bold, every part, and its link.
fn content(announcement: &Announcement) -> String {
    let title = literal(&announcement.title, Dialect::Matrix);
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

/// `announcement` as the actor `actor` on `domain` publishes it: a public
/// `Note`, behind `content_warning` where it is given.
#[must_use]
pub fn note(
    domain: &str,
    actor: &str,
    announcement: &Announcement,
    content_warning: Option<&str>,
) -> Value {
    let id = actor_id(domain, actor);
    let mut note = json!({
        "id": note_id(domain, actor, &announcement.key.to_string()),
        "type": "Note",
        "attributedTo": id,
        "published": announcement.occurred.to_string(),
        "to": [PUBLIC],
        "cc": [followers(&id)],
        "content": content(announcement),
        "sensitive": content_warning.is_some(),
    });
    note.as_object_mut().into_iter().for_each(|fields| {
        if let Some(link) = &announcement.link {
            fields.insert("url".to_owned(), json!(safe_url(link)));
        }
        if let Some(warning) = content_warning {
            fields.insert("summary".to_owned(), json!(warning));
        }
    });
    note
}

fn followers(actor: &str) -> String {
    format!("{actor}/followers")
}

/// The `Create` of `announcement`'s Note, as each follower's inbox takes it.
#[must_use]
pub fn create(
    domain: &str,
    actor: &str,
    announcement: &Announcement,
    content_warning: Option<&str>,
) -> Value {
    let id = actor_id(domain, actor);
    json!({
        "@context": STREAMS,
        "id": format!("{}/activity", note_id(domain, actor, &announcement.key.to_string())),
        "type": "Create",
        "actor": id,
        "published": announcement.occurred.to_string(),
        "to": [PUBLIC],
        "cc": [followers(&id)],
        "object": note(domain, actor, announcement, content_warning),
    })
}
