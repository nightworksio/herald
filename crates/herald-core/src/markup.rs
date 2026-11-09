use pulldown_cmark::{CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd, html};

use crate::profile::{Bullet, Escape, Fences, Heading, Links, Markup};
use crate::symbols::Symbols;

/// A text converted to a destination's dialect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    /// The text in the dialect, or plain text beside `html` for Matrix.
    pub body: String,
    /// The sanitised HTML, for Matrix alone.
    pub html: Option<String>,
}

/// `markdown` in `dialect`.
///
/// Whatever the Markdown says, the result mentions no one and links nowhere it
/// does not show: raw HTML is shown as text, a link to anything but the web or
/// mail keeps only its text, a link shows its URL, every URL is
/// percent-encoded past what could close it, and an `@` cannot start a
/// mention.
#[must_use]
pub fn convert(markdown: &str, markup: Markup) -> Text {
    convert_in(markdown, markup, Symbols::DEFAULT)
}

/// `markdown` in `markup`, as [`convert`] writes it, with `symbols` where
/// the markup has none of its own.
#[must_use]
pub fn convert_in(markdown: &str, markup: Markup, symbols: Symbols) -> Text {
    let body = Writer::run(markdown, markup, symbols);
    if !markup.html {
        return Text { body, html: None };
    }
    let mut html = String::new();
    let events: Vec<Event<'static>> = safe(markdown, markup)
        .map(matrix)
        .map(Event::into_static)
        .collect();
    html::push_html(&mut html, shown_links(events.into_iter()).into_iter());
    Text {
        body,
        html: Some(html.trim_end().to_owned()),
    }
}

/// `text` as Markdown that shows it literally: every ASCII punctuation
/// mark escaped, so nothing in it is read as markup.
#[must_use]
pub fn escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii_punctuation() {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// `text` shown literally in `dialect`: nothing in it is read as markup, a
/// mention or a link. For a title or a name placed where markup is read.
#[must_use]
pub fn literal(text: &str, markup: Markup) -> Text {
    let body = match markup.escape {
        Escape::Backslash => backslashed(text),
        Escape::Entities => entities(text),
        Escape::Neutral => neutralised(text),
    };
    if !markup.html {
        return Text { body, html: None };
    }
    let mut html = String::new();
    html::push_html(
        &mut html,
        std::iter::once(Event::Text(CowStr::from(neutralised(text)))),
    );
    Text {
        body,
        html: Some(html),
    }
}

/// The Markdown extensions every markup reads.
pub(crate) const EXTENSIONS: Options =
    Options::ENABLE_STRIKETHROUGH.union(Options::ENABLE_TASKLISTS);

const BREAK: char = '\u{200B}';
const SIGILS: [char; 4] = ['@', '\u{FF20}', '\u{FE6B}', '#'];
/// The schemes a link keeps its target with.
pub const LINK_SCHEMES: [&str; 3] = ["https://", "http://", "mailto:"];

fn parser(markdown: &str) -> Parser<'_> {
    Parser::new_ext(markdown, EXTENSIONS)
}

fn linkable(url: &str, markup: Markup) -> bool {
    let web = LINK_SCHEMES.iter().any(|scheme| url.starts_with(scheme));
    web && !markup.pills.is_some_and(|pills| url.starts_with(pills))
}

