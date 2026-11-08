//! An ActivityPub actor's document read for the key a signature names.

#[cfg(test)]
mod actor {
    use herald_dest_fediverse::{
        ActorError, ActorKey, Found, Inboxes, activity_json, actor_key, actor_of, confirms,
        document, inboxes, key_request, origin, same_origin,
    };
    use serde_json::json;

    const KEY: &str = "https://social.example/users/ana#main-key";
    const ANA: &str = "https://social.example/users/ana";
    const ALONE: &str = "https://social.example/users/ana/main-key";
    const PEM: &str = "-----BEGIN PUBLIC KEY-----\nAQAB\n-----END PUBLIC KEY-----\n";

    fn read(document: &serde_json::Value, key: &str) -> Result<Found, ActorError> {
        actor_key(document.to_string().as_bytes(), key)
    }

    fn held() -> Found {
        Found::Held(ActorKey {
            actor: ANA.to_owned(),
            pem: PEM.to_owned(),
        })
    }

    #[test]
    fn an_actor_s_own_document_lists_the_key_it_holds() {
        let actor = json!({
            "id": ANA,
            "type": "Person",
            "publicKey": { "id": KEY, "owner": ANA, "publicKeyPem": PEM },
        });
        assert_eq!(read(&actor, KEY), Ok(held()));
        let unowned = json!({ "id": ANA, "type": "Service", "publicKey": { "id": KEY, "publicKeyPem": PEM } });
        assert_eq!(read(&unowned, KEY), Ok(held()));
        let untyped = json!({ "id": ANA, "publicKey": { "id": KEY, "publicKeyPem": PEM } });
        assert_eq!(read(&untyped, KEY), Err(ActorError::Misnamed));
        let note =
            json!({ "id": ANA, "type": "Note", "publicKey": { "id": KEY, "publicKeyPem": PEM } });
        assert_eq!(read(&note, KEY), Err(ActorError::Misnamed));
        let listed = json!({
            "id": ANA,
            "type": "Person",
            "publicKey": [
                { "id": "https://social.example/users/ana#old", "owner": ANA, "publicKeyPem": "old" },
                { "id": KEY, "owner": { "id": ANA }, "publicKeyPem": PEM },
            ],
        });
        assert_eq!(read(&listed, KEY), Ok(held()));
    }

    #[test]
    fn an_actor_s_document_cannot_hand_its_key_to_another_actor() {
        let given = json!({
            "id": ANA,
            "type": "Person",
            "publicKey": { "id": KEY, "owner": "https://social.example/users/bob", "publicKeyPem": PEM },
        });
        assert_eq!(read(&given, KEY), Err(ActorError::Misnamed));
        let moved = json!({
            "id": "https://social.example/users/bob",
            "type": "Person",
            "publicKey": { "id": KEY, "owner": ANA, "publicKeyPem": PEM },
        });
        assert_eq!(read(&moved, KEY), Err(ActorError::Misnamed));
        let anonymous = json!({ "publicKey": { "id": KEY, "publicKeyPem": PEM } });
        assert_eq!(read(&anonymous, KEY), Err(ActorError::Misnamed));
    }

    #[test]
    fn a_key_s_own_document_claims_an_owner_whose_document_must_list_it() {
        let alone = json!({ "id": ALONE, "owner": ANA, "publicKeyPem": PEM });
        assert_eq!(
            read(&alone, ALONE),
            Ok(Found::Claimed {
                owner: ANA.to_owned(),
                pem: PEM.to_owned()
            })
        );
        let ownerless = json!({ "id": ALONE, "publicKeyPem": PEM });
        assert_eq!(read(&ownerless, ALONE), Err(ActorError::NoKey));
        let pemless = json!({ "id": ALONE, "owner": ANA });
        assert_eq!(read(&pemless, ALONE), Err(ActorError::NoKey));
        let owner = |keys: serde_json::Value| {
            json!({ "id": ANA, "type": "Person", "publicKey": keys }).to_string()
        };
        let untyped = json!({ "id": ANA, "publicKey": { "id": ALONE } }).to_string();
        assert!(!confirms(untyped.as_bytes(), ANA, ALONE));
        let keyless = json!({ "id": ANA, "type": "Person" }).to_string();
        assert!(!confirms(keyless.as_bytes(), ANA, ALONE));
        let foreign = json!({ "id": ALONE, "owner": "https://victim.example/users/admin", "publicKeyPem": PEM });
        assert_eq!(read(&foreign, ALONE), Err(ActorError::Misnamed));
        let listing = owner(json!({ "id": ALONE, "owner": ANA }));
        assert!(confirms(listing.as_bytes(), ANA, ALONE));
        let unowned = owner(json!([{ "id": "other" }, { "id": ALONE }]));
        assert!(confirms(unowned.as_bytes(), ANA, ALONE));
        let other = owner(json!({ "id": ALONE, "owner": "https://social.example/users/bob" }));
        assert!(!confirms(other.as_bytes(), ANA, ALONE));
        let unlisted = owner(json!({ "id": "https://social.example/users/ana#other" }));
        assert!(!confirms(unlisted.as_bytes(), ANA, ALONE));
        assert!(!confirms(
            listing.as_bytes(),
            "https://social.example/users/bob",
            ALONE
        ));
        assert!(!confirms(
            unowned.as_bytes(),
            "https://social.example/users/bob",
            ALONE
        ));
        assert!(!confirms(b"[]", ANA, ALONE));
    }

