//! GitLab's vulnerabilities, which GitLab Ultimate raises: its
//! vulnerability webhook, and the newest detected through its GraphQL API.

use herald_core::Address;
use herald_ports::Request;
use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{AlertAction, ForgeAlert, Severity, Tool};
use crate::forge::{Repository, Token, authorized};
use crate::{Forge, wire::read};

/// The name GitLab gives its vulnerability webhook's deliveries.
pub(crate) const HOOK: &str = "Vulnerability Hook";
/// What GitLab's vulnerability event is read as.
pub(crate) const HOOKED: &str = "GitLab's vulnerability event";
/// What GitLab's listing of vulnerabilities is read as.
pub(crate) const LISTED: &str = "GitLab's vulnerabilities";
/// What a vulnerability's page is written under, after its project's.
const PAGED: &str = "/-/security/vulnerabilities/";
/// What a vulnerability's GraphQL listing asks of each.
const FIELDS: &str = "title state severity reportType detectedAt confirmedAt resolvedAt dismissedAt updatedAt webUrl primaryIdentifier { name } location { ... on VulnerabilityLocationSast { file } ... on VulnerabilityLocationSecretDetection { file } ... on VulnerabilityLocationDependencyScanning { file dependency { package { name } } } ... on VulnerabilityLocationContainerScanning { image dependency { package { name } } } }";
/// What an overview's listing asks of each vulnerability: what it is, never
/// where it was found, so no secret's file is ever asked (ADR-0067).
pub(crate) const OPENED_FIELDS: &str = "title state severity reportType detectedAt confirmedAt resolvedAt dismissedAt updatedAt webUrl primaryIdentifier { name } location { ... on VulnerabilityLocationDependencyScanning { dependency { package { name } } } ... on VulnerabilityLocationContainerScanning { dependency { package { name } } } }";

/// The tool GitLab's report type `name` is read as.
fn tool_of(name: &str) -> Tool {
    match name.to_ascii_lowercase().as_str() {
        "sast" => Tool::CodeScanning,
        "dependency_scanning" => Tool::DependencyScanning,
        "secret_detection" => Tool::SecretScanning,
        "container_scanning" | "cluster_image_scanning" | "container_scanning_for_registry" => {
            Tool::ContainerScanning
        }
        "dast" => Tool::Dast,
        "coverage_fuzzing" | "api_fuzzing" => Tool::Fuzzing,
        _ => Tool::Other,
    }
}

/// What a vulnerability's state, in any case, says of it.
fn action_of(state: &str) -> Option<AlertAction> {
    match state.to_ascii_lowercase().as_str() {
        "detected" => Some(AlertAction::Created),
        "confirmed" => Some(AlertAction::Confirmed),
        "resolved" => Some(AlertAction::Fixed),
        "dismissed" => Some(AlertAction::Dismissed),
        _ => None,
    }
}

/// A package a vulnerability names.
#[derive(Deserialize)]
struct Package {
    name: Option<String>,
}

/// A dependency a vulnerability is found in.
#[derive(Deserialize)]
struct Dependency {
    package: Option<Package>,
}

/// Where a vulnerability was found.
#[derive(Deserialize)]
struct Location {
    file: Option<String>,
    image: Option<String>,
    dependency: Option<Dependency>,
}

/// One of a vulnerability's identifiers.
#[derive(Deserialize)]
struct Identifier {
    name: String,
}

/// The project a group's listing names a vulnerability of.
#[derive(Deserialize)]
struct Project {
    #[serde(rename = "fullPath")]
    full_path: String,
}

/// A vulnerability, as GitLab's webhook and its GraphQL API describe it.
#[derive(Deserialize)]
struct Vulnerability {
    #[serde(alias = "webUrl")]
    url: String,
    title: Option<String>,
    state: String,
    severity: Option<String>,
    #[serde(alias = "reportType")]
    report_type: Option<String>,
    #[serde(alias = "detectedAt")]
    created_at: Timestamp,
    #[serde(alias = "confirmedAt")]
    confirmed_at: Option<Timestamp>,
    #[serde(alias = "resolvedAt")]
    resolved_at: Option<Timestamp>,
    #[serde(alias = "dismissedAt")]
    dismissed_at: Option<Timestamp>,
    #[serde(alias = "updatedAt")]
    updated_at: Option<Timestamp>,
    #[serde(default)]
    identifiers: Vec<Identifier>,
    #[serde(rename = "primaryIdentifier")]
    primary: Option<Identifier>,
    location: Option<Location>,
    project: Option<Project>,
}

/// Why a vulnerability is not read where its page is not one: the page is
/// never quoted, as a group's listing may name a project no audience may
/// see (ADR-0067).
pub(super) const UNPAGED: &str = "a vulnerability's web address is not a vulnerability's page";

