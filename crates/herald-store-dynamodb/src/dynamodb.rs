use herald_ports::{
    Clock, Http, Method, Request, Response, Secret, Secrets, StoreError, Text, USER_AGENT,
};
use herald_records::{Blob, Kept, Records, Wrote};
use serde_json::{Value, json};

use crate::options::{REGION, StoreOptions};
use crate::sigv4::{Signing, signed};

/// The version of DynamoDB's API every target names.
const API: &str = "DynamoDB_20120810";
/// The item that counts the table's writes, by its directory and name.
const COUNT: (&str, &str) = ("~herald", "made");
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
    endpoint: String,
    host: String,
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
        let endpoint = endpoint.map_or_else(
            || format!("https://dynamodb.{region}.amazonaws.com"),
            |endpoint| endpoint.trim_end_matches('/').to_owned(),
        );
        let host = endpoint
            .split_once("://")
            .map_or(endpoint.as_str(), |(_, rest)| rest)
            .split('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        Self {
            endpoint,
            host,
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
        &self.endpoint
    }

    /// The store kept in the table, reached through `http` and signed at the
    /// time `clock` tells.
    #[must_use]
    pub fn store<H: Http, C: Clock>(self, http: H, clock: C) -> DynamodbStore<H, C> {
        Kept::new(DynamodbRecords {
            table: self,
            http,
            clock,
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
}

/// The store a [`Dynamodb`] table keeps.
pub type DynamodbStore<H, C> = Kept<DynamodbRecords<H, C>>;

fn unavailable(reason: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable {
        reason: reason.to_string(),
    }
}

fn corrupt(reason: &str) -> StoreError {
    StoreError::Corrupt {
        reason: reason.to_owned(),
    }
}

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
            host: &self.table.host,
        };
        let request = Request::new(
            Method::Post,
            Text::plain(format!("{}/", self.table.endpoint)),
        )
        .header("User-Agent", Text::plain(USER_AGENT))
        .body(body.to_string().into_bytes());
        let request = signed(
            request,
            &format!("{API}.{action}"),
            &signing,
            self.clock.now(),
        );
        let response = self.http.send(request).await.map_err(unavailable)?;
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
            return unavailable(format!(
                "the table {} does not exist in {}; make it with the string partition key directory and the string sort key name",
                self.table.table, self.table.region
            ));
        }
        unavailable(format!("DynamoDB refused it: {said}"))
    }

    async fn conditional(&self, action: &str, body: Value) -> Result<Wrote, StoreError> {
        match self.call(action, body).await? {
            Answer::Done(_) => Ok(Wrote::Written),
            Answer::Failed(kind, _) if kind == CONDITION_FAILED => Ok(Wrote::Conflict),
            Answer::Failed(kind, said) => Err(self.failed(&kind, &said)),
        }
    }

    /// The next number of the table's write count.
    async fn counted(&self) -> Result<i64, StoreError> {
        let (directory, name) = COUNT;
        let answered = self
            .done(
                "UpdateItem",
                json!({
                    "Key": { "directory": { "S": directory }, "name": { "S": name } },
                    "UpdateExpression": "ADD #m :one",
                    "ExpressionAttributeNames": { "#m": "made" },
                    "ExpressionAttributeValues": { ":one": { "N": "1" } },
                    "ReturnValues": "UPDATED_NEW",
                }),
            )
            .await?;
        answered
            .pointer("/Attributes/made/N")
            .and_then(Value::as_str)
            .and_then(|made| made.parse().ok())
            .ok_or_else(|| corrupt("the table's write count is not a number"))
    }
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
        match (text(item, "body", "S"), text(item, "version", "N")) {
            (Some(body), Some(version)) => Ok(Some(Blob {
                token: version.to_owned(),
                bytes: body.as_bytes().to_vec(),
            })),
            _ => Err(corrupt("a record's item is not a body and a version")),
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
                    .ok_or_else(|| corrupt("a record's name is not text"))?;
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
        let body = std::str::from_utf8(bytes).map_err(|_| corrupt("a record is not UTF-8 text"))?;
        let version = self.counted().await?;
        let (directory, name) = split(path);
        let mut item = json!({
            "Item": {
                "directory": { "S": directory },
                "name": { "S": name },
                "body": { "S": body },
                "version": { "N": version.to_string() },
            },
            "ExpressionAttributeNames": { "#n": "name" },
        });
        let condition = match replacing {
            None => json!({ "ConditionExpression": "attribute_not_exists(#n)" }),
            Some(token) => json!({
                "ConditionExpression": "#v = :v",
                "ExpressionAttributeNames": { "#n": "name", "#v": "version" },
                "ExpressionAttributeValues": { ":v": { "N": token } },
            }),
        };
        if let (Some(item), Value::Object(condition)) = (item.as_object_mut(), condition) {
            item.extend(condition);
        }
        self.conditional("PutItem", item).await
    }

    async fn remove(&self, path: &str, replacing: &str) -> Result<Wrote, StoreError> {
        let removed = self
            .conditional(
                "DeleteItem",
                json!({
                    "Key": keyed(path),
                    "ConditionExpression": "#v = :v",
                    "ExpressionAttributeNames": { "#v": "version" },
                    "ExpressionAttributeValues": { ":v": { "N": replacing } },
                }),
            )
            .await?;
        if removed == Wrote::Written || self.read(path).await?.is_none() {
            return Ok(Wrote::Written);
        }
        Ok(Wrote::Conflict)
    }
}
