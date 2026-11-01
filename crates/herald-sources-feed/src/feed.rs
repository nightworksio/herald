use std::collections::BTreeMap;
use std::fmt::Write;

use herald_core::{Cursor, Draft, Event, Kind, Polled, digest};
use herald_ports::{Method, Request, Response, Text, USER_AGENT};
use herald_wire::{NOT_MODIFIED, PAGE, SourceError, answered, host};
use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event as Xml};

use crate::entity::{decoded, resolved};
use crate::html::{markdown, plain};
use crate::options::FeedOptions;

/// The most bytes a feed may hold.
const LIMIT: usize = 4_194_304;
/// What a feed is called in an error.
const WHAT: &str = "the feed";
/// What herald asks a feed's server for.
const ACCEPT: &str =
    "application/atom+xml, application/rss+xml, application/xml;q=0.9, text/xml;q=0.9, */*;q=0.1";

/// One RSS 2.0 or Atom 1.0 feed, whose entries are asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    url: String,
}

/// Which format a feed is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    /// RSS 2.0, whose items are in its channel.
    Rss,
    /// RSS 1.0, whose items are beside its channel.
    Rdf,
    /// Atom 1.0.
    Atom,
}

impl Format {
    /// How deep an item or entry is.
    const fn item(self) -> usize {
        match self {
            Self::Rss => 3,
            Self::Rdf | Self::Atom => 2,
        }
    }

    /// How deep the feed's own title is.
    const fn title(self) -> usize {
        match self {
            Self::Rss | Self::Rdf => 3,
            Self::Atom => 2,
        }
    }
}

/// What an entry says, as it is read.
#[derive(Debug, Default)]
struct Entry {
    id: Option<String>,
    link: Option<String>,
    title: Option<String>,
    content: Option<String>,
    summary: Option<String>,
    published: Option<Timestamp>,
    updated: Option<Timestamp>,
}

/// The field of an entry that text is read into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Id,
    Link,
    Title { html: bool },
    Content { html: bool },
    Summary { html: bool },
    Published,
    Updated,
}

/// What text read inside an element goes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// The feed's own title.
    FeedTitle,
    /// A field of the entry being read.
    Entry(Field),
}

/// A feed, part-way through being read.
struct Reading {
    format: Format,
    depth: usize,
    feed_title: String,
    entries: Vec<Entry>,
    entry: Option<Entry>,
    target: Option<(Target, usize, String)>,
}

/// A time as a feed writes it: RFC 3339, as Atom does, RFC 2822, as RSS
/// does, or a date or a time with no offset, as some RSS 1.0 feeds do,
/// which is read as UTC.
fn when(text: &str) -> Option<Timestamp> {
    let text = text.trim();
    let civil = || {
        text.parse::<DateTime>()
            .or_else(|_| text.parse::<Date>().map(|date| date.at(0, 0, 0, 0)))
            .ok()
            .and_then(|civil| civil.to_zoned(TimeZone::UTC).ok())
            .map(|zoned| zoned.timestamp())
    };
    text.parse::<Timestamp>()
        .ok()
        .or_else(|| {
            jiff::fmt::rfc2822::parse(text)
                .ok()
                .map(|zoned| zoned.timestamp())
        })
        .or_else(civil)
}

fn attribute(element: &BytesStart<'_>, wanted: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.local_name().as_ref() == wanted)
        .map(|attribute| decoded(&attribute.value).into_owned())
}

/// Whether an Atom text construct of `element` holds HTML, and not text.
fn html(element: &BytesStart<'_>) -> bool {
    attribute(element, "type").is_some_and(|kind| kind == "html" || kind == "xhtml")
}

fn web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

fn unreadable(reason: impl std::fmt::Display) -> SourceError {
    SourceError::Unreadable {
        what: WHAT,
        reason: reason.to_string(),
    }
}

fn neither() -> SourceError {
    unreadable("it is neither RSS nor Atom")
}

/// The format the root element of `reader` names.
fn root(reader: &mut Reader<&[u8]>) -> Result<Format, SourceError> {
    loop {
        match reader.read_event().map_err(unreadable)? {
            Xml::Start(element) => {
                return match element.local_name().as_ref() {
                    "rss" => Ok(Format::Rss),
                    "RDF" => Ok(Format::Rdf),
                    "feed" => Ok(Format::Atom),
                    _ => Err(neither()),
                };
            }
            Xml::Eof => return Err(neither()),
            _ => {}
        }
    }
}

impl Reading {
    /// The text being read, if any.
    fn held(&mut self) -> Option<&mut String> {
        self.target.as_mut().map(|(_, _, held)| held)
    }

