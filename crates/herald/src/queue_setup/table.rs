//! The `[merge_queues.<name>]` table the wizard writes into herald.toml,
//! in place of the one of that name, every other line kept as it was.

use toml_edit::{Array, InlineTable, Item, Table, Value, value};

use crate::edited::{document, table_of};

/// The table every merge queue sits under.
const QUEUES: &str = "merge_queues";

/// A queue's keys, in the order they are written.
pub(super) type Keys = Vec<(&'static str, Item)>;

/// `text` as a key's value.
pub(super) fn text(text: &str) -> Item {
    value(text)
}

/// `names` as a key's list.
pub(super) fn listed(names: &[String]) -> Value {
    Value::Array(names.iter().collect::<Array>())
}

/// `pairs` as one inline table.
pub(super) fn inline(pairs: Vec<(&str, Value)>) -> Value {
    Value::InlineTable(pairs.into_iter().collect::<InlineTable>())
}

/// The config `text` with `keys` as the merge queue `name`, its table
/// emptied and refilled in place where it was written already.
pub(super) fn placed(text: &str, name: &str, keys: &Keys) -> Result<String, String> {
    let mut written = document(text)?;
    let queues = table_of(written.as_table_mut(), QUEUES, true)?;
    let mut fresh = Table::new();
    let table = match queues.get_mut(name).and_then(Item::as_table_mut) {
        Some(table) => {
            table.clear();
            table
        }
        None => &mut fresh,
    };
    for (key, item) in keys {
        table.insert(key, item.clone());
    }
    if !fresh.is_empty() {
        queues.insert(name, Item::Table(fresh));
    }
    Ok(written.to_string())
}

/// `keys` as the table of the merge queue `name`, alone.
pub(super) fn shown(name: &str, keys: &Keys) -> String {
    placed("", name, keys).unwrap_or_default()
}
