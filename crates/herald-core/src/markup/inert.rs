//! The oracle the markup property tests and fuzz targets share: whether a
//! converted text could mention anyone or link anywhere it does not show.

use crate::{Dialect, Text};

const BREAK: char = '\u{200B}';
const MENTIONS: [char; 3] = ['@', '\u{FF20}', '\u{FE6B}'];
const SCHEMES: [&str; 3] = ["https://", "http://", "mailto:"];

/// Why `text`, converted to `dialect`, is not inert.
pub fn inert(text: &Text, dialect: Dialect) -> Result<(), String> {
    match dialect {
        Dialect::Discord => discord(&text.body),
        Dialect::Slack => slack(&text.body),
        Dialect::Plain => plain(&text.body),
        Dialect::Matrix => {
            let markup = text.html.as_deref().unwrap_or_default();
            breaks(&outside_urls(&text.body), &MENTIONS)?;
            html(markup)?;
            hrefs_shown_in(markup, &text.body)
        }
    }
}

/// Why `text`, shown literally in `dialect`, could be read as anything but
/// its letters.
pub fn literal_inert(text: &Text, dialect: Dialect) -> Result<(), String> {
    inert(text, dialect)?;
    match dialect {
        Dialect::Discord => {
            let mut characters = text.body.chars();
            while let Some(character) = characters.next() {
                if character == '\\' {
                    characters.next();
                } else if character.is_ascii_punctuation() && character != '@' {
                    return Err(format!("{character:?} is unescaped in {:?}", text.body));
                }
            }
            Ok(())
        }
        Dialect::Slack if text.body.contains('<') => {
            Err(format!("a link or mention in {:?}", text.body))
        }
        Dialect::Matrix if text.html.as_deref().unwrap_or_default().contains('<') => {
            Err(format!("a tag in {:?}", text.html))
        }
        _ => Ok(()),
    }
}

fn breaks(text: &str, sigils: &[char]) -> Result<(), String> {
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if sigils.contains(&character) && characters.peek() != Some(&BREAK) {
            return Err(format!(
                "a {character} that could mention or tag in {text:?}"
            ));
        }
    }
    Ok(())
}

fn at_breaks(text: &str) -> Result<(), String> {
    breaks(text, &MENTIONS)
}

fn urls(text: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    for scheme in SCHEMES {
        for (start, _) in text.match_indices(scheme) {
            let rest = text.get(start..).unwrap_or_default();
            let length = rest
                .find(|character: char| character.is_whitespace() || "()".contains(character))
                .unwrap_or(rest.len());
            found.push((start, start + length));
        }
    }
    found.sort_unstable();
    found
}

fn outside_urls(text: &str) -> String {
    let mut outside = String::new();
    let mut at = 0;
    for (start, end) in urls(text) {
        if start >= at {
            outside.push_str(text.get(at..start).unwrap_or_default());
            outside.push(' ');
            at = end;
        }
    }
    outside.push_str(text.get(at..).unwrap_or_default());
    outside
}

fn hrefs_shown_in(markup: &str, body: &str) -> Result<(), String> {
    let shown: Vec<&str> = urls(body)
        .into_iter()
        .filter_map(|(start, end)| body.get(start..end))
        .collect();
    anchors(markup).into_iter().try_for_each(|href| {
        if shown.contains(&href.as_str()) {
            Ok(())
        } else {
            Err(format!(
                "the link {href} is not shown in the plain body {body:?}"
            ))
        }
    })
}

fn anchors(markup: &str) -> Vec<String> {
    markup
        .split('<')
        .skip(1)
        .filter_map(|tag| tag.strip_prefix("a href=\""))
        .filter_map(|rest| rest.split('"').next())
        .map(|href| href.replace("&amp;", "&"))
        .collect()
}

fn link_span(rest: &str) -> Option<usize> {
    let end = rest.find('>')?;
    let span = rest.get(1..end)?;
    let url = span.split('|').next().unwrap_or_default();
    let allowed = SCHEMES.iter().any(|scheme| url.starts_with(scheme));
    (allowed && !url.contains(char::is_whitespace)).then_some(end + 1)
}

