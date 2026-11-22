//! A CODEOWNERS file as herald read it, kept in the store beside the pull
//! requests with the validator its forge answered with, so a file
//! unchanged since is neither downloaded nor read again (ADR-0074).

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::{Codeowners, Dialect, MOST_CODEOWNERS_LINES, Rule, Skipped, compiled};
use crate::SourceName;

/// The form a kept read is written in: a record of another form is read
/// again in full.
pub const KEPT_CODEOWNERS: u32 = 1;

/// Which file a kept read is of: a source's, of a repository by its full
/// path in lower case, at a branch or, where none, the default branch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CodeownersKey {
    /// The source that read it.
    pub source: SourceName,
    /// The repository's full path, in lower case.
    pub repository: String,
    /// The branch it was read at, or none for the default branch.
    pub reference: Option<String>,
}

/// A CODEOWNERS file as herald read it, with the validator its forge
/// answered with and the bounds it was read under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptCodeowners {
    /// Which file it is.
    pub key: CodeownersKey,
    /// The form it is written in: [`KEPT_CODEOWNERS`] where this herald
    /// wrote it.
    pub form: u32,
    /// The place it was found at.
    pub path: String,
    /// The validator its forge answered it with.
    pub etag: String,
    /// When herald last read it in full.
    pub at: Timestamp,
    /// The most bytes and the most areas it was read under.
    pub most: (usize, usize),
    file: Form,
}

/// A read file's lines, the patterns they match by, and what was skipped
/// or refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Form {
    dialect: Dialect,
    rules: Vec<Rule>,
    patterns: Vec<String>,
    sections: Vec<String>,
    skipped: Vec<Skipped>,
    refused: Option<String>,
}

impl KeptCodeowners {
    /// `file`, found at `place.0` and answered with the validator
    /// `place.1`, read under `read.0` at `read.1`, as kept under `key`:
    /// none where its record is longer than the bytes of the file it was
    /// read under.
    #[must_use]
    pub fn of(
        key: CodeownersKey,
        (path, etag): (&str, &str),
        file: &Codeowners,
        (most, at): ((usize, usize), Timestamp),
    ) -> Option<Self> {
        let kept = Self {
            key,
            form: KEPT_CODEOWNERS,
            path: path.to_owned(),
            etag: etag.to_owned(),
            at,
            most,
            file: file.form(),
        };
        serde_json::to_vec(&kept)
            .is_ok_and(|written| written.len() <= most.0)
            .then_some(kept)
    }

    /// Whether it was read as a file of `dialect` is read under `most`, in
    /// the form this herald writes: only then is its validator asked with.
    #[must_use]
    pub fn fits(&self, dialect: Dialect, most: (usize, usize)) -> bool {
        self.form == KEPT_CODEOWNERS && self.file.dialect == dialect && self.most == most
    }

    /// The file as herald read it: none where its lines and patterns do
    /// not pair, it holds more lines than a file is read with, the
    /// patterns no longer compile together, or it names more areas than
    /// it was read under.
    #[must_use]
    pub fn file(&self) -> Option<Codeowners> {
        let form = self.file.clone();
        if form.rules.len() != form.patterns.len() || form.rules.len() > MOST_CODEOWNERS_LINES {
            return None;
        }
        let set = if form.patterns.is_empty() {
            None
        } else {
            Some(compiled(&form.patterns).ok()?)
        };
        let file = Codeowners {
            dialect: form.dialect,
            rules: form.rules,
            set,
            sections: form.sections,
            skipped: form.skipped,
            refused: form.refused,
        };
        (file.named() <= self.most.1).then_some(file)
    }
}

impl Codeowners {
    /// What of it is kept.
    fn form(&self) -> Form {
        Form {
            dialect: self.dialect,
            rules: self.rules.clone(),
            patterns: self
                .set
                .as_ref()
                .map(|set| set.patterns().to_vec())
                .unwrap_or_default(),
            sections: self.sections.clone(),
            skipped: self.skipped.clone(),
            refused: self.refused.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
