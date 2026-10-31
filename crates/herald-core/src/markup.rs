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

/// `markdown` in `dialect`, with raw HTML shown as text and links to anything
/// but the web or mail shown as their text alone.
#[must_use]
pub fn convert(markdown: &str, dialect: Dialect) -> Text {
    match dialect {
        Dialect::Matrix => {
            let mut html = String::new();
            html::push_html(
                &mut html,
                safe(markdown).map(|event| match event {
                    Event::Html(text) | Event::InlineHtml(text) => Event::Text(text),
                    other => other,
                }),
            );
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

/// The Markdown extensions every dialect reads.
pub(crate) const EXTENSIONS: Options =
    Options::ENABLE_STRIKETHROUGH.union(Options::ENABLE_TASKLISTS);

fn parser(markdown: &str) -> Parser<'_> {
    Parser::new_ext(markdown, EXTENSIONS)
}

fn web(url: &str) -> bool {
    ["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

fn safe(markdown: &str) -> impl Iterator<Item = Event<'_>> {
    let mut unsafe_links = Vec::new();
    parser(markdown).filter_map(move |event| match event {
        Event::Start(Tag::Link { ref dest_url, .. } | Tag::Image { ref dest_url, .. }) => {
            let drop = !web(dest_url);
            unsafe_links.push(drop);
            (!drop).then_some(event)
        }
        Event::End(TagEnd::Link | TagEnd::Image) => {
            let drop = unsafe_links.pop().unwrap_or(false);
            (!drop).then_some(event)
        }
        other => Some(other),
    })
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
        for event in safe(markdown) {
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
                    CodeBlockKind::Fenced(language) if self.dialect == Dialect::Discord => language,
                    _ => CowStr::Borrowed(""),
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
            TagEnd::Item => self.line(),
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
            Dialect::Plain | Dialect::Matrix => self.push(code),
            Dialect::Slack => {
                let code = slack_escape(code);
                self.push(&format!("`{code}`"));
            }
            Dialect::Discord => {
                let fence = if code.contains('`') { "``" } else { "`" };
                self.push(&format!("{fence}{code}{fence}"));
            }
        }
    }

    fn open_link(&mut self, url: &str) {
        self.links.push(Some(url.to_owned()));
        self.buffers.push(String::new());
    }

    fn close_link(&mut self) {
        let text = self.buffers.pop().unwrap_or_default();
        let url = self.links.pop().flatten().unwrap_or_default();
        let bare = text.is_empty() || text == url;
        let link = match self.dialect {
            Dialect::Slack => {
                let url = url.replace('|', "%7C").replace('>', "%3E");
                if bare {
                    format!("<{url}>")
                } else {
                    format!("<{url}|{text}>")
                }
            }
            _ if bare => url,
            Dialect::Discord => format!("[{text}]({url})"),
            _ => format!("{text} ({url})"),
        };
        self.push(&link);
    }

    fn escape(&self, text: &str) -> String {
        match self.dialect {
            Dialect::Slack => slack_escape(text),
            Dialect::Discord if !self.code => discord_escape(text),
            _ => text.to_owned(),
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
        if matches!(
            character,
            '\\' | '*' | '_' | '~' | '`' | '|' | '<' | '>' | '[' | ']' | '#'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests;
