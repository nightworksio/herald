//! `[store]` read into the store it names, with that store's options.

#[cfg(test)]
mod stores {
    use herald_config::{Store, load};
    use herald_store_sqlite::StoreOptions;

    fn store(options: &str) -> Result<Store, String> {
        load(
            "herald.toml",
            &format!("version = 1\n[store]\nkind = \"sqlite\"\n{options}"),
            &|_| Err(String::new()),
        )
        .map(|config| config.store)
        .map_err(|errors| errors.to_string())
    }

    #[test]
    fn a_sqlite_store_names_its_file_or_keeps_it_beside_the_config() {
        assert_eq!(store(""), Ok(Store::Sqlite(StoreOptions::default())));
        assert_eq!(
            store("path = \"/var/lib/herald/records.db\"\n"),
            Ok(Store::Sqlite(StoreOptions {
                path: Some("/var/lib/herald/records.db".to_owned())
            }))
        );
        let unknown = store("file = \"x\"\n").unwrap_err();
        assert!(unknown.contains("store"), "{unknown}");
        assert!(unknown.contains("file"), "{unknown}");
    }
}
