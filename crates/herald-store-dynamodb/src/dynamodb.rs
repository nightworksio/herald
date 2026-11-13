use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use herald_ports::{Clock, Http, Method, Request, Response, Secret, Secrets, StoreError, Text};
use herald_records::{Blob, Kept, Records, Wrote};
use serde_json::{Value, json};

use crate::options::StoreOptions;
use herald_aws::{Endpoint, REGION, Signing, signed};

/// The service DynamoDB's requests are signed for.
const SERVICE: &str = "dynamodb";
/// The path every request to DynamoDB's JSON API is sent to.
const ROOT: &str = "/";
/// What DynamoDB's requests and answers are.
const JSON: &str = "application/x-amz-json-1.0";
/// The version of DynamoDB's API every target names.
const API: &str = "DynamoDB_20120810";
/// What DynamoDB names a condition that did not hold.
const CONDITION_FAILED: &str = "ConditionalCheckFailedException";
/// What DynamoDB names a table that does not exist.
const NO_TABLE: &str = "ResourceNotFoundException";

/// What a DynamoDB store's requests are signed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    /// The access key's id.
    pub access_key_id: Secret,
    /// The secret access key.
    pub secret_access_key: Secret,
    /// A session token, where the credentials are temporary.
    pub session_token: Option<Secret>,
}

/// Where the records are kept: a DynamoDB table in a region, reached with
/// credentials that may read and write its items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dynamodb {
    endpoint: Endpoint,
    region: String,
    table: String,
    credentials: Credentials,
}

impl Dynamodb {
    /// The table `table` in `region`, reached at `endpoint` or the region's
    /// own, with `credentials`.
    #[must_use]
    pub fn new(
        table: &str,
        region: &str,
        endpoint: Option<&str>,
        credentials: Credentials,
    ) -> Self {
        Self {
            endpoint: Endpoint::new(SERVICE, region, endpoint),
            region: region.to_owned(),
            table: table.to_owned(),
            credentials,
        }
    }

    /// The table `options` name, in their region or the platform's, with
    /// the credentials they name read through `secrets`.
    ///
    /// # Errors
    ///
    /// When no region is given or set, or a credential is not set.
    pub async fn reached<S: Secrets>(options: &StoreOptions, secrets: &S) -> Result<Self, String> {
        let region = match &options.region {
            Some(region) => region.clone(),
            None => secrets
                .secret(REGION)
                .await
                .map(|region| region.expose().to_owned())
                .map_err(|_| format!("{REGION} is not set; give the dynamodb store a region"))?,
        };
        let required = |name: &str| {
            let name = name.to_owned();
            async move {
                secrets.secret(&name).await.map_err(|_| {
                    format!("{name} is not set; the dynamodb store signs its requests with it")
                })
            }
        };
        let credentials = Credentials {
            access_key_id: required(options.access_key_id()).await?,
            secret_access_key: required(options.secret_access_key()).await?,
            session_token: secrets.secret(options.session_token()).await.ok(),
        };
        Ok(Self::new(
            options.table(),
            &region,
            options.endpoint.as_deref(),
            credentials,
        ))
    }

    /// The base URL it reaches.
    #[must_use]
    pub fn api(&self) -> &str {
        self.endpoint.url()
    }

    /// The host its requests are signed for: the endpoint's, with its port
    /// where that is not its scheme's.
    #[must_use]
    pub fn host(&self) -> &str {
        self.endpoint.host()
    }

    /// The store kept in the table, reached through `http` and signed at the
    /// time `clock` tells.
    ///
    /// Each record it makes draws its epoch from `seed`, a nonce no other
    /// run takes, and a count of the records this store has made.
    #[must_use]
    pub fn store<H: Http, C: Clock>(self, http: H, clock: C, seed: String) -> DynamodbStore<H, C> {
        Kept::new(DynamodbRecords {
            table: self,
            http,
            clock,
            seed,
            made: Arc::default(),
        })
    }
}

/// [`Dynamodb`] reached through `Http`: where a [`DynamodbStore`] keeps its
/// records.
#[derive(Debug, Clone)]
pub struct DynamodbRecords<H, C> {
    table: Dynamodb,
    http: H,
    clock: C,
    seed: String,
    made: Arc<AtomicU64>,
}

