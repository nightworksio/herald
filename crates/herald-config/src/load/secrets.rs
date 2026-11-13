//! The secrets the file names as `{ secret = "NAME" }`, each with the keys
//! that name it.

use std::collections::BTreeMap;

use toml::de::{DeTable, DeValue};

use super::Value;

/// Every secret `table` names as `{ secret = "NAME" }`, by its name, with
/// each key that names it.
pub(super) fn secret_keys(table: &DeTable<'_>) -> BTreeMap<String, Vec<String>> {
    let mut found = BTreeMap::new();
    for (key, value) in table {
        named_secrets(key.get_ref(), value, &mut found);
    }
    found
}

/// Each secret `value`, written at `path`, names, added to `found`.
fn named_secrets(path: &str, value: &Value<'_>, found: &mut BTreeMap<String, Vec<String>>) {
    match value.get_ref() {
        DeValue::Table(table) => {
            let mut entries = table.iter();
            if let (Some((key, child)), None) = (entries.next(), entries.next())
                && key.get_ref() == "secret"
                && let DeValue::String(name) = child.get_ref()
            {
                found
                    .entry(name.to_string())
                    .or_default()
                    .push(path.to_owned());
                return;
            }
            for (key, child) in table {
                named_secrets(&format!("{path}.{}", key.get_ref()), child, found);
            }
        }
        DeValue::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                named_secrets(&format!("{path}[{index}]"), item, found);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_naming_a_secret_is_found_in_tables_and_arrays() {
        let (root, _) = DeTable::parse_recoverable(
            "a = [{ secret = \"X\" }]\n[b]\nc = { secret = \"X\" }\nd = { secret = 1 }\ne = { secret = \"Y\", other = 1 }\n",
        );
        assert_eq!(
            secret_keys(root.get_ref()),
            [("X".to_owned(), vec!["a[0]".to_owned(), "b.c".to_owned()])].into()
        );
    }
}