    #[test]
    fn a_document_without_the_key_or_its_pem_or_no_object_names_none() {
        let other = json!({ "id": ANA, "type": "Person", "publicKey": { "id": "https://social.example/users/ana#other", "publicKeyPem": PEM } });
        assert_eq!(read(&other, KEY), Err(ActorError::NoKey));
        let pemless =
            json!({ "id": ANA, "type": "Person", "publicKey": { "id": KEY, "owner": ANA } });
        assert_eq!(read(&pemless, KEY), Err(ActorError::NoKey));
        assert_eq!(actor_key(b"[]", KEY), Err(ActorError::Unreadable));
        assert_eq!(actor_key(b"not json", KEY), Err(ActorError::Unreadable));
        for (error, said) in [
            (
                ActorError::Unreadable,
                "the actor's document is not a JSON object",
            ),
            (
                ActorError::NoKey,
                "the actor's document holds no key under the signature's keyId",
            ),
            (
                ActorError::Misnamed,
                "the document is not the one it was read from, or names another actor as the key's owner",
            ),
            (
                ActorError::Unconfirmed,
                "the actor the key names as its owner does not list the key",
            ),
        ] {
            assert_eq!(error.to_string(), said);
        }
    }

    #[test]
    fn a_key_is_fetched_from_its_document_as_activity_json_on_https_alone() {
        assert_eq!(document(KEY), ANA);
        assert_eq!(document(ANA), ANA);
        for refused in [
            "https://social.example/users/%2e%2e/ana#main-key",
            "http://social.example/users/ana#main-key",
            "https://ana@social.example/users/ana#main-key",
            "https:///users/ana",
            "social.example/users/ana",
        ] {
            assert!(key_request(refused, 262_144).is_none(), "{refused}");
        }
        let request = key_request(KEY, 262_144).unwrap();
        assert_eq!(request.url.redacted(), ANA);
        assert_eq!(request.method, herald_ports::Method::Get);
        let accept = request
            .headers
            .iter()
            .find(|(name, _)| name == "Accept")
            .map(|(_, value)| value.redacted());
        assert_eq!(accept.as_deref(), Some("application/activity+json"));
        assert!(request.headers.iter().any(|(name, _)| name == "User-Agent"));
        assert_eq!(request.most, 262_144);
    }

    #[test]
    fn an_activity_names_its_actor_and_its_own_id_as_strings_alone() {
        assert_eq!(
            actor_of(
                json!({ "type": "Follow", "actor": ANA, "id": "https://social.example/f/1" })
                    .to_string()
                    .as_bytes()
            ),
            Some((
                ANA.to_owned(),
                Some("https://social.example/f/1".to_owned())
            ))
        );
        assert_eq!(
            actor_of(json!({ "actor": ANA }).to_string().as_bytes()),
            Some((ANA.to_owned(), None))
        );
        for named in [
            json!({ "actor": { "id": ANA } }),
            json!({ "actor": ANA, "id": { "id": "https://victim.example/n/1" } }),
            json!({ "actor": ANA, "id": ["https://victim.example/n/1"] }),
            json!({ "actor": ANA, "id": 1 }),
            json!({ "type": "Follow" }),
        ] {
            assert_eq!(actor_of(named.to_string().as_bytes()), None, "{named}");
        }
        assert_eq!(actor_of(b"not json"), None);
    }

    #[test]
    fn a_document_is_activitypub_json_by_its_content_type() {
        for served in [
            "application/activity+json",
            "application/activity+json; charset=utf-8",
            "Application/Activity+JSON",
            "application/ld+json; profile=\"https://www.w3.org/ns/activitystreams\"",
            "application/ld+json;profile=\"https://www.w3.org/ns/activitystreams https://w3id.org/security/v1\"",
        ] {
            assert!(activity_json(Some(served)), "{served}");
        }
        for served in [
            "application/json",
            "text/plain",
            "application/ld+json",
            "application/ld+json; profile=\"https://example.org/other\"",
            "text/html; profile=\"https://www.w3.org/ns/activitystreams\"",
        ] {
            assert!(!activity_json(Some(served)), "{served}");
        }
        assert!(!activity_json(None));
    }

