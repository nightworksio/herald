use herald_core::Address;
use herald_core::status;
use herald_ports::{Http, Method, Request, Response, Secret, SecretText, StoreError};
use herald_records::Wrote;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::cache::{CacheRecords, Generation};

/// The cache service's Twirp methods' common path.
const SERVICE: &str = "/twirp/github.actions.results.api.v1.CacheService/";
/// The version every generation is saved under: the SHA-256 of
/// `herald/actions-cache/v1`.
const VERSION: &str = "514567f3aac3eee4eded2f7416de16186bc488fbfc99341324de7a200c712123";
/// The service, as its errors name it.
const CACHE: &str = "the Actions cache";
/// What a Twirp method answers, as its errors name it.
const ANSWER: &str = "the Actions cache's answer";
/// The name a signed URL is redacted under.
const SIGNED: &str = "the Actions cache's signed URL";

/// What the Actions cache answers a lookup of a key.
#[derive(Deserialize)]
struct CacheLookup {
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

fn encoded(what: &str, value: &impl Serialize) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec(value).map_err(|error| StoreError::unreadable(what, error))
}

fn read<T: DeserializeOwned>(what: &str, response: &Response) -> Result<T, StoreError> {
    if !response.succeeded() {
        return Err(StoreError::answered(CACHE, response.status));
    }
    serde_json::from_slice(&response.body).map_err(|error| StoreError::unreadable(what, error))
}

/// The origin of a signed blob URL, which must be HTTPS.
fn origin(url: &str) -> Result<String, StoreError> {
    Address::parse(url)
        .filter(|address| address.scheme() == "https")
        .map(|address| address.origin())
        .ok_or_else(|| {
            StoreError::unavailable("the Actions cache named a blob URL that is not HTTPS")
        })
}

impl<H: Http + Send> CacheRecords<'_, H> {
    fn key(&self, number: u64) -> String {
        format!("{}-{number:020}", self.cache.prefix)
    }

    async fn call(&self, method: &str, body: &serde_json::Value) -> Result<Response, StoreError> {
        let request = Request::new(
            Method::Post,
            SecretText::plain(format!("{}{SERVICE}{method}", self.cache.results)),
        )
        .header("Content-Type", SecretText::plain("application/json"))
        .header("Accept", SecretText::plain("application/json"))
        .identified()
        .bearer(self.cache.token.clone())
        .body(body.to_string());
        self.service
            .send(request)
            .await
            .map_err(StoreError::unavailable)
    }

    async fn blob(&self, request: Request, url: &str) -> Result<Response, StoreError> {
        let http = (self.blobs)(&origin(url)?).map_err(StoreError::unavailable)?;
        http.send(request).await.map_err(StoreError::unavailable)
    }

    /// The newest generation, if the cache holds one.
    pub(crate) async fn latest(&self) -> Result<Option<Generation>, StoreError> {
        let prefix = format!("{}-", self.cache.prefix);
        let body = json!({ "key": prefix, "restore_keys": [prefix], "version": VERSION });
        let response = self.call("GetCacheEntryDownloadURL", &body).await?;
        let found: CacheLookup = read(ANSWER, &response)?;
        if !found.ok {
            return Ok(None);
        }
        let number: u64 = found
            .matched_key
            .strip_prefix(&prefix)
            .and_then(|number| number.parse().ok())
            .ok_or_else(|| {
                StoreError::unreadable(&found.matched_key, "not a generation of herald's records")
            })?;
        let url = Secret::new(SIGNED, found.signed_download_url);
        let request = Request::new(Method::Get, SecretText::secret(url.clone()));
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
        if response.status == status::CONFLICT {
            return Ok(Wrote::Conflict);
        }
        let reserved: Reserved = read(ANSWER, &response)?;
        if !reserved.ok {
            return Ok(Wrote::Conflict);
        }
        let bytes = encoded("the cache's generation", generation)?;
        let size = bytes.len();
        let url = Secret::new(SIGNED, reserved.signed_upload_url);
        let request = Request::new(Method::Put, SecretText::secret(url.clone()))
            .header("x-ms-blob-type", SecretText::plain("BlockBlob"))
            .header(
                "Content-Type",
                SecretText::plain("application/octet-stream"),
            )
            .body(bytes);
        let uploaded = self.blob(request, url.expose()).await?;
        if !uploaded.succeeded() {
            return Err(StoreError::answered(CACHE, uploaded.status));
        }
        let body = json!({ "key": key, "version": VERSION, "size_bytes": size.to_string() });
        let response = self.call("FinalizeCacheEntryUpload", &body).await?;
        let finalized: Finalized = read(ANSWER, &response)?;
        if finalized.ok {
            Ok(Wrote::Written)
        } else {
            Err(StoreError::unavailable(format_args!(
                "the Actions cache did not keep {key}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::Serializer;
    use serde::ser::Error;

    use super::*;

    struct Unwritable;

    impl Serialize for Unwritable {
        fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(S::Error::custom("no JSON for this"))
        }
    }

    #[test]
    fn a_value_with_no_json_is_refused_as_corrupt() {
        assert!(matches!(
            encoded("the generation", &Unwritable),
            Err(StoreError::Corrupt { reason }) if reason == "the generation: no JSON for this"
        ));
    }
}
