//! `[store]` read into the store it names, with that store's options.

mod common;

#[cfg(test)]
mod stores {
    use herald_config::{Store, load};
    use herald_store_sqlite::StoreOptions;

    fn store(options: &str) -> Result<Store, String> {
        load(
            "herald.toml",
            &format!("version = 1\n[store]\nkind = \"sqlite\"\n{options}"),
            &|_| Err(String::new()),
            &crate::common::kinds(),
        )
        .map(|config| config.store)
        .map_err(|errors| errors.to_string())
    }

    #[test]
    fn a_sqlite_store_names_its_file_or_keeps_it_beside_the_config() {
        assert_eq!(store(""), Ok(Store::new(StoreOptions::default())));
        assert_eq!(
            store("path = \"/var/lib/herald/records.db\"\n"),
            Ok(Store::new(StoreOptions {
                key: None,
                path: Some("/var/lib/herald/records.db".to_owned()),
                busy: None,
            }))
        );
        let waiting = store("busy = \"30s\"\n").unwrap();
        let Some(options) = waiting.of::<herald_store_sqlite::StoreOptions>() else {
            panic!("expected a sqlite store, got {waiting:?}");
        };
        assert_eq!(options.busy(), jiff::SignedDuration::from_secs(30));
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
                &crate::common::kinds(),
            )
            .map_err(|errors| errors.to_string())
        };
        let named = config("url = { secret = \"HERALD_POSTGRES_URL\" }\n").unwrap();
        assert_eq!(
            named.store,
            Store::new(herald_store_postgres::StoreOptions {
                key: None,
                url: herald_core::SecretRef {
                    secret: "HERALD_POSTGRES_URL".to_owned()
                },
                connect_timeout: None,
                statement_timeout: None,
            })
        );
        let Some(defaults) = named.store.of::<herald_store_postgres::StoreOptions>() else {
            panic!("expected a postgres store");
        };
        assert_eq!(
            defaults.connect_timeout(),
            herald_store_postgres::CONNECT_TIMEOUT
        );
        assert_eq!(
            defaults.statement_timeout(),
            herald_store_postgres::STATEMENT_TIMEOUT
        );
        let timed = config(
            "url = { secret = \"HERALD_POSTGRES_URL\" }\nconnect_timeout = \"5s\"\nstatement_timeout = \"2m\"\n",
        )
        .unwrap();
        let Some(timed) = timed.store.of::<herald_store_postgres::StoreOptions>() else {
            panic!("expected a postgres store");
        };
        assert_eq!(timed.connect_timeout(), jiff::SignedDuration::from_secs(5));
        assert_eq!(
            timed.statement_timeout(),
            jiff::SignedDuration::from_mins(2)
        );
        assert!(named.secrets().contains(&"HERALD_POSTGRES_URL"));
        let missing = config("").unwrap_err();
        assert!(missing.contains("url"), "{missing}");
    }

    fn kind(kind: &str, options: &str) -> Result<herald_config::Config, String> {
        load(
            "herald.toml",
            &format!("version = 1\n[store]\nkind = \"{kind}\"\n{options}"),
            &|_| Err(String::new()),
            &crate::common::kinds(),
        )
        .map_err(|errors| errors.to_string())
    }

    #[test]
    fn each_serverless_store_takes_its_own_options_and_none_of_another_s() {
        assert_eq!(
            kind("d1", "binding = \"DB\"\n").unwrap().store,
            Store::new(herald_store_d1::StoreOptions {
                key: None,
                binding: Some("DB".to_owned())
            })
        );
        assert_eq!(
            kind("durable-object", "object = \"records\"\n")
                .unwrap()
                .store,
            Store::new(herald_store_durable_object::StoreOptions {
                key: None,
                binding: None,
                object: Some("records".to_owned())
            })
        );
        assert_eq!(
            kind("deno-kv", "prefix = \"apps/herald\"\n").unwrap().store,
            Store::new(herald_store_deno_kv::StoreOptions {
                key: None,
                path: None,
                prefix: Some("apps/herald".to_owned())
            })
        );
        assert_eq!(
            kind("fastly-kv", "store = \"records\"\n").unwrap().store,
            Store::new(herald_store_fastly_kv::StoreOptions {
                key: None,
                store: Some("records".to_owned()),
                prefix: None
            })
        );
        for (store, foreign) in [
            ("d1", "object = \"x\"\n"),
            ("durable-object", "prefix = \"x\"\n"),
            ("deno-kv", "binding = \"x\"\n"),
            ("fastly-kv", "table = \"x\"\n"),
            ("dynamodb", "store = \"x\"\n"),
        ] {
            assert!(kind(store, foreign).is_err(), "{store}");
        }
    }

    #[test]
    fn a_dynamodb_store_names_its_table_region_and_the_secrets_it_signs_with() {
        let config = kind(
            "dynamodb",
            "table = \"records\"\nregion = \"eu-west-1\"\nendpoint = \"http://localhost:8000\"\naccess_key_id = { secret = \"ID\" }\nsecret_access_key = { secret = \"KEY\" }\n",
        )
        .unwrap();
        let Some(options) = config.store.of::<herald_store_dynamodb::StoreOptions>() else {
            panic!("{:?}", config.store);
        };
        assert_eq!(
            (
                options.table(),
                options.region.as_deref(),
                options.endpoint.as_deref()
            ),
            ("records", Some("eu-west-1"), Some("http://localhost:8000"))
        );
        let secrets = config.secrets();
        assert!(secrets.contains(&"ID") && secrets.contains(&"KEY"));
        let unnamed = kind("dynamodb", "").unwrap();
        assert_eq!(
            unnamed.store,
            Store::new(herald_store_dynamodb::StoreOptions::default())
        );
        let hidden = unnamed.secrets();
        for name in [
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
        ] {
            assert!(hidden.contains(&name), "{name} is hidden");
        }
        assert_eq!(
            kind("markers", "").unwrap().store,
            Store::new(herald_store_markers::MarkersOptions {})
        );
    }
}
