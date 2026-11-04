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

    #[test]
    fn a_postgres_store_reads_its_connection_url_from_a_secret_it_names() {
        let config = |options: &str| {
            load(
                "herald.toml",
                &format!("version = 1\n[store]\nkind = \"postgres\"\n{options}"),
                &|_| Err(String::new()),
            )
            .map_err(|errors| errors.to_string())
        };
        let named = config("url = { secret = \"HERALD_POSTGRES_URL\" }\n").unwrap();
        assert_eq!(
            named.store,
            Store::Postgres(herald_store_postgres::StoreOptions {
                url: herald_core::SecretRef {
                    secret: "HERALD_POSTGRES_URL".to_owned()
                }
            })
        );
        assert!(named.secrets().contains(&"HERALD_POSTGRES_URL"));
        let missing = config("").unwrap_err();
        assert!(missing.contains("url"), "{missing}");
    }
}
