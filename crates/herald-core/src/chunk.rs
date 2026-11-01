use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag};
use unicode_segmentation::UnicodeSegmentation;

use crate::markup::{EXTENSIONS, Text, convert};
use crate::profile::{Dialect, Measure};

/// How long each part of a chunked text may be, and how length is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Room {
    /// How length is counted.
    pub measure: Measure,
    /// The longest first part.
    pub first: usize,
    /// The longest of every later part.
    pub rest: usize,
}

/// `markdown` in `dialect`, cut into parts that each fit `room`: at a block
/// where it can, then at a line, a word and a grapheme. A code block cut in two
/// is fenced again on both sides. There is always at least one part.
#[must_use]
pub fn chunk(markdown: &str, dialect: Dialect, room: Room) -> Vec<Text> {
    let fits = |text: &str, limit: usize| size(&convert(text, dialect), room.measure) <= limit;
    let narrowest = room.first.min(room.rest);
    let pieces: Vec<String> = blocks(markdown)
        .into_iter()
        .flat_map(|block| {
            if fits(&block, narrowest) {
                vec![block]
            } else if let Some((open, close, code)) = fenced(&block) {
                let open = if fits(&format!("{open}\n{close}"), narrowest) {
                    open
                } else {
                    close.clone()
                };
                split(
                    &code,
                    &|piece| fits(&format!("{open}\n{piece}\n{close}"), narrowest),
                    Level::Lines,
                )
                .into_iter()
                .map(|piece| format!("{open}\n{piece}\n{close}"))
                .collect()
            } else {
                split(&block, &|piece| fits(piece, narrowest), Level::Lines)
            }
        })
        .collect();
    let mut parts: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for piece in pieces {
        let limit = if parts.is_empty() {
            room.first
        } else {
            room.rest
        };
        current = Some(match current {
            Some(text) if fits(&format!("{text}\n\n{piece}"), limit) => {
                format!("{text}\n\n{piece}")
            }
            Some(text) => {
                parts.push(text);
                piece
            }
            None => piece,
        });
    }
    parts.extend(current);
    if parts.is_empty() {
        parts.push(String::new());
    }
    parts.iter().map(|part| convert(part, dialect)).collect()
}

/// The length of `text` as `measure` counts it, its HTML included.
#[must_use]
pub fn size(text: &Text, measure: Measure) -> usize {
    let count = |text: &str| match measure {
        Measure::Chars => text.chars().count(),
        Measure::Bytes => text.len(),
    };
    count(&text.body) + text.html.as_deref().map_or(0, count)
}

fn blocks(markdown: &str) -> Vec<String> {
    let mut depth = 0usize;
    let mut start = None;
    let mut blocks = Vec::new();
    for (event, range) in Parser::new_ext(markdown, EXTENSIONS).into_offset_iter() {
        match event {
            Event::Start(_) => {
                if depth == 0 {
                    start = Some(range.start);
                }
                depth += 1;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    blocks.extend(
                        start
                            .take()
                            .and_then(|start| markdown.get(start..range.end)),
                    );
                }
            }
            _ => {
                if depth == 0 {
                    blocks.extend(markdown.get(range));
                }
            }
        }
    }
    blocks
        .into_iter()
        .map(str::trim)
        .filter(|block| !block.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn fenced(block: &str) -> Option<(String, String, String)> {
    let first = Parser::new_ext(block, EXTENSIONS).next();
    if !matches!(
        first,
        Some(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))))
    ) {
        return None;
    }
    let (open, rest) = block.split_once('\n')?;
    let marker = ["```", "~~~"]
        .into_iter()
        .find(|marker| open.starts_with(marker))?;
    let code = rest
        .strip_suffix(marker)
        .unwrap_or(rest)
        .trim_end_matches('\n');
    Some((open.to_owned(), marker.to_owned(), code.to_owned()))
}

#[derive(Clone, Copy)]
enum Level {
    Lines,
    Words,
    Graphemes,
    Chars,
}

impl Level {
    fn units(self, text: &str) -> Vec<&str> {
        match self {
            Self::Lines => text.split('\n').collect(),
            Self::Words => text.split(' ').collect(),
            Self::Graphemes => text.graphemes(true).collect(),
            Self::Chars => text
                .char_indices()
                .filter_map(|(index, character)| text.get(index..index + character.len_utf8()))
                .collect(),
        }
    }

    fn separator(self) -> &'static str {
        match self {
            Self::Lines => "\n",
            Self::Words => " ",
            Self::Graphemes | Self::Chars => "",
        }
    }

    fn finer(self) -> Option<Self> {
        match self {
            Self::Lines => Some(Self::Words),
            Self::Words => Some(Self::Graphemes),
            Self::Graphemes => Some(Self::Chars),
            Self::Chars => None,
        }
    }
}

fn split(text: &str, fits: &dyn Fn(&str) -> bool, level: Level) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current: Option<String> = None;
    for unit in level.units(text) {
        let joined = current
            .as_ref()
            .map(|text| format!("{text}{}{unit}", level.separator()));
        match (current.take(), joined) {
            (Some(_), Some(joined)) if fits(&joined) => current = Some(joined),
            (previous, _) => {
                pieces.extend(previous);
                match level.finer().filter(|_| !fits(unit)) {
                    Some(finer) => {
                        let mut finer_pieces = split(unit, fits, finer);
                        current = finer_pieces.pop();
                        pieces.extend(finer_pieces);
                    }
                    None => current = Some(unit.to_owned()),
                }
            }
        }
    }
    pieces.extend(current);
    pieces
}

#[cfg(test)]
mod tests;
