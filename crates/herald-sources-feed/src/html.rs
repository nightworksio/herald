use std::fmt::Write;

use crate::entity::decoded;

/// Elements whose content is never text: it is dropped with them.
const DROPPED: [&str; 8] = [
    "script", "style", "template", "iframe", "object", "noscript", "svg", "head",
];
/// Elements that end a paragraph.
const BLOCKS: [&str; 15] = [
    "p",
    "div",
    "section",
    "article",
    "header",
    "footer",
    "figure",
    "figcaption",
    "table",
    "tr",
    "dl",
    "dt",
    "dd",
    "details",
    "summary",
];
/// What Markdown reads in text as markup, escaped wherever it appears.
const MARKUP: [char; 11] = ['\\', '`', '*', '_', '[', ']', '<', '>', '#', '~', '|'];
/// The schemes a link keeps its target with.
const SCHEMES: [&str; 3] = ["https://", "http://", "mailto:"];

/// One piece of HTML.
#[derive(Debug, PartialEq, Eq)]
enum Token<'a> {
    Text(&'a str),
    Open {
        name: String,
        attributes: Vec<(String, String)>,
    },
    Close(String),
}

/// The pieces of `html`: text, opening tags with their attributes, and
/// closing tags. Comments, declarations and the content of every dropped
/// element are left out, and a `<` that opens no tag is text.
fn tokens(html: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(at) = rest.find('<') else {
            tokens.push(Token::Text(rest));
            break;
        };
        let (text, from) = rest.split_at(at);
        if !text.is_empty() {
            tokens.push(Token::Text(text));
        }
        rest = if let Some(after) = from.strip_prefix("<!--") {
            after
                .find("-->")
                .map_or("", |end| after.get(end + 3..).unwrap_or_default())
        } else if from.starts_with("<!") || from.starts_with("<?") {
            from.find('>')
                .map_or("", |end| from.get(end + 1..).unwrap_or_default())
        } else if let Some((token, after)) = tag(from) {
            let dropped = match &token {
                Token::Open { name, .. } if DROPPED.contains(&name.as_str()) => Some(name.clone()),
                _ => None,
            };
            tokens.push(token);
            match dropped {
                Some(name) => skipped(after, &name),
                None => after,
            }
        } else {
            tokens.push(Token::Text("<"));
            from.get(1..).unwrap_or_default()
        };
    }
    tokens
}

/// What follows the closing tag of `name` in `html`, or nothing where it
/// never closes.
fn skipped<'a>(html: &'a str, name: &str) -> &'a str {
    let lower = html.to_ascii_lowercase();
    let close = format!("</{name}");
    match lower.find(&close) {
        Some(at) => {
            let from = html.get(at..).unwrap_or_default();
            from.find('>')
                .map_or("", |end| from.get(end + 1..).unwrap_or_default())
        }
        None => "",
    }
}

fn name_end(text: &str) -> usize {
    text.find(|character: char| !character.is_ascii_alphanumeric() && character != '-')
        .unwrap_or(text.len())
}

