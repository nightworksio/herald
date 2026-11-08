//! The DynamoDB store against a model of DynamoDB's API, through the
//! conformance suite every store passes, and through the answers a table
//! that is missing, refusing or changed underneath gives.

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the model answers at once, and is written as the async trait it fakes"
)]
mod conformance {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use herald_ports::{Http, HttpError, Request, Response, Secret, StoreError};
    use herald_records::{Records, Wrote};
    use herald_store_dynamodb::StoreOptions;
    use herald_store_dynamodb::{Credentials, Dynamodb, DynamodbRecords, DynamodbStore};
    use herald_testing::{FakeClock, FakeSecrets, store_conformance};
    use serde_json::{Value, json};

    /// The most items the model answers a query with in one page.
    const PAGE: usize = 2;

    #[derive(Debug, Default)]
    struct Model {
        items: BTreeMap<(String, String), Value>,
        fail: Option<(String, u16, String)>,
        unsigned: u32,
    }

    #[derive(Debug, Default)]
    struct Table {
        model: Mutex<Model>,
    }

    fn reply(status: u16, body: &Value) -> Response {
        Response {
            status,
            headers: Vec::new(),
            body: body.to_string().into_bytes(),
        }
    }

    fn failed(kind: &str) -> Response {
        reply(
            400,
            &json!({ "__type": format!("com.amazonaws.dynamodb.v20120810#{kind}"), "message": "no" }),
        )
    }

