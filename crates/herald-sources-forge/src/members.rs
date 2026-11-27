//! A member file: the list of repository names an owner source takes,
//! read from TOML or JSON (ADR-0030).

use herald_core::{position, unparsed};
use serde_json::Value;

/// The format a member file is written in, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// TOML, as `.toml`.
    Toml,
    /// JSON, as `.json`.
    Json,
}

impl Format {
    /// The format the file at `path` is written in, by its extension.
    ///
    /// # Errors
    ///
    /// When the extension is neither `.toml` nor `.json`.
    pub fn of(path: &str) -> Result<Self, String> {
        let extension = std::path::Path::new(path)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        if extension.eq_ignore_ascii_case("toml") {
            Ok(Self::Toml)
        } else if extension.eq_ignore_ascii_case("json") {
            Ok(Self::Json)
        } else {
            Err(format!(
                "\"{path}\" is neither .toml nor .json; name a member file in one of them"
            ))
        }
    }
}

/// The names the member file `text` at `path` lists: the `key` of each
/// entry of each array `tables` names, in order, or of the file's own
/// array where `tables` is empty, which only JSON can hold.
///
/// # Errors
///
/// When the file cannot be read in its format, a table it names is not an
/// array, or an entry has no `key` that is a string.
pub fn names(path: &str, text: &str, tables: &[String], key: &str) -> Result<Vec<String>, String> {
    let document: Value = match Format::of(path)? {
        Format::Toml => toml::from_str(text).map_err(|error: toml::de::Error| {
            let (line, column) = position(text, error.span().map_or(0, |span| span.start));
            unparsed(path, line, Some(column))
        })?,
        Format::Json => serde_json::from_str(text)
            .map_err(|error| unparsed(path, error.line(), Some(error.column())))?,
    };
    let arrays: Vec<(&str, &Value)> = if tables.is_empty() {
        vec![("the file", &document)]
    } else {
        tables
            .iter()
            .map(|table| (table.as_str(), document.get(table).unwrap_or(&Value::Null)))
            .collect()
    };
    let mut names = Vec::new();
    for (table, array) in arrays {
        let Some(entries) = array.as_array() else {
            return Err(format!(
                "{path}: {table} is not an array of entries; name the arrays that list the repositories with tables"
            ));
        };
        for (index, entry) in entries.iter().enumerate() {
            let Some(name) = entry.get(key).and_then(Value::as_str) else {
                return Err(format!(
                    "{path}: entry {index} of {table} has no {key}; give each entry its repository's name as {key}"
                ));
            };
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests;