    #[test]
    fn an_id_has_an_origin_only_where_it_is_the_url_the_parser_reads() {
        for (id, read) in [
            (KEY, "https://social.example"),
            (ANA, "https://social.example"),
            (
                "https://xn--bcher-kva.example/u?x=1#k",
                "https://xn--bcher-kva.example",
            ),
            ("https://a-b.example/users/%61na", "https://a-b.example"),
        ] {
            assert_eq!(origin(id).as_deref(), Some(read), "{id}");
        }
        for id in [
            "https://social.example",
            "http://social.example/users/ana",
            "https://Social.example/users/ana",
            "https://social.example:443/users/ana",
            "https://social.example:8443/users/ana",
            "https://ana@social.example/users/ana",
            "https://:pw@social.example/users/ana",
            "https://social.example\\evil/users/ana",
            "https://soc\tial.example/users/ana",
            "https://social.example/users/../admin",
            "https://social.example/./users/ana",
            "https://a.example/x/%2E%2E/y",
            "https://a.example/%2e%2e/b",
            "https://social.example/users/ana x",
            "https://bücher.example/",
            "https://1572395042/users/ana",
            "https://93.184.216.042/users/ana",
            "https://0x5db8d822/users/ana",
            "https://93.184.216.34/users/ana",
            "https://[2606:4700::1111]/users/ana",
            "https://social.example./users/ana",
            "https://a..b/users/ana",
            "https:///users/ana",
            "social.example/users/ana",
        ] {
            assert_eq!(origin(id), None, "{id}");
        }
        assert!(same_origin(KEY, ANA));
        assert!(!same_origin(KEY, "https://other.example/users/ana"));
        assert!(!same_origin(
            "https://Social.example/x",
            "https://Social.example/y"
        ));
    }

    #[test]
    fn a_key_or_owner_not_written_as_every_parser_reads_it_is_misnamed() {
        let dotted = "https://social.example/users/x/../ana#main-key";
        let actor = json!({ "id": "https://social.example/users/x/../ana", "type": "Person", "publicKey": { "id": dotted, "publicKeyPem": PEM } });
        assert_eq!(read(&actor, dotted), Err(ActorError::Misnamed));
        let claimed = json!({ "id": ALONE, "owner": "https://social.example/users/%2e%2e/ana", "publicKeyPem": PEM });
        assert_eq!(read(&claimed, ALONE), Err(ActorError::Misnamed));
    }

    fn delivered(actor: &serde_json::Value) -> Option<Inboxes> {
        inboxes(actor.to_string().as_bytes(), ANA)
    }

    #[test]
    fn a_follower_is_delivered_to_at_its_server_s_shared_inbox_or_else_its_own() {
        let shared = json!({
            "id": ANA,
            "type": "Person",
            "inbox": "https://social.example/users/ana/inbox",
            "endpoints": { "sharedInbox": "https://social.example/inbox" },
        });
        let found = delivered(&shared).unwrap();
        assert_eq!(found.personal, "https://social.example/users/ana/inbox");
        assert_eq!(found.delivered(), "https://social.example/inbox");
        let alone = json!({ "id": ANA, "type": "Service", "inbox": "https://social.example/users/ana/inbox" });
        let found = delivered(&alone).unwrap();
        assert_eq!(found.shared, None);
        assert_eq!(found.delivered(), "https://social.example/users/ana/inbox");
    }

    #[test]
    fn a_follower_s_inboxes_are_read_only_from_its_own_document_on_its_own_origin() {
        let foreign =
            json!({ "id": ANA, "type": "Person", "inbox": "https://elsewhere.example/inbox" });
        assert_eq!(delivered(&foreign), None);
        let foreign_shared = json!({
            "id": ANA,
            "type": "Person",
            "inbox": "https://social.example/users/ana/inbox",
            "endpoints": { "sharedInbox": "https://elsewhere.example/inbox" },
        });
        assert_eq!(delivered(&foreign_shared).unwrap().shared, None);
        let other = json!({ "id": "https://social.example/users/bo", "type": "Person", "inbox": "https://social.example/users/bo/inbox" });
        assert_eq!(delivered(&other), None);
        let note =
            json!({ "id": ANA, "type": "Note", "inbox": "https://social.example/users/ana/inbox" });
        assert_eq!(delivered(&note), None);
        let boxless = json!({ "id": ANA, "type": "Person" });
        assert_eq!(delivered(&boxless), None);
        assert_eq!(inboxes(b"not json", ANA), None);
    }
}
