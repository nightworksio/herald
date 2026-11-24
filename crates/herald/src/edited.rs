//! `herald.toml` edited in place: a table found or made, every other line
//! kept as it was.

use toml_edit::{DocumentMut, Item, Table, TableLike};

/// `text` read as a TOML document.
pub(crate) fn document(text: &str) -> Result<DocumentMut, String> {
    text.parse::<DocumentMut>()
        .map_err(|error| error.to_string())
}

/// The table `name` of `document`, made where it is absent, as a table
/// under its parent's header where `implicit`.
pub(crate) fn table_of<'d>(
    document: &'d mut Table,
    name: &str,
    implicit: bool,
) -> Result<&'d mut dyn TableLike, String> {
    let entry = document.entry(name).or_insert_with(|| {
        let mut table = Table::new();
        table.set_implicit(implicit);
        Item::Table(table)
    });
    entry
        .as_table_like_mut()
        .ok_or_else(|| format!("{name} is not written as a table; edit it by hand"))
}