fn discord(body: &str) -> Result<(), String> {
    let mut index = 0;
    while let Some(rest) = body.get(index..) {
        let Some(character) = rest.chars().next() else {
            break;
        };
        let width = character.len_utf8();
        let line_start = index == 0
            || body
                .get(..index)
                .is_some_and(|before| before.ends_with('\n'));
        if line_start && (rest.starts_with("-#") || rest.starts_with(">>>")) {
            return Err(format!("the line start of {rest:?} in {body:?}"));
        }
        match character {
            '\\' => {
                index += width
                    + rest
                        .get(width..)
                        .and_then(|after| after.chars().next())
                        .map_or(0, char::len_utf8);
            }
            '`' => {
                let fence = rest.chars().take_while(|tick| *tick == '`').count();
                let marker = "`".repeat(if fence >= 3 { 3 } else { fence });
                let inside = rest.get(marker.len()..).unwrap_or_default();
                let close = inside
                    .find(&marker)
                    .ok_or_else(|| format!("an unclosed code span in {body:?}"))?;
                at_breaks(inside.get(..close).unwrap_or_default())?;
                index += marker.len() * 2 + close;
            }
            '<' => {
                index += link_span(rest)
                    .ok_or_else(|| format!("a < that is no shown link in {body:?}"))?;
            }
            '[' | ']' => return Err(format!("a masked link bracket in {body:?}")),
            '@' | '\u{FF20}' | '\u{FE6B}' => {
                at_breaks(rest.get(..width + BREAK.len_utf8()).unwrap_or(rest))?;
                index += width;
            }
            _ => index += width,
        }
    }
    Ok(())
}

fn slack(body: &str) -> Result<(), String> {
    let mut index = 0;
    while let Some(rest) = body.get(index..) {
        let Some(character) = rest.chars().next() else {
            break;
        };
        if character == '<' {
            index +=
                link_span(rest).ok_or_else(|| format!("a < that is no shown link in {body:?}"))?;
        } else if character == '>' && !quote_marker(body, index) {
            return Err(format!("an unescaped > in {body:?}"));
        } else if character == '&'
            && !["&amp;", "&lt;", "&gt;"]
                .iter()
                .any(|entity| rest.starts_with(entity))
        {
            return Err(format!("an unescaped & in {body:?}"));
        } else {
            index += character.len_utf8();
        }
    }
    Ok(())
}

fn quote_marker(body: &str, index: usize) -> bool {
    let line = body
        .get(..index)
        .map_or("", |before| before.rsplit('\n').next().unwrap_or_default());
    let follows = body
        .get(index + 1..)
        .is_some_and(|after| after.is_empty() || after.starts_with([' ', '\n']));
    follows
        && line
            .chars()
            .all(|character| character == '>' || character == ' ')
}

fn plain(body: &str) -> Result<(), String> {
    breaks(&outside_urls(body), &['@', '\u{FF20}', '\u{FE6B}', '#'])
}

fn shows_href(after: &str, tag: &str) -> bool {
    let href = tag
        .strip_prefix("a href=\"")
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_default();
    let text = after
        .split("</a>")
        .next()
        .unwrap_or_default()
        .replace(BREAK, "");
    text == href
}

fn numbered(tag: &str) -> bool {
    tag.strip_prefix("ol start=\"")
        .and_then(|rest| rest.strip_suffix('"'))
        .is_some_and(|number| number.chars().all(|digit| digit.is_ascii_digit()))
}

fn html(markup: &str) -> Result<(), String> {
    const TAGS: [&str; 18] = [
        "p",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "strong",
        "em",
        "del",
        "code",
        "pre",
        "ul",
        "ol",
        "li",
        "blockquote",
        "hr",
        "br",
    ];
    let mut index = 0;
    while let Some(rest) = markup.get(index..).filter(|rest| !rest.is_empty()) {
        let Some(open) = rest.find('<') else {
            return at_breaks(rest);
        };
        at_breaks(rest.get(..open).unwrap_or_default())?;
        let close = rest
            .get(open..)
            .and_then(|tag| tag.find('>'))
            .ok_or_else(|| format!("an unclosed tag in {markup:?}"))?;
        let tag = rest.get(open + 1..open + close).unwrap_or_default();
        let name = tag
            .trim_start_matches('/')
            .split([' ', '/'])
            .next()
            .unwrap_or_default();
        let allowed = match name {
            "a" => {
                tag == "/a"
                    || (shows_href(rest.get(open + close + 1..).unwrap_or_default(), tag)
                        && tag.strip_prefix("a href=\"").is_some_and(|href| {
                            SCHEMES.iter().any(|scheme| href.starts_with(scheme))
                                && !href.starts_with("https://matrix.to/")
                        }))
            }
            "input" => {
                tag == "input disabled=\"\" type=\"checkbox\"/"
                    || tag == "input disabled=\"\" type=\"checkbox\" checked=\"\"/"
            }
            "code" => tag == "code" || tag == "/code" || tag.starts_with("code class=\"language-"),
            "ol" => tag == "ol" || tag == "/ol" || numbered(tag),
            _ => TAGS.contains(&name) && !tag.contains('='),
        };
        if !allowed {
            return Err(format!("the tag <{tag}> in {markup:?}"));
        }
        index += open + close + 1;
    }
    Ok(())
}
