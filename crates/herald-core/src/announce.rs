use jiff::Timestamp;
use unicode_segmentation::UnicodeSegmentation;

use crate::chunk::{Room, chunk};
use crate::event::Event;
use crate::key::{Key, RouteId};
use crate::marker::Marker;
use crate::markup::Text;
use crate::profile::{Measure, Profile};
use crate::render::{Context, TemplateError, Templates};
use crate::route::{Route, TemplateName};

const ELLIPSIS: &str = "…";

/// The name a template's title is compiled under.
#[must_use]
pub fn title_template(template: &TemplateName) -> String {
    format!("{}/title", template.as_str())
}

/// The name a template's body is compiled under.
#[must_use]
pub fn body_template(template: &TemplateName) -> String {
    format!("{}/body", template.as_str())
}

/// The name a route's thread name is compiled under.
#[must_use]
pub fn thread_template(route: &RouteId) -> String {
    format!("route/{}/thread", route.as_str())
}

/// An event rendered for one route on one destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    /// Its never-twice key.
    pub key: Key,
    /// The marker its posts carry, the MAC of its key.
    pub marker: Marker,
    /// The route that made it.
    pub route: RouteId,
    /// The title, on one line, within the destination's limit.
    pub title: String,
    /// The body, cut into the parts the destination takes, the first part
    /// first.
    pub parts: Vec<Text>,
    /// The thread's name, where the route opens one.
    pub thread: Option<String>,
    /// The link the announcement leads to: the event's `release` link, or
    /// its first.
    pub link: Option<String>,
    /// Whether the event is a pre-release.
    pub prerelease: bool,
    /// When the event happened.
    pub occurred: Timestamp,
}

/// `event` rendered by `route` for a destination described by `profile`,
/// carrying `marker`, the MAC of its key.
///
/// # Errors
///
/// When a template does not render.
pub fn announce(
    event: &Event,
    route: &Route,
    templates: &Templates,
    profile: &Profile,
    herald: &str,
    marker: Marker,
) -> Result<Announcement, TemplateError> {
    let context = Context {
        event,
        route: &route.id,
        herald,
    };
    let title = templates.render_line(&title_template(&route.template), &context)?;
    let body = templates.render(&body_template(&route.template), &context)?;
    let thread = match &route.delivery.thread {
        Some(_) => Some(templates.render_line(&thread_template(&route.id), &context)?),
        None => None,
    };
    let room = Room {
        measure: profile.measure,
        first: profile.limits.text,
        rest: profile.limits.followup,
    };
    Ok(Announcement {
        key: Key::derive(&route.id, event.identity()),
        marker,
        route: route.id.clone(),
        title: shortened(&title, profile.limits.title, profile.measure),
        parts: chunk(&body, profile.dialect, room),
        thread: thread
            .map(|thread| shortened(&thread, profile.limits.thread_name, profile.measure)),
        link: event
            .link("release")
            .or_else(|| event.first_link())
            .map(ToOwned::to_owned),
        prerelease: event.prerelease(),
        occurred: event.occurred(),
    })
}

fn length(text: &str, measure: Measure) -> usize {
    match measure {
        Measure::Chars => text.chars().count(),
        Measure::Bytes => text.len(),
    }
}

/// `text` within `limit`, cut between graphemes and ended with an ellipsis
/// where it was cut.
#[must_use]
pub fn shortened(text: &str, limit: usize, measure: Measure) -> String {
    if length(text, measure) <= limit {
        return text.to_owned();
    }
    let room = limit.saturating_sub(length(ELLIPSIS, measure));
    let mut kept = String::new();
    for grapheme in text.graphemes(true) {
        if length(&kept, measure) + length(grapheme, measure) > room {
            break;
        }
        kept.push_str(grapheme);
    }
    format!("{}{ELLIPSIS}", kept.trim_end())
}

#[cfg(test)]
mod tests;