/// The tag `html` starts with, and what follows it.
fn tag(html: &str) -> Option<(Token<'_>, &str)> {
    let body = html.get(1..)?;
    let (closing, body) = match body.strip_prefix('/') {
        Some(body) => (true, body),
        None => (false, body),
    };
    if !body.starts_with(|character: char| character.is_ascii_alphabetic()) {
        return None;
    }
    let end = name_end(body);
    let name = body.get(..end)?.to_ascii_lowercase();
    let mut rest = body.get(end..)?;
    let mut attributes = Vec::new();
    loop {
        rest = rest
            .trim_start_matches(|character: char| character.is_whitespace() || character == '/');
        if let Some(after) = rest.strip_prefix('>') {
            let token = if closing {
                Token::Close(name)
            } else {
                Token::Open { name, attributes }
            };
            return Some((token, after));
        }
        if rest.is_empty() {
            return None;
        }
        let end = rest
            .find(|character: char| {
                character.is_whitespace() || matches!(character, '=' | '>' | '/')
            })
            .unwrap_or(rest.len());
        if end == 0 {
            rest = rest.get(1..)?;
            continue;
        }
        let key = rest.get(..end)?.to_ascii_lowercase();
        rest = rest.get(end..)?.trim_start();
        let value = if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let quote = after
                .chars()
                .next()
                .filter(|first| matches!(first, '"' | '\''));
            let (value, left) = if let Some(quote) = quote {
                let inner = after.get(1..)?;
                let close = inner.find(quote).unwrap_or(inner.len());
                (
                    inner.get(..close)?,
                    inner.get(close + 1..).unwrap_or_default(),
                )
            } else {
                let close = after
                    .find(|character: char| character.is_whitespace() || character == '>')
                    .unwrap_or(after.len());
                (after.get(..close)?, after.get(close..)?)
            };
            rest = left;
            decoded(value).into_owned()
        } else {
            String::new()
        };
        attributes.push((key, value));
    }
}

/// `text` with what Markdown would read as markup escaped.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if MARKUP.contains(&character) {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// Where a list's items are numbered, and from what.
enum List {
    Bullets,
    Numbers(u32),
}

/// The Markdown written so far, and where in the HTML it is.
#[derive(Default)]
struct Writer {
    out: String,
    lists: Vec<List>,
    links: Vec<Option<String>>,
    quotes: Vec<usize>,
    preformatted: usize,
    code: usize,
}

impl Writer {
    fn trim_end(&mut self) {
        let kept = self.out.trim_end_matches([' ', '\t']).len();
        self.out.truncate(kept);
    }

