use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd, html};

use crate::profile::Dialect;

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
pub fn convert(markdown: &str, dialect: Dialect) -> Text {
    match dialect {
        Dialect::Matrix => {
            let mut html = String::new();
            html::push_html(&mut html, safe(markdown, dialect).map(matrix));
            Text {
                body: Writer::run(markdown, Dialect::Plain),
                html: Some(html.trim_end().to_owned()),
            }
        }
        _ => Text {
            body: Writer::run(markdown, dialect),
            html: None,
        },
    }
}

/// `text` shown literally in `dialect`: nothing in it is read as markup, a
/// mention or a link. For a title or a name placed where markup is read.
#[must_use]
pub fn literal(text: &str, dialect: Dialect) -> Text {
    match dialect {
        Dialect::Discord => Text {
            body: discord_escape(text),
            html: None,
        },
        Dialect::Slack => Text {
            body: slack_escape(text),
            html: None,
        },
        Dialect::Matrix => {
            let mut html = String::new();
            html::push_html(
                &mut html,
                std::iter::once(Event::Text(CowStr::from(neutralised(text)))),
            );
            Text {
                body: neutralised(text),
                html: Some(html),
            }
        }
        Dialect::Plain => Text {
            body: neutralised(text),
            html: None,
        },
    }
}

/// The Markdown extensions every dialect reads.
pub(crate) const EXTENSIONS: Options =
    Options::ENABLE_STRIKETHROUGH.union(Options::ENABLE_TASKLISTS);

const BREAK: char = '\u{200B}';
const PILLS: &str = "https://matrix.to/";

fn parser(markdown: &str) -> Parser<'_> {
    Parser::new_ext(markdown, EXTENSIONS)
}

fn linkable(url: &str, dialect: Dialect) -> bool {
    let web = ["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| url.starts_with(scheme));
    web && !(dialect == Dialect::Matrix && url.starts_with(PILLS))
}

fn safe_url(url: &str) -> String {
    url.bytes()
        .map(|byte| {
            if byte.is_ascii_graphic() && !b"\"<>\\^`{|}()[]".contains(&byte) {
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
    text.replace('@', &format!("@{BREAK}"))
}

fn safe(markdown: &str, dialect: Dialect) -> impl Iterator<Item = Event<'_>> {
    let mut unsafe_links = Vec::new();
    parser(markdown).filter_map(move |event| match event {
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let keep = linkable(&dest_url, dialect);
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
            let keep = linkable(&dest_url, dialect);
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
    dialect: Dialect,
    root: String,
    buffers: Vec<String>,
    lists: Vec<Option<u64>>,
    links: Vec<Option<String>>,
    code: bool,
}

impl Writer {
    fn run(markdown: &str, dialect: Dialect) -> String {
        let mut writer = Self {
            dialect,
            root: String::new(),
            buffers: Vec::new(),
            lists: Vec::new(),
            links: Vec::new(),
            code: false,
        };
        for event in safe(markdown, dialect) {
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
        self.out().push_str(text);
    }

    fn line(&mut self) {
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
            Event::TaskListMarker(done) => self.push(if done { "☑ " } else { "☐ " }),
            Event::SoftBreak | Event::HardBreak => self.push("\n"),
            Event::Rule => {
                self.block();
                self.push("───");
                self.block();
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.block();
                match self.dialect {
                    Dialect::Discord => {
                        let depth = (level as usize).min(3);
                        self.push(&format!("{} ", "#".repeat(depth)));
                    }
                    Dialect::Slack => self.push("*"),
                    _ => {}
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
                    CodeBlockKind::Fenced(info) if self.dialect == Dialect::Discord => {
                        language(&info)
                    }
                    _ => String::new(),
                };
                if self.dialect != Dialect::Plain {
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
                    _ if self.dialect == Dialect::Slack => "• ".to_owned(),
                    _ => "- ".to_owned(),
                };
                self.push(&format!("{}{marker}", "  ".repeat(depth)));
            }
            Tag::Emphasis => self.mark("*", "_", ""),
            Tag::Strong => self.mark("**", "*", ""),
            Tag::Strikethrough => self.mark("~~", "~", ""),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => self.open_link(&dest_url),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.block(),
            TagEnd::Heading(_) => {
                if self.dialect == Dialect::Slack {
                    self.push("*");
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
                if self.dialect != Dialect::Plain {
                    self.push("```");
                }
                self.block();
            }
            TagEnd::List(_) => {
                self.lists.pop();
                self.block();
            }
            TagEnd::Emphasis => self.mark("*", "_", ""),
            TagEnd::Strong => self.mark("**", "*", ""),
            TagEnd::Strikethrough => self.mark("~~", "~", ""),
            TagEnd::Link | TagEnd::Image => self.close_link(),
            _ => {}
        }
    }

    fn mark(&mut self, discord: &str, slack: &str, plain: &str) {
        let mark = match self.dialect {
            Dialect::Discord => discord,
            Dialect::Slack => slack,
            _ => plain,
        };
        self.push(mark);
    }

    fn inline_code(&mut self, code: &str) {
        match self.dialect {
            Dialect::Plain | Dialect::Matrix => self.push(&neutralised(code)),
            Dialect::Slack if code.contains('`') => self.push(&slack_escape(code)),
            Dialect::Slack => self.push(&format!("`{}`", slack_escape(code))),
            Dialect::Discord => {
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
        let link = match self.dialect {
            Dialect::Slack | Dialect::Discord if bare => format!("<{url}>"),
            Dialect::Slack => format!("<{url}|{text}>"),
            Dialect::Discord => format!("{text} (<{url}>)"),
            _ if bare => url,
            _ => format!("{text} ({url})"),
        };
        self.push(&link);
    }

    fn escape(&self, text: &str) -> String {
        match self.dialect {
            Dialect::Slack if self.code => slack_escape(&unfenced(text)),
            Dialect::Slack => slack_escape(text),
            Dialect::Discord if self.code => unfenced(&neutralised(text)),
            Dialect::Discord => discord_escape(text),
            _ => neutralised(text),
        }
    }
}

fn slack_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn discord_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii_punctuation() && character != '@' {
            escaped.push('\\');
        }
        escaped.push(character);
        if character == '@' {
            escaped.push(BREAK);
        }
    }
    escaped
}

#[cfg(test)]
mod inert;
#[cfg(test)]
mod tests;