/// `url` with every byte that could end a Markdown link or open markup in
/// it percent-encoded.
#[must_use]
pub fn safe_url(url: &str) -> String {
    url.bytes()
        .map(|byte| {
            if byte.is_ascii_graphic() && !b"\"'<>\\^`{|}()[]".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn language(info: &str) -> String {
    info.split_whitespace()
        .next()
        .filter(|word| {
            word.chars()
                .all(|character| character.is_ascii_alphanumeric() || "_+-.#".contains(character))
        })
        .unwrap_or_default()
        .to_owned()
}

fn unfenced(code: &str) -> String {
    let mut code = code.to_owned();
    while code.contains("``") {
        code = code.replace("``", &format!("`{BREAK}`"));
    }
    code
}

fn neutralised(text: &str) -> String {
    let mut neutral = String::with_capacity(text.len());
    for character in text.chars() {
        neutral.push(character);
        if SIGILS.contains(&character) {
            neutral.push(BREAK);
        }
    }
    neutral
}

fn shown_links(events: impl Iterator<Item = Event<'static>>) -> Vec<Event<'static>> {
    let mut shown = Vec::new();
    let mut open: Option<(CowStr<'static>, Vec<Event<'static>>, usize)> = None;
    for event in events {
        open = match (open, event) {
            (None, Event::Start(Tag::Link { dest_url, .. })) => Some((dest_url, Vec::new(), 0)),
            (None, other) => {
                shown.push(other);
                None
            }
            (Some((url, inner, depth)), Event::Start(Tag::Link { .. })) => {
                Some((url, inner, depth + 1))
            }
            (Some((url, inner, depth)), Event::End(TagEnd::Link)) if depth > 0 => {
                Some((url, inner, depth - 1))
            }
            (Some((url, inner, _)), Event::End(TagEnd::Link)) => {
                shown.extend(link(url, inner));
                None
            }
            (Some((url, mut inner, depth)), other) => {
                inner.push(other);
                Some((url, inner, depth))
            }
        };
    }
    shown
}

fn link(url: CowStr<'static>, text: Vec<Event<'static>>) -> Vec<Event<'static>> {
    let plain: String = text
        .iter()
        .filter_map(|event| {
            if let Event::Text(text) = event {
                Some(text.as_ref())
            } else {
                None
            }
        })
        .collect();
    let bare = text.is_empty() || plain == url.as_ref();
    let shown = CowStr::from(neutralised(&url));
    let anchor = [
        Event::Start(Tag::Link {
            link_type: LinkType::Inline,
            dest_url: url,
            title: CowStr::Borrowed(""),
            id: CowStr::Borrowed(""),
        }),
        Event::Text(shown),
        Event::End(TagEnd::Link),
    ];
    if bare {
        return anchor.into();
    }
    text.into_iter()
        .chain([Event::Text(CowStr::Borrowed(" ("))])
        .chain(anchor)
        .chain([Event::Text(CowStr::Borrowed(")"))])
        .collect()
}

fn safe(markdown: &str, markup: Markup) -> impl Iterator<Item = Event<'_>> {
    let mut unsafe_links = Vec::new();
    parser(markdown).filter_map(move |event| match event {
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let keep = linkable(&dest_url, markup);
            unsafe_links.push(!keep);
            let dest_url = CowStr::from(safe_url(&dest_url));
            keep.then_some(Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }))
        }
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let keep = linkable(&dest_url, markup);
            unsafe_links.push(!keep);
            let dest_url = CowStr::from(safe_url(&dest_url));
            keep.then_some(Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }))
        }
        Event::End(TagEnd::Link | TagEnd::Image) => {
            let drop = unsafe_links.pop().unwrap_or(false);
            (!drop).then_some(event)
        }
        other => Some(other),
    })
}

fn matrix(event: Event<'_>) -> Event<'_> {
    match event {
        Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
            Event::Text(CowStr::from(neutralised(&text)))
        }
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }),
        Event::Code(code) => Event::Code(CowStr::from(neutralised(&code))),
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => Event::Start(Tag::CodeBlock(
            CodeBlockKind::Fenced(CowStr::from(language(&info))),
        )),
        Event::End(TagEnd::Image) => Event::End(TagEnd::Link),
        other => other,
    }
}

struct Writer {
    markup: Markup,
    symbols: Symbols,
    root: String,
    buffers: Vec<String>,
    lists: Vec<Option<u64>>,
    links: Vec<Option<String>>,
    code: bool,
    spaced: bool,
}

impl Writer {
    fn run(markdown: &str, markup: Markup, symbols: Symbols) -> String {
        let mut writer = Self {
            markup,
            symbols,
            root: String::new(),
            buffers: Vec::new(),
            lists: Vec::new(),
            links: Vec::new(),
            code: false,
            spaced: false,
        };
        for event in safe(markdown, markup) {
            writer.event(event);
        }
        writer.root.trim().to_owned()
    }

    fn out(&mut self) -> &mut String {
        match self.buffers.last_mut() {
            Some(buffer) => buffer,
            None => &mut self.root,
        }
    }

    fn push(&mut self, text: &str) {
        if std::mem::take(&mut self.spaced) && !text.starts_with(char::is_whitespace) {
            self.out().push(' ');
        }
        self.out().push_str(text);
    }

    fn line(&mut self) {
        self.spaced = false;
        let out = self.out();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
    }