    fn block(&mut self) {
        self.trim_end();
        if self.out.is_empty() {
            return;
        }
        while !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn line(&mut self) {
        self.trim_end();
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn text(&mut self, raw: &str) {
        let text = decoded(raw);
        if self.preformatted > 0 {
            self.out.push_str(&text);
            return;
        }
        let at_start = self.out.is_empty() || self.out.ends_with('\n');
        let mut words = text.split_whitespace().peekable();
        if words.peek().is_none() {
            if !text.is_empty() && !at_start && !self.out.ends_with(' ') {
                self.out.push(' ');
            }
            return;
        }
        if text.starts_with(char::is_whitespace) && !at_start && !self.out.ends_with(' ') {
            self.out.push(' ');
        }
        let joined: Vec<&str> = words.collect();
        let joined = joined.join(" ");
        if self.code > 0 {
            self.out.push_str(&joined);
        } else {
            let mut written = escaped(&joined);
            if (self.out.is_empty() || self.out.ends_with('\n')) && written.starts_with(['-', '+'])
            {
                written.insert(0, '\\');
            }
            self.out.push_str(&written);
        }
        if text.ends_with(char::is_whitespace) {
            self.out.push(' ');
        }
    }

    fn open(&mut self, name: &str, attributes: &[(String, String)]) {
        let attribute = |wanted: &str| {
            attributes
                .iter()
                .find(|(key, _)| key == wanted)
                .map(|(_, value)| value.as_str())
        };
        match name {
            "br" => {
                self.trim_end();
                self.out.push_str("  \n");
            }
            "hr" => {
                self.block();
                self.out.push_str("---\n\n");
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.block();
                let level = name
                    .get(1..)
                    .and_then(|level| level.parse().ok())
                    .unwrap_or(1);
                self.out.push_str(&"#".repeat(level));
                self.out.push(' ');
            }
            "strong" | "b" => self.out.push_str("**"),
            "em" | "i" => self.out.push('*'),
            "del" | "s" | "strike" => self.out.push_str("~~"),
            "code" if self.preformatted == 0 => {
                self.code += 1;
                self.out.push('`');
            }
            "pre" => {
                self.block();
                self.preformatted += 1;
                self.out.push_str("```\n");
            }
            "ul" => {
                self.line();
                self.lists.push(List::Bullets);
            }
            "ol" => {
                self.line();
                let start = attribute("start")
                    .and_then(|start| start.parse().ok())
                    .unwrap_or(1);
                self.lists.push(List::Numbers(start));
            }
            "li" => {
                self.line();
                let depth = self.lists.len().saturating_sub(1);
                self.out.push_str(&"  ".repeat(depth));
                match self.lists.last_mut() {
                    Some(List::Numbers(next)) => {
                        let _ = write!(self.out, "{next}. ");
                        *next += 1;
                    }
                    _ => self.out.push_str("- "),
                }
            }
            "blockquote" => {
                self.block();
                self.quotes.push(self.out.len());
            }
            "a" => {
                let target = attribute("href")
                    .map(str::trim)
                    .filter(|href| SCHEMES.iter().any(|scheme| href.starts_with(scheme)))
                    .map(ToOwned::to_owned);
                if target.is_some() {
                    self.out.push('[');
                }
                self.links.push(target);
            }
            "img" => {
                if let Some(alt) = attribute("alt").filter(|alt| !alt.trim().is_empty()) {
                    self.text(alt);
                }
            }
            "td" | "th" => self.text(" "),
            _ if BLOCKS.contains(&name) => self.block(),
            _ => {}
        }
    }

    fn close(&mut self, name: &str) {
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" => {
                if matches!(name, "ul" | "ol") {
                    self.lists.pop();
                }
                self.block();
            }
            "strong" | "b" => self.out.push_str("**"),
            "em" | "i" => self.out.push('*'),
            "del" | "s" | "strike" => self.out.push_str("~~"),
            "code" if self.code > 0 => {
                self.code -= 1;
                self.out.push('`');
            }
            "pre" if self.preformatted > 0 => {
                self.preformatted -= 1;
                if !self.out.ends_with('\n') {
                    self.out.push('\n');
                }
                self.out.push_str("```");
                self.block();
            }
            "blockquote" => {
                if let Some(start) = self.quotes.pop() {
                    let quoted = self.out.split_off(start.min(self.out.len()));
                    let lines: Vec<String> = quoted
                        .trim_end()
                        .lines()
                        .map(|line| {
                            if line.is_empty() {
                                ">".to_owned()
                            } else {
                                format!("> {line}")
                            }
                        })
                        .collect();
                    self.out.push_str(&lines.join("\n"));
                }
                self.block();
            }
            "a" => {
                if let Some(Some(target)) = self.links.pop() {
                    let encoded = target.replace(' ', "%20").replace(')', "%29");
                    let _ = write!(self.out, "]({encoded})");
                }
            }
            _ if BLOCKS.contains(&name) => self.block(),
            _ => {}
        }
    }
}

/// `html` as Markdown: paragraphs, headings, emphasis, code, lists, quotes
/// and links to the web or mail kept; scripts, styles and every other
/// element left out with what they hold; and the text escaped so that
/// nothing in it reads as markup.
#[must_use]
pub fn markdown(html: &str) -> String {
    let mut writer = Writer::default();
    for token in tokens(html) {
        match token {
            Token::Text(text) => writer.text(text),
            Token::Open { name, attributes } => writer.open(&name, &attributes),
            Token::Close(name) => writer.close(&name),
        }
    }
    writer.out.trim().to_owned()
}

/// `html` as one line of plain text: its tags left out, its references
/// resolved, and its whitespace collapsed.
pub(crate) fn plain(html: &str) -> String {
    let text: Vec<String> = tokens(html)
        .into_iter()
        .filter_map(|token| match token {
            Token::Text(text) => Some(decoded(text).into_owned()),
            Token::Open { name, .. } | Token::Close(name) => {
                (name == "br" || BLOCKS.contains(&name.as_str())).then(|| " ".to_owned())
            }
        })
        .collect();
    text.concat()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