/// The store a [`Dynamodb`] table keeps.
pub type DynamodbStore<H, C> = Kept<DynamodbRecords<H, C>>;

/// A record's path as its directory and its name.
fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("/", path))
}

/// A record's path as an item's key.
fn keyed(path: &str) -> Value {
    let (directory, name) = split(path);
    json!({ "directory": { "S": directory }, "name": { "S": name } })
}

fn text<'a>(item: &'a Value, attribute: &str, kind: &str) -> Option<&'a str> {
    item.get(attribute)?.get(kind)?.as_str()
}

/// What DynamoDB answered: the body where it succeeded, or the kind of
/// error it named and what it said.
enum Answer {
    Done(Value),
    Failed(String, String),
}

fn answer(response: &Response) -> Answer {
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    if response.succeeded() {
        return Answer::Done(body);
    }
    let kind = body
        .get("__type")
        .and_then(Value::as_str)
        .and_then(|kind| kind.rsplit('#').next())
        .unwrap_or_default()
        .to_owned();
    let said = body
        .get("message")
        .or_else(|| body.get("Message"))
        .and_then(Value::as_str)
        .map_or_else(
            || format!("DynamoDB answered {}", response.status),
            ToOwned::to_owned,
        );
    Answer::Failed(kind, said)
}

impl<H: Http, C: Clock> DynamodbRecords<H, C> {
    async fn call(&self, action: &str, mut body: Value) -> Result<Answer, StoreError> {
        if let Some(object) = body.as_object_mut() {
            object.insert("TableName".to_owned(), json!(self.table.table));
        }
        let credentials = &self.table.credentials;
        let signing = Signing {
            access_key_id: &credentials.access_key_id,
            secret_access_key: &credentials.secret_access_key,
            session_token: credentials.session_token.as_ref(),
            region: &self.table.region,
            service: SERVICE,
            host: self.table.endpoint.host(),
        };
        let request = Request::new(Method::Post, Text::plain(self.table.endpoint.at(ROOT)))
            .identified()
            .body(body.to_string().into_bytes());
        let target = format!("{API}.{action}");
        let request = signed(
            request,
            &self.table.endpoint.canonical(ROOT),
            JSON,
            &[("Target", &target)],
            &signing,
            self.clock.now(),
        );
        let response = self
            .http
            .send(request)
            .await
            .map_err(StoreError::unavailable)?;
        Ok(answer(&response))
    }

    async fn done(&self, action: &str, body: Value) -> Result<Value, StoreError> {
        match self.call(action, body).await? {
            Answer::Done(body) => Ok(body),
            Answer::Failed(kind, said) => Err(self.failed(&kind, &said)),
        }
    }

    fn failed(&self, kind: &str, said: &str) -> StoreError {
        if kind == NO_TABLE {
            return StoreError::unavailable(format_args!(
                "the table {} does not exist in {}; make it with the string partition key directory and the string sort key name",
                self.table.table, self.table.region
            ));
        }
        StoreError::unavailable(format_args!("DynamoDB refused it: {said}"))
    }

    async fn conditional(&self, action: &str, body: Value) -> Result<Wrote, StoreError> {
        match self.call(action, body).await? {
            Answer::Done(_) => Ok(Wrote::Written),
            Answer::Failed(kind, _) if kind == CONDITION_FAILED => Ok(Wrote::Conflict),
            Answer::Failed(kind, said) => Err(self.failed(&kind, &said)),
        }
    }

    /// A new epoch for a record this store makes.
    fn epoch(&self) -> String {
        format!(
            "{}-{}",
            self.seed,
            self.made.fetch_add(1, Ordering::Relaxed)
        )
    }
}

/// A record's token: the epoch it drew when it was made, and how many
/// times it has been written since.
fn token(epoch: &str, version: &str) -> String {
    format!("{epoch}.{version}")
}

/// The epoch and the version a token names, where it names them.
fn parsed(token: &str) -> Option<(&str, u64)> {
    let (epoch, version) = token.split_once('.')?;
    Some((epoch, version.parse().ok()?))
}

