use herald_ports::{Http, Method, Request, Response, Secret, StoreError, Text, USER_AGENT};
use herald_records::Wrote;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::cache::{CacheRecords, Generation};

/// The cache service's Twirp methods' common path.
const SERVICE: &str = "/twirp/github.actions.results.api.v1.CacheService/";
/// The version every generation is saved under: the SHA-256 of
/// `herald/actions-cache/v1`.
const VERSION: &str = "514567f3aac3eee4eded2f7416de16186bc488fbfc99341324de7a200c712123";
/// What the service answers to an entry that exists.
const EXISTS: u16 = 409;
/// The name a signed URL is redacted under.
const SIGNED: &str = "the Actions cache's signed URL";

#[derive(Deserialize)]
struct Found {
    ok: bool,
    #[serde(default, alias = "signedDownloadUrl")]
    signed_download_url: String,
    #[serde(default, alias = "matchedKey")]
    matched_key: String,
}

#[derive(Deserialize)]
struct Reserved {
    ok: bool,
    #[serde(default, alias = "signedUploadUrl")]
    signed_upload_url: String,
}

#[derive(Deserialize)]
struct Finalized {
    ok: bool,
}

fn unavailable(reason: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable {
        reason: reason.to_string(),
    }
}

fn answered(response: &Response) -> StoreError {
    unavailable(format!("the Actions cache answered {}", response.status))
}

fn corrupt(what: &str, error: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        reason: format!("{what}: {error}"),
    }
}

fn read<T: DeserializeOwned>(what: &str, response: &Response) -> Result<T, StoreError> {
    if !response.succeeded() {
        return Err(answered(response));
    }
    serde_json::from_slice(&response.body).map_err(|error| corrupt(what, error))
}

/// The origin of a signed blob URL, which must be HTTPS.
fn origin(url: &str) -> Result<&str, StoreError> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| unavailable("the Actions cache named a blob URL that is not HTTPS"))?;
    let host = rest
        .find('/')
        .map_or(url.len(), |end| end + "https://".len());
    Ok(&url[..host])
}

impl<H: Http + Send> CacheRecords<'_, H> {
    fn key(&self, number: u64) -> String {
        format!("{}-{number:020}", self.cache.prefix)
    }

    async fn call(&self, method: &str, body: &serde_json::Value) -> Result<Response, StoreError> {
        let request = Request::new(
            Method::Post,
            Text::plain(format!("{}{SERVICE}{method}", self.cache.results)),
        )
        .header("Content-Type", Text::plain("application/json"))
        .header("Accept", Text::plain("application/json"))
        .header("User-Agent", Text::plain(USER_AGENT))
        .header(
            "Authorization",
            Text::plain("Bearer ").then_secret(self.cache.token.clone()),
        )
        .body(body.to_string());
        self.service.send(request).await.map_err(unavailable)
    }

    async fn blob(&self, request: Request, url: &str) -> Result<Response, StoreError> {
        let http = (self.blobs)(origin(url)?).map_err(unavailable)?;
        http.send(request).await.map_err(unavailable)
    }

    /// The newest generation, if the cache holds one.
    pub(crate) async fn latest(&self) -> Result<Option<Generation>, StoreError> {
        let prefix = format!("{}-", self.cache.prefix);
        let body = json!({ "key": prefix, "restore_keys": [prefix], "version": VERSION });
        let response = self.call("GetCacheEntryDownloadURL", &body).await?;
        let found: Found = read("the Actions cache's answer", &response)?;
        if !found.ok {
            return Ok(None);
        }
        let number: u64 = found
            .matched_key
            .strip_prefix(&prefix)
            .and_then(|number| number.parse().ok())
            .ok_or_else(|| corrupt(&found.matched_key, "not a generation of herald's records"))?;
        let url = Secret::new(SIGNED, found.signed_download_url);
        let request = Request::new(Method::Get, Text::secret(url.clone()));
        let downloaded = self.blob(request, url.expose()).await?;
        let mut generation: Generation = read(&found.matched_key, &downloaded)?;
        generation.number = number;
        Ok(Some(generation))
    }

    /// Saves `generation` as the cache's next entry, unless another run saved
    /// that generation first.
    pub(crate) async fn publish(&self, generation: &Generation) -> Result<Wrote, StoreError> {
        let key = self.key(generation.number);
        let body = json!({ "key": key, "version": VERSION });
        let response = self.call("CreateCacheEntry", &body).await?;
        if response.status == EXISTS {
            return Ok(Wrote::Conflict);
        }
        let reserved: Reserved = read("the Actions cache's answer", &response)?;
        if !reserved.ok {
            return Ok(Wrote::Conflict);
        }
        let bytes = serde_json::to_vec(generation).unwrap_or_default();
        let size = bytes.len();
        let url = Secret::new(SIGNED, reserved.signed_upload_url);
        let request = Request::new(Method::Put, Text::secret(url.clone()))
            .header("x-ms-blob-type", Text::plain("BlockBlob"))
            .header("Content-Type", Text::plain("application/octet-stream"))
            .body(bytes);
        let uploaded = self.blob(request, url.expose()).await?;
        if !uploaded.succeeded() {
            return Err(answered(&uploaded));
        }
        let body = json!({ "key": key, "version": VERSION, "size_bytes": size.to_string() });
        let response = self.call("FinalizeCacheEntryUpload", &body).await?;
        let finalized: Finalized = read("the Actions cache's answer", &response)?;
        if finalized.ok {
            Ok(Wrote::Written)
        } else {
            Err(unavailable(format!("the Actions cache did not keep {key}")))
        }
    }
}