    fn block(&mut self) {
        if self.lists.is_empty() {
            self.line();
            let out = self.out();
            if !out.is_empty() && !out.ends_with("\n\n") {
                out.push('\n');
            }
        } else {
            self.line();
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t)
            | Event::Html(t)
            | Event::InlineHtml(t)
            | Event::FootnoteReference(t) => {
                let text = self.escape(&t);
                self.push(&text);
            }
            Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => self.inline_code(&t),
            Event::TaskListMarker(done) => {
                let marker = if done {
                    self.symbols.done
                } else {
                    self.symbols.todo
                };
                self.push(&format!("{marker} "));
            }
            Event::SoftBreak | Event::HardBreak => self.push("\n"),
            Event::Rule => {
                self.block();
                self.push(&self.symbols.rule.to_string().repeat(3));
                self.block();
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.block();
                match self.markup.heading {
                    Heading::Hashes(most) => {
                        let depth = (level as usize).min(usize::from(most));
                        self.push(&format!("{} ", "#".repeat(depth)));
                    }
                    Heading::Wrapped(mark) => self.push(mark),
                    Heading::Bare => {}
                }
            }
            Tag::BlockQuote(_) => {
                self.block();
                self.buffers.push(String::new());
            }
            Tag::CodeBlock(kind) => {
                self.block();
                self.code = true;
                let language = match kind {
                    CodeBlockKind::Fenced(info) if self.markup.fences == Fences::Labelled => {
                        language(&info)
                    }
                    _ => String::new(),
                };
                if self.markup.fences != Fences::None {
                    self.push(&format!("```{language}\n"));
                }
            }
            Tag::List(start) => {
                if self.lists.is_empty() {
                    self.block();
                } else {
                    self.line();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.line();
                let depth = self.lists.len().saturating_sub(1);
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}. ");
                        *number += 1;
                        marker
                    }
                    _ => match self.markup.bullet {
                        Bullet::Symbol(bullet) => {
                            format!("{} ", self.symbols.bullet.unwrap_or(bullet))
                        }
                        Bullet::Fixed(marker) => marker.to_owned(),
                    },
                };
                self.push(&format!("{}{marker}", "  ".repeat(depth)));
            }
            Tag::Emphasis => self.push(self.markup.marks.emphasis),
            Tag::Strong => self.push(self.markup.marks.strong),
            Tag::Strikethrough => self.push(self.markup.marks.strike),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => self.open_link(&dest_url),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.block(),
            TagEnd::Heading(_) => {
                if let Heading::Wrapped(mark) = self.markup.heading {
                    self.push(mark);
                }
                self.block();
            }
            TagEnd::BlockQuote(_) => {
                let quoted = self.buffers.pop().unwrap_or_default();
                let quoted: Vec<String> = quoted
                    .trim_end()
                    .lines()
                    .map(|line| format!("> {line}").trim_end().to_owned())
                    .collect();
                self.push(&quoted.join("\n"));
                self.block();
            }
            TagEnd::CodeBlock => {
                self.code = false;
                self.line();
                if self.markup.fences != Fences::None {
                    self.push("```");
                }
                self.block();
            }
            TagEnd::List(_) => {
                self.lists.pop();
                self.block();
            }
            TagEnd::Emphasis => self.push(self.markup.marks.emphasis),
            TagEnd::Strong => self.push(self.markup.marks.strong),
            TagEnd::Strikethrough => self.push(self.markup.marks.strike),
            TagEnd::Link | TagEnd::Image => self.close_link(),
            _ => {}
        }
    }

    fn inline_code(&mut self, code: &str) {
        match self.markup.escape {
            Escape::Neutral => self.push(&neutralised(code)),
            Escape::Entities if code.contains('`') => self.push(&entities(code)),
            Escape::Entities => self.push(&format!("`{}`", entities(code))),
            Escape::Backslash => {
                let code = unfenced(&neutralised(code));
                let fence = if code.contains('`') { "``" } else { "`" };
                let pad = if code.starts_with('`') || code.ends_with('`') {
                    " "
                } else {
                    ""
                };
                self.push(&format!("{fence}{pad}{code}{pad}{fence}"));
            }
        }
    }

    fn open_link(&mut self, url: &str) {
        let nested = !self.links.is_empty();
        self.links.push((!nested).then(|| url.to_owned()));
        self.buffers.push(String::new());
    }

    fn close_link(&mut self) {
        let text = self.buffers.pop().unwrap_or_default();
        let Some(url) = self.links.pop().flatten() else {
            self.push(&text);
            return;
        };
        let bare = text.is_empty() || text == self.escape(&url);
        let link = match self.markup.links {
            Links::Piped | Links::Bracketed if bare => format!("<{url}>"),
            Links::Piped => format!("<{url}|{}>", text.replace('\n', " ")),
            Links::Bracketed => format!("{text} (<{url}>)"),
            Links::Spelled if bare => url,
            Links::Spelled => format!("{text} ({url})"),
        };
        self.push(&link);
        self.spaced = bare && self.markup.links == Links::Spelled;
    }

    fn escape(&self, text: &str) -> String {
        match self.markup.escape {
            Escape::Entities if self.code => entities(&unfenced(text)),
            Escape::Entities => entities(text),
            Escape::Backslash if self.code => unfenced(&neutralised(text)),
            Escape::Backslash => backslashed(text),
            Escape::Neutral => neutralised(text),
        }
    }
}

fn entities(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn backslashed(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii_punctuation() && character != '@' {
            escaped.push('\\');
        }
        escaped.push(character);
        if SIGILS.contains(&character) {
            escaped.push(BREAK);
        }
    }
    escaped
}

#[cfg(test)]
pub(crate) mod inert;
#[cfg(test)]
mod tests;
