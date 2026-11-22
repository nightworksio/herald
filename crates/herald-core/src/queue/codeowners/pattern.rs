//! A CODEOWNERS pattern as the regular expression a path is matched by:
//! GitHub's gitignore-style patterns and GitLab's `fnmatch` patterns are
//! translated, a Forgejo pattern already is one.

use std::fmt::Write as _;
use std::str::Chars;

/// Any run of characters within one path segment.
const SEGMENT: &str = "[^/]*";
/// Any one character within one path segment.
const ONE: &str = "[^/]";
/// Zero or more whole directories, each ending in `/`.
const DIRECTORIES: &str = "(?:.*/)?";

fn literal(out: &mut String, character: char) {
    let mut buffer = [0; 4];
    out.push_str(&regex::escape(character.encode_utf8(&mut buffer)));
}

/// `segment`'s `*`, `?`, `[...]` classes and `\\`-escaped characters, as
/// a regular expression none of whose parts crosses a `/`.
fn segment(out: &mut String, segment: &str) {
    let mut characters = segment.chars();
    while let Some(character) = characters.next() {
        match character {
            '*' => out.push_str(SEGMENT),
            '?' => out.push_str(ONE),
            '\\' => literal(out, characters.next().unwrap_or('\\')),
            '[' => {
                let mut within = characters.clone();
                match class(&mut within) {
                    Some(written) => {
                        out.push_str(&written);
                        characters = within;
                    }
                    None => literal(out, '['),
                }
            }
            other => literal(out, other),
        }
    }
}

/// The class `characters` hold up to its `]`, as `fnmatch` reads one:
/// `!` or `^` first negates it, a `]` first is one of its members, `a-z`
/// is a range, and `\\` escapes; it never takes a `/`. None where it never
/// closes.
fn class(characters: &mut Chars<'_>) -> Option<String> {
    let mut ahead = characters.clone();
    let negated = matches!(ahead.next(), Some('!' | '^'));
    if negated {
        *characters = ahead;
    }
    let mut members = String::new();
    let mut first = true;
    loop {
        let mut character = characters.next()?;
        if character == ']' && !first {
            break;
        }
        first = false;
        if character == '\\' {
            character = characters.next()?;
        }
        let _ = write!(members, "\\x{{{:X}}}", u32::from(character));
        let mut ahead = characters.clone();
        if let (Some('-'), Some(end)) = (ahead.next(), ahead.next())
            && end != ']'
        {
            let _ = write!(members, "-\\x{{{:X}}}", u32::from(end));
            *characters = ahead;
        }
    }
    Some(if negated {
        format!("[^{members}/]")
    } else {
        format!("[[{members}]&&[^/]]")
    })
}

/// The regular expression GitHub's `pattern` matches a path by, following
/// gitignore: a `/` first or within anchors it at the root, and otherwise
/// it matches at any depth; `**` stands for any directories; a `/` last
/// takes everything within; a last segment `*` takes only the files
/// directly within; any other pattern takes a path it names and
/// everything within it.
///
/// # Errors
///
/// Where it starts with `!`, `\#` or holds `[`, which GitHub does not
/// read, names no path, or holds an empty segment.
pub(super) fn github(pattern: &str) -> Result<String, String> {
    if pattern.starts_with('!') {
        return Err("GitHub does not read a pattern starting with \"!\"".to_owned());
    }
    if pattern.starts_with("\\#") {
        return Err("GitHub does not read a pattern starting with \"\\#\"".to_owned());
    }
    if pattern.contains('[') {
        return Err("GitHub does not read \"[ ]\" in a pattern".to_owned());
    }
    let (rooted, body) = pattern
        .strip_prefix('/')
        .map_or((false, pattern), |body| (true, body));
    let (directory, body) = body
        .strip_suffix('/')
        .map_or((false, body), |body| (true, body));
    let segments: Vec<&str> = body.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err("the pattern names no path, or holds \"//\"".to_owned());
    }
    let mut out = String::from(if rooted || segments.len() > 1 {
        "(?s)^"
    } else {
        "(?s)^(?:.*/)?"
    });
    let last = segments.len() - 1;
    let mut joined = false;
    for (at, written) in segments.iter().enumerate() {
        if *written == "**" {
            match (at == 0, at == last) {
                (true, true) => out.push_str(".*"),
                (false, true) => out.push_str("/.*"),
                (true, false) => out.push_str(DIRECTORIES),
                (false, false) => out.push_str("(?:/.*)?"),
            }
            if at == last {
                out.push('$');
                return Ok(out);
            }
            joined = at != 0;
            continue;
        }
        if joined {
            out.push('/');
        }
        segment(&mut out, written);
        joined = true;
    }
    if directory {
        out.push_str("/.*");
    } else if segments.last() != Some(&"*") {
        out.push_str("(?:/.*)?");
    }
    out.push('$');
    Ok(out)
}

/// The regular expression GitLab's `pattern` matches a path by, as GitLab
/// reads it: `*` alone is every path, a pattern not starting with `/` is
/// matched at any depth, one ending in `/` takes everything within, and
/// the rest is `fnmatch` with `FNM_PATHNAME` and `FNM_DOTMATCH`: `*`, `?`
/// and classes stay within a segment, and `**/` is any directories.
pub(super) fn gitlab(pattern: &str) -> String {
    let mut normal = if pattern == "*" {
        "**/*".to_owned()
    } else {
        pattern
            .strip_prefix('/')
            .map_or_else(|| format!("**/{pattern}"), str::to_owned)
    };
    if normal.ends_with('/') {
        normal.push_str("**/*");
    }
    let mut out = String::from("(?s)^");
    let mut segments = normal.split('/').peekable();
    while let Some(written) = segments.next() {
        let within = segments.peek().is_some();
        if within && written == "**" {
            out.push_str(DIRECTORIES);
        } else {
            segment(&mut out, written);
            if within {
                out.push('/');
            }
        }
    }
    out.push('$');
    out
}

/// The regular expression Forgejo matches a path by: the pattern between
/// `^` and `$`, ungrouped, as Forgejo's own reading writes it.
pub(super) fn forgejo(pattern: &str) -> String {
    format!("^{pattern}$")
}

#[cfg(test)]
mod tests;