/// The project page and number a vulnerability's page `url` names.
fn paged(what: &'static str, url: &str) -> Result<(String, u64), SourceError> {
    url.rsplit_once(PAGED)
        .and_then(|(page, number)| Some((page.to_owned(), number.parse().ok()?)))
        .filter(|(page, _)| Address::parse(page).is_some())
        .ok_or_else(|| SourceError::Unreadable {
            what,
            reason: UNPAGED.to_owned(),
        })
}

impl Vulnerability {
    /// The vulnerability, as it is once it did `action`.
    fn read(self, what: &'static str, action: AlertAction) -> Result<ForgeAlert, SourceError> {
        let (_, number) = paged(what, &self.url)?;
        let occurred = match action {
            AlertAction::Confirmed => self.confirmed_at,
            AlertAction::Fixed => self.resolved_at,
            AlertAction::Dismissed => self.dismissed_at,
            AlertAction::Created | AlertAction::Reopened => Some(self.created_at),
        }
        .or(self.updated_at)
        .unwrap_or(self.created_at);
        let location = self.location;
        let package = location
            .as_ref()
            .and_then(|location| location.dependency.as_ref())
            .and_then(|dependency| dependency.package.as_ref())
            .and_then(|package| package.name.clone());
        let path = location.and_then(|location| location.file.or(location.image));
        let rule = self
            .primary
            .or_else(|| self.identifiers.into_iter().next())
            .map(|identifier| identifier.name);
        Ok(ForgeAlert {
            tool: tool_of(self.report_type.as_deref().unwrap_or_default()),
            number,
            action,
            severity: self.severity.as_deref().and_then(Severity::named),
            rule,
            summary: self.title,
            package,
            ecosystem: None,
            path,
            resolution: None,
            url: self.url,
            occurred,
            repository: self.project.map(|project| project.full_path),
        })
    }
}

/// GitLab's vulnerability event.
#[derive(Deserialize)]
struct Hooked {
    object_attributes: Attributes,
}

/// What GitLab's vulnerability event says of the vulnerability and its
/// project.
#[derive(Deserialize)]
struct Attributes {
    project_id: u64,
    #[serde(flatten)]
    vulnerability: Vulnerability,
}

/// What GitLab's vulnerability event `payload` names: the project's full
/// path, read from the vulnerability's page under `web`, the instance's
/// web root, the project, and the vulnerability where its state is one
/// herald reads.
pub(super) fn delivered(
    web: &str,
    payload: &[u8],
) -> Result<(String, Repository, Option<ForgeAlert>), SourceError> {
    let hooked: Hooked = read(HOOKED, payload)?;
    let attributes = hooked.object_attributes;
    let (page, _) = paged(HOOKED, &attributes.vulnerability.url)?;
    let path = under(web, &page).ok_or_else(|| SourceError::Unreadable {
        what: HOOKED,
        reason: format!("{page} is not a project of {web}"),
    })?;
    let name = path.rsplit('/').next().unwrap_or_default().to_owned();
    let repository = Repository {
        id: attributes.project_id,
        name,
        page: Some(page),
    };
    let alert = match action_of(&attributes.vulnerability.state) {
        Some(action) => Some(attributes.vulnerability.read(HOOKED, action)?),
        None => None,
    };
    Ok((path, repository, alert))
}

/// The path of the project at `page` under `web`, ignoring case, where
/// `page` is under it.
fn under(web: &str, page: &str) -> Option<String> {
    let web = web.trim_end_matches('/');
    let (root, path) = (page.get(..web.len())?, page.get(web.len()..)?);
    let path = path.strip_prefix('/')?;
    (root.eq_ignore_ascii_case(web) && !path.is_empty()).then(|| path.to_owned())
}

/// The request asking GitLab's GraphQL API at `endpoint` for the newest
/// `page` vulnerabilities detected in the project at `path`, carrying
/// `token`, reading at most `parse` bytes.
pub(super) fn listing(
    endpoint: &str,
    (path, token): (&str, Option<&Token>),
    (page, parse): (usize, usize),
) -> Request {
    let query = format!(
        "query($path: ID!, $page: Int!) {{ project(fullPath: $path) {{ vulnerabilities(first: $page, sort: detected_desc) {{ nodes {{ {FIELDS} }} }} }} }}"
    );
    let body = json!({ "query": query, "variables": { "path": path, "page": page } });
    authorized(
        Forge::GitLab,
        crate::graphql::posted(endpoint, &body, parse),
        token,
    )
}

/// The nodes of a listing of vulnerabilities.
#[derive(Deserialize)]
struct Nodes {
    nodes: Vec<Vulnerability>,
}

/// A project, as a listing of its vulnerabilities names it.
#[derive(Deserialize)]
struct Listed {
    vulnerabilities: Option<Nodes>,
}