    /// What text inside `element`, an entry's child, goes into, reading an
    /// XHTML content whole from `reader`.
    fn chosen(
        &mut self,
        reader: &mut Reader<&[u8]>,
        element: &BytesStart<'_>,
    ) -> Result<Option<Field>, SourceError> {
        let local = element.local_name();
        let qualified = element.name();
        let rss = matches!(self.format, Format::Rss | Format::Rdf);
        let field = match (rss, local.as_ref()) {
            (true, "title") => Some(Field::Title { html: false }),
            (true, "guid") | (false, "id") => Some(Field::Id),
            (true, "link") => Some(Field::Link),
            (true, "description") => Some(Field::Summary { html: true }),
            (true, "encoded") if qualified.as_ref() == "content:encoded" => {
                Some(Field::Content { html: true })
            }
            (true, "pubDate" | "date") | (false, "published") => Some(Field::Published),
            (false, "title") => Some(Field::Title {
                html: html(element),
            }),
            (false, "updated") => Some(Field::Updated),
            (false, "summary") => Some(Field::Summary {
                html: html(element),
            }),
            (false, "content") if attribute(element, "type").as_deref() == Some("xhtml") => {
                let end = element.to_end().into_owned();
                let inner = reader.read_text(end.name()).map_err(unreadable)?;
                self.depth -= 1;
                if let Some(entry) = self.entry.as_mut() {
                    entry.content = Some(inner.xml10_content().into_owned());
                }
                None
            }
            (false, "content") => Some(Field::Content {
                html: html(element),
            }),
            (false, "link") => {
                let alternate = attribute(element, "rel").is_none_or(|rel| rel == "alternate");
                if let (true, Some(entry)) = (alternate, self.entry.as_mut())
                    && entry.link.is_none()
                {
                    entry.link = attribute(element, "href");
                }
                None
            }
            _ => None,
        };
        Ok(field)
    }

    fn start(
        &mut self,
        reader: &mut Reader<&[u8]>,
        element: &BytesStart<'_>,
    ) -> Result<(), SourceError> {
        self.depth += 1;
        let item = match self.format {
            Format::Rss | Format::Rdf => element.local_name().as_ref() == "item",
            Format::Atom => element.local_name().as_ref() == "entry",
        };
        if item && self.depth == self.format.item() {
            self.entry = Some(Entry {
                id: attribute(element, "about"),
                ..Entry::default()
            });
            return Ok(());
        }
        if self.target.is_some() {
            return Ok(());
        }
        let target = if self.entry.is_some() {
            if self.depth == self.format.item() + 1 {
                self.chosen(reader, element)?.map(Target::Entry)
            } else {
                None
            }
        } else {
            (self.depth == self.format.title() && element.local_name().as_ref() == "title")
                .then_some(Target::FeedTitle)
        };
        self.target = target.map(|target| (target, self.depth, String::new()));
        Ok(())
    }

    fn end(&mut self) {
        if let Some((target, _, text)) = self.target.take_if(|(_, at, _)| *at == self.depth) {
            let text = text.trim().to_owned();
            match target {
                Target::FeedTitle => self.feed_title = plain(&text),
                Target::Entry(field) => {
                    if let Some(entry) = self.entry.as_mut() {
                        entry.read(field, text);
                    }
                }
            }
        }
        if self.depth == self.format.item() {
            self.entries.extend(self.entry.take());
        }
        self.depth = self.depth.saturating_sub(1);
    }
}

impl Entry {
    fn read(&mut self, field: Field, text: String) {
        match field {
            Field::Id => self.id = Some(text),
            Field::Link => self.link = Some(text),
            Field::Title { html } => {
                self.title = Some(if html {
                    plain(&text)
                } else {
                    text.split_whitespace().collect::<Vec<_>>().join(" ")
                });
            }
            Field::Content { html } => {
                self.content = Some(if html { text } else { escaped_text(&text) });
            }
            Field::Summary { html } => {
                self.summary = Some(if html { text } else { escaped_text(&text) });
            }
            Field::Published => self.published = when(&text),
            Field::Updated => self.updated = when(&text),
        }
    }
}

