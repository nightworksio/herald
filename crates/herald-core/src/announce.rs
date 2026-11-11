use jiff::Timestamp;
use unicode_segmentation::UnicodeSegmentation;

use crate::chunk::{Room, chunk_in};
use crate::event::Event;
use crate::key::{Key, RouteId};
use crate::kind::{LinkSpec, RELEASE_LINK};
use crate::marker::Marks;
use crate::markup::{Text, literal};
use crate::profile::{Markup, Measure, Profile};
use crate::render::{Context, TemplateError, Templates};
use crate::route::{Route, TemplateName};
use crate::wording::{Phrase, Wording};

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

/// An announcement's title, rendered with nothing escaped. It is read plain
/// only where the destination reads no markup, and shown literally wherever
/// it does, so a title never mentions, tags or links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Title(String);

impl Title {
    /// The title `text`.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The title as written, for a field the destination reads no markup in.
    #[must_use]
    pub fn plain(&self) -> &str {
        &self.0
    }

    /// The title shown literally in `markup`, for a field the destination
    /// reads markup in.
    #[must_use]
    pub fn shown(&self, markup: Markup) -> Text {
        literal(&self.0, markup)
    }
}

/// A button to one of an event's links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    /// The link's name.
    pub name: String,
    /// Its label, where the config gives one.
    pub label: Option<String>,
    /// Where it leads.
    pub url: String,
}

impl Button {
    /// Its label: the config's, or as `wording` words the link, or the
    /// link's name.
    #[must_use]
    pub fn label<'a>(&'a self, wording: &'a Wording) -> &'a str {
        match (&self.label, Phrase::link(&self.name)) {
            (Some(label), _) => label,
            (None, Some(phrase)) => wording.text(phrase),
            (None, None) => &self.name,
        }
    }
}

/// An event rendered for one route on one destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    /// Its never-twice key.
    pub key: Key,
    /// The markers its post carries.
    pub marks: Marks,
    /// The route that made it.
    pub route: RouteId,
    /// The project the event is of.
    pub project: String,
    /// The title, on one line, within the destination's limit.
    pub title: Title,
    /// The body, cut into the parts the destination takes, the first part
    /// first.
    pub parts: Vec<Text>,
    /// The thread's name, where the route opens one.
    pub thread: Option<String>,
    /// The link the announcement leads to: the event's `release` link, or
    /// its first.
    pub link: Option<String>,
    /// The buttons to the links its kind names, in its kind's order, each
    /// where the event carries the link.
    pub buttons: Vec<Button>,
    /// Whether the event is a pre-release.
    pub prerelease: bool,
    /// When the event happened.
    pub occurred: Timestamp,
    /// The route's earlier pinned post, where the store remembers it, for
    /// the unpin step to unpin.
    pub replaces: Option<String>,
    /// The users it mentions: the subscribers herald keeps to its project on
    /// its destination, where its route mentions them, as many as its room
    /// holds.
    pub mentioned: Vec<String>,
    /// The users its room does not hold, each list mentioned in a message of
    /// its own after its parts.
    pub mentioned_after: Vec<Vec<String>>,
}

impl Announcement {
    /// Its part `index`, where it has one.
    #[must_use]
    pub fn part(&self, index: u32) -> Option<&Text> {
        self.parts.get(usize::try_from(index).ok()?)
    }

    /// How many messages it takes: its parts, then a message for each list
    /// of users mentioned after them.
    #[must_use]
    pub fn messages(&self) -> usize {
        self.parts.len() + self.mentioned_after.len()
    }

    /// The users mentioned in its message `index`, where that message is one
    /// of those after its parts.
    #[must_use]
    pub fn mentioned_in(&self, index: usize) -> Option<&[String]> {
        index
            .checked_sub(self.parts.len())
            .and_then(|after| self.mentioned_after.get(after))
            .map(Vec::as_slice)
    }
}

/// `event` rendered by `route` for a destination described by `profile`,
/// carrying `marks`, with a button to each of `links` the event carries.
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
    marks: Marks,
    links: &[LinkSpec],
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
        marks,
        route: route.id.clone(),
        project: event.project().to_owned(),
        title: Title::new(shortened(&title, profile.limits.title, profile.measure)),
        parts: chunk_in(&body, profile.markup, profile.symbols, room),
        thread: thread
            .map(|thread| shortened(&thread, profile.limits.thread_name, profile.measure)),
        link: event
            .link(RELEASE_LINK)
            .or_else(|| event.first_link())
            .map(ToOwned::to_owned),
        buttons: links
            .iter()
            .filter_map(|link| {
                event.link(&link.name).map(|url| Button {
                    name: link.name.clone(),
                    label: link.label.clone(),
                    url: url.to_owned(),
                })
            })
            .collect(),
        prerelease: event.prerelease(),
        occurred: event.occurred(),
        replaces: None,
        mentioned: Vec::new(),
        mentioned_after: Vec::new(),
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