/// What a listing's data holds.
#[derive(Deserialize)]
struct Data {
    project: Option<Listed>,
}

/// A listing of vulnerabilities, as GitLab's GraphQL API answers it.
#[derive(Deserialize)]
struct Envelope {
    data: Option<Data>,
    #[serde(default)]
    errors: Vec<Value>,
}

/// The vulnerabilities GitLab's GraphQL listing `body` holds, parsing no
/// more than `limit` bytes, each as its state says.
pub(super) fn listed(limit: usize, body: &[u8]) -> Result<Vec<ForgeAlert>, SourceError> {
    let envelope: Envelope = herald_wire::read(LISTED, limit, body)?;
    let nodes = envelope
        .data
        .and_then(|data| data.project)
        .and_then(|project| project.vulnerabilities)
        .ok_or_else(|| SourceError::Unreadable {
            what: LISTED,
            reason: format!(
                "the answer holds no vulnerabilities, which need GitLab Ultimate and a token that may read them: {}",
                Value::from(envelope.errors)
            ),
        })?;
    let mut alerts = Vec::new();
    for vulnerability in nodes.nodes {
        if let Some(action) = action_of(&vulnerability.state) {
            alerts.push(vulnerability.read(LISTED, action)?);
        }
    }
    Ok(alerts)
}

/// A page of open vulnerabilities, and the cursor of the next where one
/// follows.
pub(crate) type OpenedPage = (Vec<ForgeAlert>, Option<String>);

/// What GitLab's listing of open vulnerabilities is read as.
pub(crate) const OPENED: &str = "GitLab's open vulnerabilities";

/// The request asking GitLab's GraphQL API at `endpoint` for a page of
/// `page` vulnerabilities detected or confirmed in the project at `path`,
/// or in the group there with its projects' paths where `group` says,
/// after `after`, carrying `token`, reading at most `parse` bytes.
pub(crate) fn opened_listing(
    endpoint: &str,
    (group, path, token): (bool, &str, Option<&Token>),
    (after, page, parse): (Option<&str>, usize, usize),
) -> Request {
    let (holder, project) = if group {
        ("group", " project { fullPath }")
    } else {
        ("project", "")
    };
    let query = format!(
        "query($path: ID!, $page: Int!, $after: String) {{ {holder}(fullPath: $path) {{ vulnerabilities(first: $page, after: $after, state: [DETECTED, CONFIRMED], sort: detected_desc) {{ nodes {{ {OPENED_FIELDS}{project} }} pageInfo {{ hasNextPage endCursor }} }} }} }}"
    );
    let body =
        json!({ "query": query, "variables": { "path": path, "page": page, "after": after } });
    authorized(
        Forge::GitLab,
        crate::graphql::posted(endpoint, &body, parse),
        token,
    )
}

/// Where a page of a listing ends.
#[derive(Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    more: bool,
    #[serde(rename = "endCursor")]
    end: Option<String>,
}

/// A page of open vulnerabilities.
#[derive(Deserialize)]
struct Opened {
    nodes: Vec<Vulnerability>,
    #[serde(rename = "pageInfo")]
    page: PageInfo,
}

/// A project or group, as a listing of its open vulnerabilities names it.
#[derive(Deserialize)]
struct Holder {
    vulnerabilities: Option<Opened>,
}

/// What a listing of open vulnerabilities holds.
#[derive(Deserialize)]
struct OpenedData {
    project: Option<Holder>,
    group: Option<Holder>,
}

/// A listing of open vulnerabilities, as GitLab's GraphQL API answers it.
#[derive(Deserialize)]
struct OpenedEnvelope {
    data: Option<OpenedData>,
}

/// The open vulnerabilities GitLab's GraphQL page `body` holds, parsing no
/// more than `limit` bytes, and the cursor of the next page where one
/// follows; none where GitLab answered no vulnerabilities, as it does for
/// a token that may not read them or an instance without Ultimate.
///
/// # Errors
///
/// Where the body is no listing, or a vulnerability's page names none.
pub(crate) fn opened(limit: usize, body: &[u8]) -> Result<Option<OpenedPage>, SourceError> {
    let envelope: OpenedEnvelope = herald_wire::read(OPENED, limit, body)?;
    let Some(listed) = envelope
        .data
        .and_then(|data| data.project.or(data.group))
        .and_then(|holder| holder.vulnerabilities)
    else {
        return Ok(None);
    };
    let mut alerts = Vec::new();
    for vulnerability in listed.nodes {
        if let Some(action) = action_of(&vulnerability.state) {
            alerts.push(vulnerability.read(OPENED, action)?);
        }
    }
    let next = listed.page.end.filter(|_| listed.page.more);
    Ok(Some((alerts, next)))
}
