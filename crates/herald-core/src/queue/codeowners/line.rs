//! A CODEOWNERS line split into its words, and the owners and GitLab
//! section headings those words write.

use super::{Dialect, MOST_LINE_OWNERS};
use crate::queue::area::unnamed;

/// An owner a CODEOWNERS line names, by what it names, as its forge writes
/// it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CodeOwner {
    /// A team or a group: `@org/team` on GitHub, Forgejo and Gitea,
    /// `@group/subgroup` on GitLab.
    Team(String),
    /// A person by login: `@login` on GitHub, Forgejo and Gitea.
    Person(String),
    /// A GitLab `@name`: a person or a top-level group, which the file
    /// alone does not tell apart.
    Either(String),
    /// A person by email address.
    Email(String),
    /// A GitLab role, `@@developer`.
    Role(String),
}

impl CodeOwner {
    /// The owner as its forge writes it; a Forgejo owner written without
    /// its `@` gains one.
    #[must_use]
    pub fn written(&self) -> &str {
        match self {
            Self::Team(written)
            | Self::Person(written)
            | Self::Either(written)
            | Self::Email(written)
            | Self::Role(written) => written,
        }
    }
}

/// The words of `line`, split at spaces and tabs a `\` does not escape.
/// GitHub and GitLab keep each `\` in its word for the pattern to read;
/// Forgejo drops it and ends the line at the first unescaped `#`, as
/// Forgejo's own reading does.
pub(super) fn words(line: &str, dialect: Dialect) -> Vec<String> {
    let forgejo = dialect == Dialect::Forgejo;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            word.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
            if !forgejo {
                word.push(character);
            }
        } else if forgejo && character == '#' {
            break;
        } else if character == ' ' || character == '\t' {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(character);
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Whether `word` is an email address: something, an `@`, and a domain.
fn email(word: &str) -> bool {
    word.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty() && !domain.is_empty() && !domain.contains('@') && !word.contains('/')
    })
}

/// Whether each `/`-separated part of `name` is written.
fn parted(name: &str) -> bool {
    name.split('/').all(|part| !part.is_empty())
}

/// The owner `word` names on `dialect`'s forge, where it names one that
/// can be an area's name.
pub(super) fn owner(word: &str, dialect: Dialect) -> Option<CodeOwner> {
    if unnamed(word).is_some() {
        return None;
    }
    match dialect {
        Dialect::GitHub => github(word),
        Dialect::GitLab => gitlab(word),
        Dialect::Forgejo => forgejo(word),
    }
}

fn github(word: &str) -> Option<CodeOwner> {
    let Some(name) = word.strip_prefix('@') else {
        return email(word).then(|| CodeOwner::Email(word.to_owned()));
    };
    if name.is_empty() || name.contains('@') || !parted(name) {
        return None;
    }
    match name.matches('/').count() {
        0 => Some(CodeOwner::Person(word.to_owned())),
        1 => Some(CodeOwner::Team(word.to_owned())),
        _ => None,
    }
}

fn gitlab(word: &str) -> Option<CodeOwner> {
    if let Some(role) = word.strip_prefix("@@") {
        return (!role.is_empty() && !role.contains(['@', '/']))
            .then(|| CodeOwner::Role(word.to_owned()));
    }
    let Some(name) = word.strip_prefix('@') else {
        return email(word).then(|| CodeOwner::Email(word.to_owned()));
    };
    if name.is_empty() || name.contains('@') || !parted(name) {
        None
    } else if name.contains('/') {
        Some(CodeOwner::Team(word.to_owned()))
    } else {
        Some(CodeOwner::Either(word.to_owned()))
    }
}

fn forgejo(word: &str) -> Option<CodeOwner> {
    let name = word.strip_prefix('@').unwrap_or(word);
    if name.is_empty() || name.contains('@') || !parted(name) {
        return None;
    }
    let written = format!("@{name}");
    match name.matches('/').count() {
        0 => Some(CodeOwner::Person(written)),
        1 => Some(CodeOwner::Team(written)),
        _ => None,
    }
}

/// The owners `words` name on `dialect`'s forge, past the pattern: on
/// GitHub up to a word starting with `#`, which begins a comment.
///
/// # Errors
///
/// Where the line names more than [`MOST_LINE_OWNERS`] owners, or, on
/// GitHub, writes an owner GitHub does not read: GitHub skips such a
/// line. GitLab and Forgejo skip only the owner.
pub(super) fn owners(words: &[String], dialect: Dialect) -> Result<Vec<CodeOwner>, String> {
    let written: Vec<&String> = words
        .iter()
        .take_while(|word| dialect != Dialect::GitHub || !word.starts_with('#'))
        .collect();
    if written.len() > MOST_LINE_OWNERS {
        return Err(format!(
            "it names {} owners, and at most {MOST_LINE_OWNERS} are read",
            written.len()
        ));
    }
    let mut owners = Vec::with_capacity(written.len());
    for word in written {
        match owner(word, dialect) {
            Some(owner) => owners.push(owner),
            None if dialect == Dialect::GitHub => {
                return Err(format!(
                    "\"{}\" is no owner GitHub reads: write @login, @org/team or an email address",
                    word.escape_debug()
                ));
            }
            None => {}
        }
    }
    Ok(owners)
}

/// A GitLab section heading, read as GitLab's `SectionParser::HEADER_REGEX`
/// reads one: `[Name]`, `^[Name]` for an optional section, an approvals
/// count `[2]` of digits and spaces only, and then default owners, as far
/// as `@`, word characters, `.`, `-`, `/` and spaces run.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Heading {
    /// The section's name, as written up to its first `]`.
    pub(super) name: String,
    /// The owners an entry naming none takes.
    pub(super) owners: Vec<CodeOwner>,
}

/// The heading `line` writes, where it starts as one does.
///
/// # Errors
///
/// Where its name never closes, which GitLab does not read as a heading
/// and reads the entries after it as the previous section's, or it names
/// more than [`MOST_LINE_OWNERS`] default owners.
pub(super) fn heading(line: &str) -> Option<Result<Heading, String>> {
    let opened = line.strip_prefix('^').unwrap_or(line).strip_prefix('[')?;
    Some(headed(opened))
}

fn headed(opened: &str) -> Result<Heading, String> {
    let Some((name, after)) = opened.split_once(']') else {
        return Err("its section heading never closes with \"]\"".to_owned());
    };
    let after = match after
        .strip_prefix('[')
        .and_then(|counted| counted.split_once(']'))
    {
        Some((count, rest))
            if count
                .chars()
                .all(|character| character.is_ascii_digit() || character.is_ascii_whitespace()) =>
        {
            rest
        }
        _ => after,
    };
    let defaults = after
        .split(|character: char| {
            !(character.is_ascii_alphanumeric()
                || character.is_ascii_whitespace()
                || matches!(character, '@' | '_' | '.' | '-' | '/'))
        })
        .next()
        .unwrap_or(after);
    Ok(Heading {
        name: name.to_owned(),
        owners: owners(&words(defaults, Dialect::GitLab), Dialect::GitLab)?,
    })
}

#[cfg(test)]
mod tests;
