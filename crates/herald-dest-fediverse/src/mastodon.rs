use herald_core::{
    Announcement, Dialect, Entry, Features, Limits, Measure, MentionStyle, Pins, Profile, ReadBack,
    Sent, Step, Threads, Waits,
};
use herald_ports::{Http, HttpError, Request, Secret};
use jiff::SignedDuration;

use crate::options::RouteOptions;
use crate::read::read;
use crate::request::{Refusal, step};

/// The most characters Mastodon takes in a status, where a route gives no
/// limit of its server's own.
pub const LIMIT: usize = 500;
/// The two blank lines between the title, the text and the link.
const BREAKS: usize = 4;

/// One Mastodon destination, for one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mastodon {
    pub(crate) server: String,
    pub(crate) token: Secret,
    pub(crate) route: RouteOptions,
    pub(crate) read_back: ReadBack,
    pub(crate) waits: Waits,
}

impl Mastodon {
    /// The waits where the config sets none: a limit lasts as long as
    /// Mastodon's own windows, five minutes.
    pub const WAITS: Waits = Waits {
        limited: SignedDuration::from_mins(5),
        ..Waits::DEFAULT
    };

    /// The account whose access token is `token`, on the server at `server`,
    /// for a route whose Mastodon options are `route`.
    #[must_use]
    pub fn new(server: &str, token: Secret, route: RouteOptions) -> Self {
        Self {
            server: server.trim_end_matches('/').to_owned(),
            token,
            route,
            read_back: ReadBack::DEFAULT,
            waits: Self::WAITS,
        }
    }

    /// The destination, waiting `waits` where the server names no wait.
    #[must_use]
    pub fn waiting(self, waits: Waits) -> Self {
        Self { waits, ..self }
    }

    /// The destination, reading back `read_back` pages of its own posts.
    #[must_use]
    pub fn reading_back(self, read_back: ReadBack) -> Self {
        Self { read_back, ..self }
    }

    /// The base URL of the server it reaches, the only origin its `Http` may
    /// reach (ADR-0007).
    #[must_use]
    pub fn api(&self) -> &str {
        &self.server
    }

    /// What the destination can do, as the core plans with it.
    #[must_use]
    pub fn profile(&self) -> Profile {
        profile(&self.route)
    }

    /// The request for `step` of `announcement`, given what `entry` says
    /// already landed.
    ///
    /// # Errors
    ///
    /// When Mastodon cannot take the step, or an earlier step it needs has
    /// not landed.
    pub fn request(
        &self,
        step_to_take: &Step,
        announcement: &Announcement,
        entry: &Entry,
    ) -> Result<Request, Refusal> {
        step(self, step_to_take, announcement, entry)
    }

    /// Sends `step` of `announcement` and reads Mastodon's answer.
    ///
    /// # Errors
    ///
    /// When the request got no response at all.
    pub async fn send<H: Http>(
        &self,
        http: &H,
        step_to_take: &Step,
        announcement: &Announcement,
        entry: &Entry,
    ) -> Result<Sent, HttpError> {
        let request = match self.request(step_to_take, announcement, entry) {
            Ok(request) => request,
            Err(refusal) => return Ok(Sent::Refused(refusal.to_string())),
        };
        Ok(read(&http.send(request).await?, self.waits))
    }
}

/// The fewest characters a status of `route` must be allowed: its title,
/// its link, the breaks between them, one character of text, and its
/// content warning, which the server counts with the status.
#[must_use]
pub fn shortest(route: &RouteOptions) -> usize {
    route.title() + BREAKS + route.link_length() + 1 + warning(route)
}

fn warning(route: &RouteOptions) -> usize {
    route
        .content_warning
        .as_deref()
        .map_or(0, |warning| warning.chars().count())
}

/// What a Mastodon destination can do, for a route with `route`'s options.
#[must_use]
pub fn profile(route: &RouteOptions) -> Profile {
    let limit = route.limit.unwrap_or(LIMIT);
    let each = limit.saturating_sub(warning(route));
    Profile {
        dialect: Dialect::Plain,
        measure: Measure::Chars,
        limits: Limits {
            title: route.title(),
            text: each
                .saturating_sub(route.title() + BREAKS + route.link_length())
                .max(1),
            followup: each.max(1),
            thread_name: route.title(),
        },
        features: Features {
            threads: Threads::Replies,
            crosspost: false,
            pins: Pins::None,
            reads_back: true,
            edits: true,
            mentions: MentionStyle::None,
            interactions: false,
        },
        symbols: herald_core::Symbols::DEFAULT,
    }
}