/// The condition that the item at a key still holds `epoch` and `version`.
fn holding(epoch: &str, version: u64) -> Value {
    json!({
        "ConditionExpression": "#e = :e AND #v = :v",
        "ExpressionAttributeNames": { "#e": "epoch", "#v": "version" },
        "ExpressionAttributeValues": {
            ":e": { "S": epoch },
            ":v": { "N": version.to_string() },
        },
    })
}

impl<H: Http, C: Clock> Records for DynamodbRecords<H, C> {
    async fn read(&self, path: &str) -> Result<Option<Blob>, StoreError> {
        let answered = self
            .done(
                "GetItem",
                json!({ "Key": keyed(path), "ConsistentRead": true }),
            )
            .await?;
        let Some(item) = answered.get("Item") else {
            return Ok(None);
        };
        match (
            text(item, "body", "S"),
            text(item, "epoch", "S"),
            text(item, "version", "N"),
        ) {
            (Some(body), Some(epoch), Some(version)) => Ok(Some(Blob {
                token: token(epoch, version),
                bytes: body.as_bytes().to_vec(),
            })),
            _ => Err(StoreError::corrupt(
                "a record's item is not a body, an epoch and a version",
            )),
        }
    }

    async fn list(&self, directory: &str) -> Result<Vec<String>, StoreError> {
        let mut paths = Vec::new();
        let mut start: Option<Value> = None;
        loop {
            let mut query = json!({
                "KeyConditionExpression": "#d = :d",
                "ProjectionExpression": "#n",
                "ExpressionAttributeNames": { "#d": "directory", "#n": "name" },
                "ExpressionAttributeValues": { ":d": { "S": directory } },
                "ConsistentRead": true,
            });
            if let (Some(start), Some(object)) = (start.take(), query.as_object_mut()) {
                object.insert("ExclusiveStartKey".to_owned(), start);
            }
            let answered = self.done("Query", query).await?;
            for item in answered
                .get("Items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = text(item, "name", "S")
                    .ok_or_else(|| StoreError::corrupt("a record's name is not text"))?;
                paths.push(format!("{directory}/{name}"));
            }
            match answered.get("LastEvaluatedKey") {
                Some(last) => start = Some(last.clone()),
                None => break,
            }
        }
        paths.sort();
        Ok(paths)
    }

    async fn write(
        &self,
        path: &str,
        bytes: &[u8],
        replacing: Option<&str>,
    ) -> Result<Wrote, StoreError> {
        let body = std::str::from_utf8(bytes)
            .map_err(|_| StoreError::corrupt("a record is not UTF-8 text"))?;
        let (epoch, version, condition) = match replacing {
            None => (
                self.epoch(),
                1,
                json!({
                    "ConditionExpression": "attribute_not_exists(#n)",
                    "ExpressionAttributeNames": { "#n": "name" },
                }),
            ),
            Some(replaced) => {
                let Some((epoch, version)) = parsed(replaced) else {
                    return Ok(Wrote::Conflict);
                };
                (epoch.to_owned(), version + 1, holding(epoch, version))
            }
        };
        let (directory, name) = split(path);
        let mut item = json!({
            "Item": {
                "directory": { "S": directory },
                "name": { "S": name },
                "body": { "S": body },
                "epoch": { "S": epoch },
                "version": { "N": version.to_string() },
            },
        });
        if let (Some(item), Value::Object(condition)) = (item.as_object_mut(), condition) {
            item.extend(condition);
        }
        self.conditional("PutItem", item).await
    }

    async fn remove(&self, path: &str, replacing: &str) -> Result<Wrote, StoreError> {
        let Some((epoch, version)) = parsed(replacing) else {
            return Ok(Wrote::Conflict);
        };
        let mut asked = json!({ "Key": keyed(path) });
        if let (Some(asked), Value::Object(condition)) =
            (asked.as_object_mut(), holding(epoch, version))
        {
            asked.extend(condition);
        }
        let removed = self.conditional("DeleteItem", asked).await?;
        if removed == Wrote::Written || self.read(path).await?.is_none() {
            return Ok(Wrote::Written);
        }
        Ok(Wrote::Conflict)
    }
}