    fn key(body: &Value, at: &str) -> (String, String) {
        let s = |attribute: &str| {
            body.pointer(&format!("/{at}/{attribute}/S"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        (s("directory"), s("name"))
    }

    fn holds(condition: &str, body: &Value, current: Option<&Value>) -> bool {
        let held = |attribute: &str, kind: &str| {
            current.and_then(|item| item.pointer(&format!("/{attribute}/{kind}")))
        };
        let asked = |value: &str, kind: &str| {
            body.pointer(&format!("/ExpressionAttributeValues/{value}/{kind}"))
        };
        match condition {
            "attribute_not_exists(#n)" => current.is_none(),
            "#e = :e AND #v = :v" => {
                current.is_some()
                    && held("epoch", "S") == asked(":e", "S")
                    && held("version", "N") == asked(":v", "N")
            }
            _ => false,
        }
    }

    impl Model {
        fn answer(&mut self, action: &str, body: &Value) -> Response {
            if let Some((failing, status, kind)) = &self.fail
                && failing == action
            {
                let mut answer = failed(kind);
                answer.status = *status;
                return answer;
            }
            match action {
                "GetItem" => match self.items.get(&key(body, "Key")) {
                    Some(item) => reply(200, &json!({ "Item": item })),
                    None => reply(200, &json!({})),
                },
                "Query" => {
                    let directory = body
                        .pointer("/ExpressionAttributeValues/:d/S")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let after = body
                        .pointer("/ExclusiveStartKey/name/S")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let names: Vec<&String> = self
                        .items
                        .keys()
                        .filter(|(d, name)| d == directory && name.as_str() > after)
                        .map(|(_, name)| name)
                        .collect();
                    let page: Vec<Value> = names
                        .iter()
                        .take(PAGE)
                        .map(|name| {
                            let item = self.items.get(&(directory.to_owned(), (*name).clone()));
                            json!({ "name": item.and_then(|item| item.get("name")) })
                        })
                        .collect();
                    let mut answer = json!({ "Items": page });
                    if names.len() > PAGE
                        && let (Some(last), Some(object)) =
                            (names.get(PAGE - 1), answer.as_object_mut())
                    {
                        object.insert(
                            "LastEvaluatedKey".to_owned(),
                            json!({ "directory": { "S": directory }, "name": { "S": last } }),
                        );
                    }
                    reply(200, &answer)
                }
                "PutItem" => {
                    let at = key(body, "Item");
                    let condition = body
                        .get("ConditionExpression")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !holds(condition, body, self.items.get(&at)) {
                        return failed("ConditionalCheckFailedException");
                    }
                    self.items
                        .insert(at, body.get("Item").cloned().unwrap_or_default());
                    reply(200, &json!({}))
                }
                "DeleteItem" => {
                    let at = key(body, "Key");
                    let condition = body
                        .get("ConditionExpression")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !holds(condition, body, self.items.get(&at)) {
                        return failed("ConditionalCheckFailedException");
                    }
                    self.items.remove(&at);
                    reply(200, &json!({}))
                }
                _ => failed("UnknownOperationException"),
            }
        }
    }

    impl Http for &Table {
        async fn send(&self, request: Request) -> Result<Response, HttpError> {
            let header = |name: &str| {
                request
                    .headers
                    .iter()
                    .find(|(header, _)| header == name)
                    .map(|(_, value)| value.expose())
                    .unwrap_or_default()
            };
            let mut model = self.model.lock().unwrap();
            if !header("Authorization").starts_with("AWS4-HMAC-SHA256 Credential=AKID/") {
                model.unsigned += 1;
            }
            assert_eq!(request.url.expose(), "http://localhost:8000/");
            let action = header("X-Amz-Target")
                .strip_prefix("DynamoDB_20120810.")
                .unwrap_or_default()
                .to_owned();
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body.get("TableName"), Some(&json!("herald")));
            Ok(model.answer(&action, &body))
        }
    }

    fn table() -> Dynamodb {
        Dynamodb::new(
            "herald",
            "eu-west-1",
            Some("http://localhost:8000/"),
            Credentials {
                access_key_id: Secret::new("AWS_ACCESS_KEY_ID", "AKID"),
                secret_access_key: Secret::new("AWS_SECRET_ACCESS_KEY", "secret"),
                session_token: None,
            },
        )
    }

    fn clock() -> FakeClock {
        FakeClock::at("2026-11-05T23:00:00Z".parse().unwrap())
    }

    fn store(model: &Table) -> DynamodbStore<&Table, FakeClock> {
        table().store(model, clock(), "seed".to_owned())
    }

    #[test]
    fn a_table_passes_the_conformance_suite_with_every_request_signed() {
        let model = Table::default();
        assert_eq!(
            pollster::block_on(store_conformance(&store(&model))),
            Ok(())
        );
        assert_eq!(model.model.lock().unwrap().unsigned, 0);
    }

    #[test]
    fn the_endpoint_is_the_region_s_own_or_the_one_named() {
        let own = Dynamodb::new(
            "herald",
            "eu-west-1",
            None,
            Credentials {
                access_key_id: Secret::new("A", "a"),
                secret_access_key: Secret::new("B", "b"),
                session_token: Some(Secret::new("C", "c")),
            },
        );
        assert_eq!(own.api(), "https://dynamodb.eu-west-1.amazonaws.com");
        assert_eq!(table().api(), "http://localhost:8000");
    }

    #[test]
    fn a_directory_is_listed_page_by_page_and_removing_what_is_gone_counts() {
        let model = Table::default();
        let kept = store(&model);
        let records: &DynamodbRecords<&Table, FakeClock> = kept.records();
        for name in ["a", "b", "c", "d", "e"] {
            assert_eq!(
                pollster::block_on(records.write(&format!("ledger/{name}.json"), b"{}", None)),
                Ok(Wrote::Written)
            );
        }
        assert_eq!(pollster::block_on(records.list("ledger")).unwrap().len(), 5);
        let read = pollster::block_on(records.read("ledger/a.json"))
            .unwrap()
            .unwrap();
        assert_eq!(
            pollster::block_on(records.write("ledger/a.json", b"[]", Some("0"))),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.remove("ledger/a.json", "0")),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.remove("ledger/a.json", &read.token)),
            Ok(Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(records.remove("ledger/a.json", &read.token)),
            Ok(Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(records.write("ledger/a.json", b"{}", None)),
            Ok(Wrote::Written)
        );
        let remade = pollster::block_on(records.read("ledger/a.json"))
            .unwrap()
            .unwrap();
        assert_ne!(remade.token, read.token);
        assert_eq!(
            pollster::block_on(records.remove("ledger/a.json", &read.token)),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.write("ledger/a.json", b"[1]", Some(&remade.token))),
            Ok(Wrote::Written)
        );
        let updated = pollster::block_on(records.read("ledger/a.json"))
            .unwrap()
            .unwrap();
        let epoch = remade.token.strip_suffix(".1").unwrap();
        assert_eq!(updated.token, format!("{epoch}.2"));
        assert_eq!(
            pollster::block_on(records.write("ledger/a.json", b"[2]", Some(&remade.token))),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.write("ledger/a.json", b"[]", Some(&read.token))),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.write("top", b"{}", None)),
            Ok(Wrote::Written)
        );
        assert!(pollster::block_on(records.read("top")).unwrap().is_some());
        assert_eq!(
            pollster::block_on(records.write("ledger/b.json", &[0xff], None)),
            Err(StoreError::Corrupt {
                reason: "a record is not UTF-8 text".to_owned()
            })
        );
    }

    #[test]
    fn a_missing_table_a_refusal_and_a_corrupt_item_are_each_said() {
        let model = Table::default();
        let kept = store(&model);
        let records = kept.records();
        model.model.lock().unwrap().fail = Some((
            "GetItem".to_owned(),
            400,
            "ResourceNotFoundException".to_owned(),
        ));
        assert_eq!(
            pollster::block_on(records.read("ledger/a.json")),
            Err(StoreError::Unavailable {
                reason: "the table herald does not exist in eu-west-1; make it with the string partition key directory and the string sort key name".to_owned()
            })
        );
        model.model.lock().unwrap().fail =
            Some(("PutItem".to_owned(), 500, "InternalServerError".to_owned()));
        assert_eq!(
            pollster::block_on(records.write("ledger/a.json", b"{}", None)),
            Err(StoreError::Unavailable {
                reason: "DynamoDB refused it: no".to_owned()
            })
        );
        model.model.lock().unwrap().fail = Some((
            "DeleteItem".to_owned(),
            400,
            "AccessDeniedException".to_owned(),
        ));
        assert!(pollster::block_on(records.remove("ledger/a.json", "e.1")).is_err());
        model.model.lock().unwrap().fail = None;
        model.model.lock().unwrap().items.insert(
            ("ledger".to_owned(), "x.json".to_owned()),
            json!({ "name": { "S": "x.json" } }),
        );
        assert_eq!(
            pollster::block_on(records.read("ledger/x.json")),
            Err(StoreError::Corrupt {
                reason: "a record's item is not a body, an epoch and a version".to_owned()
            })
        );
        model.model.lock().unwrap().items.insert(
            ("ledger".to_owned(), "y.json".to_owned()),
            json!({ "name": { "N": "1" } }),
        );
        assert_eq!(
            pollster::block_on(records.list("ledger")),
            Err(StoreError::Corrupt {
                reason: "a record's name is not text".to_owned()
            })
        );
        for unparsed in ["1", "e.x"] {
            assert_eq!(
                pollster::block_on(records.write("ledger/z.json", b"{}", Some(unparsed))),
                Ok(Wrote::Conflict)
            );
            assert_eq!(
                pollster::block_on(records.remove("ledger/z.json", unparsed)),
                Ok(Wrote::Conflict)
            );
        }
    }

    #[derive(Debug)]
    struct Unreachable;

    impl Http for Unreachable {
        async fn send(&self, _: Request) -> Result<Response, HttpError> {
            Err(HttpError::TimedOut)
        }
    }

    #[derive(Debug)]
    struct Blank;

    impl Http for Blank {
        async fn send(&self, _: Request) -> Result<Response, HttpError> {
            Ok(Response {
                status: 503,
                headers: Vec::new(),
                body: Vec::new(),
            })
        }
    }

    #[test]
    fn a_table_that_cannot_be_reached_or_answers_nothing_is_unavailable() {
        let unreachable = table().store(Unreachable, clock(), "seed".to_owned());
        assert!(matches!(
            pollster::block_on(unreachable.records().read("ledger/a.json")),
            Err(StoreError::Unavailable { .. })
        ));
        let blank = table().store(Blank, clock(), "seed".to_owned());
        assert_eq!(
            pollster::block_on(blank.records().list("ledger")),
            Err(StoreError::Unavailable {
                reason: "DynamoDB refused it: DynamoDB answered 503".to_owned()
            })
        );
    }

    #[test]
    fn a_table_is_reached_in_the_region_given_or_set_with_the_credentials_named() {
        let credentials = || {
            FakeSecrets::default()
                .with("AWS_ACCESS_KEY_ID", "AKID")
                .with("AWS_SECRET_ACCESS_KEY", "secret")
        };
        let secrets = credentials();
        let given: StoreOptions = serde_json::from_str(r#"{"region": "eu-west-1"}"#).unwrap();
        let reached = pollster::block_on(Dynamodb::reached(&given, &secrets)).unwrap();
        assert_eq!(reached.api(), "https://dynamodb.eu-west-1.amazonaws.com");
        let set = credentials()
            .with("AWS_REGION", "us-east-2")
            .with("AWS_SESSION_TOKEN", "t");
        let reached =
            pollster::block_on(Dynamodb::reached(&StoreOptions::default(), &set)).unwrap();
        assert_eq!(reached.api(), "https://dynamodb.us-east-2.amazonaws.com");
        assert_eq!(
            pollster::block_on(Dynamodb::reached(&StoreOptions::default(), &secrets)).err(),
            Some("AWS_REGION is not set; give the dynamodb store a region".to_owned())
        );
        assert_eq!(
            pollster::block_on(Dynamodb::reached(&given, &FakeSecrets::default())).err(),
            Some(
                "AWS_ACCESS_KEY_ID is not set; the dynamodb store signs its requests with it"
                    .to_owned()
            )
        );
    }
}
