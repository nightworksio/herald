use std::collections::BTreeMap;

use herald_wire::{SourceError, read};
use jiff::Timestamp;
use serde::Deserialize;

/// What a repository's tag listing is called.
pub(crate) const TAGS: &str = "the registry's tags of a repository";

/// What the registry's catalog is called.
pub(crate) const CATALOG: &str = "the registry's catalog";

/// What an image's manifest is called.
pub(crate) const MANIFEST: &str = "an image's manifest";

/// What an image's config is called.
pub(crate) const CONFIG: &str = "an image's config";

/// What the token service's answer is called.
pub(crate) const TOKEN: &str = "the registry's token";

/// The most of a token service's answer read: a token and little else.
pub(crate) const TOKEN_ANSWER: usize = 65_536;

/// The manifests a registry is asked for: OCI's and Docker's, an index of
/// platforms or one image.
pub(crate) const MANIFESTS: &str = "application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.docker.distribution.manifest.v2+json";

/// The annotation that says when an image was created.
const CREATED: &str = "org.opencontainers.image.created";

/// The annotation that marks an index's attestation manifests.
const REFERENCE_TYPE: &str = "vnd.docker.reference.type";

/// What an index's entry for something other than a platform's image says
/// of its platform.
const UNKNOWN: &str = "unknown";

/// The most characters a tag holds.
const TAG_LENGTH: usize = 128;

/// How a registry asks for a credential, as its `WWW-Authenticate` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Challenge {
    /// A bearer token, from the token service at `realm`.
    Bearer {
        realm: String,
        service: Option<String>,
        scope: Option<String>,
    },
    /// A username and password, sent as HTTP Basic.
    Basic,
}

/// The challenge `header`, a `WWW-Authenticate` value, makes, where it
/// makes one herald answers.
pub(crate) fn challenge(header: &str) -> Option<Challenge> {
    let header = header.trim_start();
    let (scheme, rest) = header
        .split_once(|c: char| c.is_ascii_whitespace())
        .unwrap_or((header, ""));
    let params = params(rest);
    if scheme.eq_ignore_ascii_case("basic") {
        return Some(Challenge::Basic);
    }
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let realm = params.get("realm")?.clone();
    Some(Challenge::Bearer {
        realm,
        service: params.get("service").cloned(),
        scope: params.get("scope").cloned(),
    })
}

/// The auth-params of a challenge, each name in lowercase.
fn params(mut rest: &str) -> BTreeMap<String, String> {
    let mut params = BTreeMap::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c == ',' || c.is_ascii_whitespace());
        let Some((name, after)) = rest.split_once('=') else {
            return params;
        };
        let after = after.trim_start();
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            unquoted(quoted)
        } else {
            let (value, next) = after.split_once(',').unwrap_or((after, ""));
            (value.trim().to_owned(), next)
        };
        params.insert(name.trim().to_ascii_lowercase(), value);
        rest = next;
    }
}

/// A quoted string's value, `quoted` holding what follows its opening
/// quote, and what follows its closing one.
fn unquoted(quoted: &str) -> (String, &str) {
    let mut value = String::new();
    let mut chars = quoted.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return (value, &quoted[at + 1..]),
            '\\' => value.extend(chars.next().map(|(_, escaped)| escaped)),
            _ => value.push(c),
        }
    }
    (value, "")
}

#[derive(Deserialize)]
struct Issued {
    #[serde(default)]
    token: String,
    #[serde(default)]
    access_token: String,
}

/// The token a token service's answer `body` issues.
///
/// # Errors
///
/// When it is not a token service's answer, or issues no token.
pub(crate) fn issued(body: &[u8]) -> Result<String, SourceError> {
    let issued: Issued = read(TOKEN, TOKEN_ANSWER, body)?;
    [issued.token, issued.access_token]
        .into_iter()
        .find(|token| !token.is_empty())
        .ok_or_else(|| SourceError::Unreadable {
            what: TOKEN,
            reason: "it issues no token".to_owned(),
        })
}

#[derive(Deserialize)]
struct Tags {
    tags: Option<Vec<String>>,
}

/// The tags a page of a repository's tag listing names.
///
/// # Errors
///
/// When it is not a tag listing, or names a tag no registry gives.
pub(crate) fn tags(limit: usize, body: &[u8]) -> Result<Vec<String>, SourceError> {
    let tags: Tags = read(TAGS, limit, body)?;
    let tags = tags.tags.unwrap_or_default();
    match tags.iter().find(|tag| !names_tag(tag)) {
        Some(tag) => Err(SourceError::Unreadable {
            what: TAGS,
            reason: format!(
                "it names \"{tag}\", which is not a tag: up to {TAG_LENGTH} letters, digits, '_', '.' and '-', not starting with '.' or '-'"
            ),
        }),
        None => Ok(tags),
    }
}

#[derive(Deserialize)]
struct Catalog {
    repositories: Option<Vec<String>>,
}