/// The feed's title and its entries, as `bytes` write them.
fn read(bytes: &[u8]) -> Result<(String, Vec<Entry>), SourceError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().expand_empty_elements = true;
    let mut reading = Reading {
        format: root(&mut reader)?,
        depth: 1,
        feed_title: String::new(),
        entries: Vec::new(),
        entry: None,
        target: None,
    };
    loop {
        match reader.read_event().map_err(unreadable)? {
            Xml::Start(element) => reading.start(&mut reader, &element)?,
            Xml::End(_) => reading.end(),
            Xml::Text(text) => {
                if let Some(held) = reading.held() {
                    held.push_str(&text.xml10_content());
                }
            }
            Xml::CData(text) => {
                if let Some(held) = reading.held() {
                    held.push_str(&text.xml10_content());
                }
            }
            Xml::GeneralRef(reference) => {
                if let Some(held) = reading.held() {
                    let name = reference.xml10_content();
                    match resolved(&name) {
                        Some(character) => held.push_str(&character),
                        None => {
                            let _ = write!(held, "&{name};");
                        }
                    }
                }
            }
            Xml::Eof => break,
            _ => {}
        }
    }
    Ok((reading.feed_title, reading.entries))
}

/// Plain text as HTML that shows it as it is.
fn escaped_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;")
}

impl Feed {
    /// The feed `options` name.
    #[must_use]
    pub fn new(options: &FeedOptions) -> Self {
        Self {
            url: options.url.trim().to_owned(),
        }
    }

    /// The base URL of everything it asks: the feed's own.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.url
    }

    /// The request for the feed, conditional on the validators `cursor`
    /// holds.
    #[must_use]
    pub fn request(&self, cursor: Option<&Cursor>) -> Request {
        let mut request = Request::new(Method::Get, Text::plain(self.url.clone()))
            .header("Accept", Text::plain(ACCEPT))
            .header("User-Agent", Text::plain(USER_AGENT));
        if let Some(etag) = cursor.and_then(|cursor| cursor.etag.as_deref()) {
            request = request.header("If-None-Match", Text::plain(etag));
        }
        if let Some(modified) = cursor.and_then(|cursor| cursor.last_modified.as_deref()) {
            request = request.header("If-Modified-Since", Text::plain(modified));
        }
        request
    }

    /// Whether the server's answer to [`Feed::request`] says nothing changed
    /// since the cursor's validators.
    #[must_use]
    pub fn unchanged(response: &Response) -> bool {
        response.status == NOT_MODIFIED
    }

    /// The newest entries in the server's answer to [`Feed::request`], the
    /// newest first. An entry that gives no time of its own takes the time
    /// of the answer.
    ///
    /// # Errors
    ///
    /// When the server did not answer with RSS or Atom, or an entry makes
    /// no event.
    pub fn entries(&self, response: &Response) -> Result<Polled, SourceError> {
        answered(WHAT, response)?;
        if response.body.len() > LIMIT {
            return Err(SourceError::TooLarge {
                what: WHAT,
                limit: LIMIT,
            });
        }
        let (feed_title, entries) = read(&response.body)?;
        let answered_at = response.header("date").and_then(when);
        let project = if feed_title.is_empty() {
            host(&self.url).to_owned()
        } else {
            feed_title
        };
        let mut events = entries
            .into_iter()
            .filter_map(|entry| self.event(&project, entry, answered_at).transpose())
            .collect::<Result<Vec<Event>, SourceError>>()?;
        events.sort_by_key(|event| std::cmp::Reverse(event.occurred()));
        events.truncate(PAGE);
        Ok(Polled {
            events,
            etag: response.header("etag").map(ToOwned::to_owned),
            last_modified: response.header("last-modified").map(ToOwned::to_owned),
        })
    }

    fn event(
        &self,
        project: &str,
        entry: Entry,
        answered_at: Option<Timestamp>,
    ) -> Result<Option<Event>, SourceError> {
        let own = entry.published.or(entry.updated);
        let Some(occurred) = own.or(answered_at) else {
            return Ok(None);
        };
        let link = entry.link.filter(|link| web(link));
        let title = entry.title.unwrap_or_default();
        let named = entry
            .id
            .filter(|id| !id.is_empty())
            .or_else(|| link.clone())
            .unwrap_or_else(|| {
                let date = own.map(|own| own.to_string()).unwrap_or_default();
                digest(&format!("{title}\n{date}"))
            });
        let body = entry.content.or(entry.summary).map(|html| markdown(&html));
        let title = if title.is_empty() {
            link.clone().unwrap_or_else(|| project.to_owned())
        } else {
            title
        };
        let draft = Draft {
            identity: format!("{}#{named}", self.url),
            kind: Kind::FeedEntry,
            project: project.to_owned(),
            version: None,
            title,
            body: body.unwrap_or_default(),
            links: link
                .map(|link| BTreeMap::from([("entry".to_owned(), link)]))
                .unwrap_or_default(),
            prerelease: false,
            occurred,
        };
        Event::normalise(draft)
            .map(Some)
            .map_err(|problem| SourceError::Invalid {
                what: WHAT,
                problem,
            })
    }
}
