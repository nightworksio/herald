//! `herald preset`: the tables a `[presets.<name>]` table expands to,
//! printed, or written into the config in its place (ADR-0092 decision 5).

use std::io::Write;
use std::path::Path;

use tokio::io::AsyncBufRead;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

use crate::asking::Asking;
use crate::cli::Presetting;
use crate::{Beside, DONE, STOPPED};

/// What `herald preset` was asked, answered against the config at `path`.
pub(crate) async fn presetting(
    path: &str,
    asked: &Presetting,
    answers: &mut (dyn AsyncBufRead + Send + Unpin),
    out: &mut dyn Write,
) -> Result<u8, String> {
    let text = crate::text(path)?;
    let name = match asked {
        Presetting::Show { name } | Presetting::Eject { name, .. } => name,
    };
    let expanded = expanded(path, &text, name)?;
    let Presetting::Eject { yes, print, .. } = asked else {
        let _ = write!(out, "{expanded}");
        return Ok(DONE);
    };
    let ejected = ejected(&text, name, &expanded)?;
    crate::loaded(path, &ejected).map_err(|why| format!("{why}\nNothing was written."))?;
    if *print {
        let _ = write!(out, "{ejected}");
        return Ok(DONE);
    }
    let mut asking = Asking {
        answers,
        out,
        yes: *yes,
    };
    let _ = write!(asking.out, "{expanded}");
    if !asking
        .confirmed(&format!(
            "Write these tables into {path} in place of [presets.{name}]?"
        ))
        .await
    {
        asking.say("Nothing was written.");
        return Ok(STOPPED);
    }
    crate::rewrite(path, ejected)?;
    asking.say(&format!(
        "Wrote the tables [presets.{name}] expanded to into {path} in its place."
    ));
    Ok(DONE)
}

/// The tables the config `text` at `path` expands `[presets.<name>]` to,
/// as `herald.toml` would write them.
fn expanded(path: &str, text: &str, name: &str) -> Result<String, String> {
    let directory = Path::new(path).parent().unwrap_or(Path::new(""));
    let tables = herald_config::expansion(path, text, &Beside { directory }, name)
        .map_err(|errors| errors.to_string())?;
    toml::to_string(&tables).map_err(|error| error.to_string())
}

/// The config `text` with `[presets.<name>]` taken out and the tables it
/// expanded to, `expanded`, written in its place.
fn ejected(text: &str, name: &str, expanded: &str) -> Result<String, String> {
    let mut document: DocumentMut = text.parse().map_err(|error| format!("{error}"))?;
    let tables: DocumentMut = expanded.parse().map_err(|error| format!("{error}"))?;
    if let Some(presets) = document
        .get_mut("presets")
        .and_then(Item::as_table_like_mut)
    {
        presets.remove(name);
        if presets.is_empty() {
            document.remove("presets");
        }
    }
    for (section, item) in tables.iter() {
        match item {
            Item::ArrayOfTables(written) => {
                let routes = document
                    .entry(section)
                    .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()));
                if let Some(routes) = routes.as_array_of_tables_mut() {
                    written.iter().for_each(|route| routes.push(route.clone()));
                }
            }
            Item::Table(written) => {
                let named = document.entry(section).or_insert_with(|| {
                    let mut named = Table::new();
                    named.set_implicit(true);
                    Item::Table(named)
                });
                if let Some(named) = named.as_table_like_mut() {
                    for (key, table) in written {
                        named.insert(key, table.clone());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(document.to_string())
}