/// The repositories a page of the registry's catalog names.
///
/// # Errors
///
/// When it is not a catalog, or names a repository no registry gives.
pub(crate) fn catalog(limit: usize, body: &[u8]) -> Result<Vec<String>, SourceError> {
    let catalog: Catalog = read(CATALOG, limit, body)?;
    let repositories = catalog.repositories.unwrap_or_default();
    match repositories
        .iter()
        .find(|repository| !super::options::names_repository(repository))
    {
        Some(repository) => Err(SourceError::Unreadable {
            what: CATALOG,
            reason: format!(
                "it names \"{repository}\", which is not a repository: names of letters, digits, '.', '_' and '-' joined by '/'"
            ),
        }),
        None => Ok(repositories),
    }
}

/// Whether `link`, a `Link` header, names a next page.
pub(crate) fn next(link: Option<&str>) -> bool {
    link.is_some_and(|link| {
        link.split(',').any(|value| {
            value.split(';').skip(1).any(|param| {
                param.trim().split_once('=').is_some_and(|(name, value)| {
                    name.trim().eq_ignore_ascii_case("rel")
                        && value
                            .trim()
                            .trim_matches('"')
                            .split_ascii_whitespace()
                            .any(|rel| rel.eq_ignore_ascii_case("next"))
                })
            })
        })
    })
}

/// Whether `tag` is written as a registry gives a tag.
pub(crate) fn names_tag(tag: &str) -> bool {
    let mut bytes = tag.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric() || first == b'_')
        && tag.len() <= TAG_LENGTH
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

/// Whether `digest` is written as a content digest: an algorithm, ':' and
/// its encoded value.
pub(crate) fn names_digest(digest: &str) -> bool {
    let Some((algorithm, encoded)) = digest.split_once(':') else {
        return false;
    };
    !algorithm.is_empty()
        && algorithm.split(['+', '.', '_', '-']).all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
        && !encoded.is_empty()
        && encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'=' | b'_' | b'-'))
}

#[derive(Deserialize)]
struct Manifest {
    manifests: Option<Vec<Descriptor>>,
    config: Option<Descriptor>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Descriptor {
    digest: String,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    platform: Option<Platform>,
}

#[derive(Deserialize)]
struct Platform {
    #[serde(default)]
    os: String,
}

impl Descriptor {
    /// Whether it is an index's entry for a platform's image, not for an
    /// attestation of one.
    fn image(&self) -> bool {
        !self.annotations.contains_key(REFERENCE_TYPE)
            && self
                .platform
                .as_ref()
                .is_none_or(|platform| platform.os != UNKNOWN)
    }
}

/// What a manifest says of when its image was created, or where to read
/// on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Manifested {
    /// The time it names.
    Created(Timestamp),
    /// An index, whose first platform's manifest has this digest.
    Platform(String),
    /// An image, whose config has this digest.
    Config(String),
}

/// What the manifest `body` says of when its image was created: its own
/// or its first platform's annotation, else where to read on.
///
/// # Errors
///
/// When it is not a manifest, names no image, or names one by a digest no
/// registry gives.
pub(crate) fn manifested(limit: usize, body: &[u8]) -> Result<Manifested, SourceError> {
    let manifest: Manifest = read(MANIFEST, limit, body)?;
    if let Some(created) = annotated(&manifest.annotations) {
        return Ok(Manifested::Created(created));
    }
    let platform = manifest
        .manifests
        .as_ref()
        .and_then(|manifests| manifests.iter().find(|entry| entry.image()));
    let digest = match (platform, &manifest.config) {
        (Some(platform), _) => {
            if let Some(created) = annotated(&platform.annotations) {
                return Ok(Manifested::Created(created));
            }
            &platform.digest
        }
        (None, Some(config)) => &config.digest,
        (None, None) => {
            return Err(SourceError::Unreadable {
                what: MANIFEST,
                reason: "it names no image: neither a platform's manifest nor a config".to_owned(),
            });
        }
    };
    if !names_digest(digest) {
        return Err(SourceError::Unreadable {
            what: MANIFEST,
            reason: format!("it names \"{digest}\", which is not a digest"),
        });
    }
    Ok(match platform {
        Some(_) => Manifested::Platform(digest.clone()),
        None => Manifested::Config(digest.clone()),
    })
}

/// The time the annotations say the image was created, where they say one
/// past the epoch.
fn annotated(annotations: &BTreeMap<String, String>) -> Option<Timestamp> {
    annotations
        .get(CREATED)
        .and_then(|created| created.parse().ok())
        .filter(|created| *created > Timestamp::UNIX_EPOCH)
}

#[derive(Deserialize)]
struct Config {
    created: Option<Timestamp>,
}

/// When the image config `body` says the image was created, where it says
/// a time past the epoch.
///
/// # Errors
///
/// When it is not an image config.
pub(crate) fn created(limit: usize, body: &[u8]) -> Result<Option<Timestamp>, SourceError> {
    let config: Config = read(CONFIG, limit, body)?;
    Ok(config
        .created
        .filter(|created| *created > Timestamp::UNIX_EPOCH))
}
