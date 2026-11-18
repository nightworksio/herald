//! GitLab's pipelines.

use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{Delivered, ForgeRun, Ran};
use crate::forge::Repository;
use crate::options::RunChoice;
use crate::wire::read;
use crate::{Conclusion, RunAction};

/// What GitLab's pipeline hook is read as.
pub(super) const RUN: &str = "GitLab's pipeline hook";
/// What GitLab's listing of pipelines is read as.
pub(super) const RUNS: &str = "GitLab's pipelines";
/// What a pipeline with no name is called.
const UNNAMED: &str = "pipeline";
/// What GitLab names a pipeline a merge request started.
const MERGE_REQUEST: &str = "merge_request_event";
/// What a run a pull request started is called on every forge.
const PULL_REQUEST: &str = "pull_request";

/// What a pipeline's status, as GitLab names it, says it did and how it
/// ended: nothing for a status GitLab does not name.
fn status(status: &str) -> Option<(RunAction, Option<Conclusion>)> {
    let done = |conclusion| Some((RunAction::Completed, Some(conclusion)));
    match status {
        "created" | "waiting_for_resource" | "preparing" | "pending" | "scheduled" => {
            Some((RunAction::Requested, None))
        }
        "running" | "canceling" => Some((RunAction::InProgress, None)),
        "success" => done(Conclusion::Success),
        "failed" => done(Conclusion::Failure),
        "canceled" => done(Conclusion::Cancelled),
        "skipped" => done(Conclusion::Skipped),
        "manual" => done(Conclusion::ActionRequired),
        _ => None,
    }
}

/// What started a pipeline, as every forge's run names it.
fn trigger(source: String) -> String {
    if source == MERGE_REQUEST {
        PULL_REQUEST.to_owned()
    } else {
        source
    }
}

/// What every description of a pipeline names.
struct Pipeline {
    id: u64,
    iid: Option<u64>,
    name: Option<String>,
    reference: Option<String>,
    sha: Option<String>,
    status: String,
    source: Option<String>,
    url: String,
    actor: Option<String>,
    occurred: Timestamp,
}

impl Pipeline {
    fn run(self) -> Option<ForgeRun> {
        let (action, conclusion) = status(&self.status)?;
        Some(ForgeRun {
            ran: Ran::Run,
            id: self.id,
            attempt: None,
            number: self.iid,
            workflow: self.name.unwrap_or_else(|| UNNAMED.to_owned()),
            action,
            conclusion,
            trigger: self.source.map(trigger),
            branch: self.reference,
            commit: self.sha,
            actor: self.actor,
            url: self.url,
            title: None,
            occurred: self.occurred,
        })
    }
}

/// A pipeline, as GitLab's hook describes it.
#[derive(Deserialize)]
struct Attributes {
    id: u64,
    iid: Option<u64>,
    name: Option<String>,
    #[serde(rename = "ref")]
    reference: Option<String>,
    sha: Option<String>,
    status: String,
    source: Option<String>,
    url: Option<String>,
    #[serde(default, deserialize_with = "crate::gitlab::when")]
    created_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "crate::gitlab::when")]
    finished_at: Option<Timestamp>,
}

/// Who a hook names.
#[derive(Deserialize)]
struct User {
    username: String,
}

/// A project, as GitLab's pipeline hook names it.
#[derive(Deserialize)]
struct Project {
    id: u64,
    name: String,
    path_with_namespace: String,
    web_url: String,
}

/// GitLab's pipeline hook.
#[derive(Deserialize)]
struct Hook {
    object_attributes: Attributes,
    user: Option<User>,
    project: Project,
}

/// What GitLab's pipeline hook `payload` names.
pub(super) fn delivered(payload: &[u8]) -> Result<Delivered, SourceError> {
    let hook: Hook = read(RUN, payload)?;
    let attributes = hook.object_attributes;
    let occurred = attributes
        .finished_at
        .or(attributes.created_at)
        .ok_or_else(|| SourceError::Unreadable {
            what: RUN,
            reason: "the pipeline has no time".to_owned(),
        })?;
    let project = hook.project;
    let url = attributes
        .url
        .unwrap_or_else(|| format!("{}/-/pipelines/{}", project.web_url, attributes.id));
    let pipeline = Pipeline {
        id: attributes.id,
        iid: attributes.iid,
        name: attributes.name,
        reference: attributes.reference,
        sha: attributes.sha,
        status: attributes.status,
        source: attributes.source,
        url,
        actor: hook.user.map(|user| user.username),
        occurred,
    };
    Ok(Delivered {
        path: project.path_with_namespace,
        repository: Repository {
            id: project.id,
            name: project.name,
            page: Some(project.web_url),
        },
        run: pipeline.run(),
    })
}

/// A pipeline, as GitLab's listing describes it.
#[derive(Deserialize)]
struct Listed {
    id: u64,
    iid: Option<u64>,
    name: Option<String>,
    #[serde(rename = "ref")]
    reference: Option<String>,
    sha: Option<String>,
    status: String,
    source: Option<String>,
    web_url: String,
    updated_at: Timestamp,
}

/// The pipelines in GitLab's listing `body`, parsing no more than `limit`
/// bytes.
pub(super) fn listed(limit: usize, body: &[u8]) -> Result<Vec<ForgeRun>, SourceError> {
    let listed: Vec<Listed> = herald_wire::read(RUNS, limit, body)?;
    Ok(listed
        .into_iter()
        .filter_map(|listed| {
            Pipeline {
                id: listed.id,
                iid: listed.iid,
                name: listed.name,
                reference: listed.reference,
                sha: listed.sha,
                status: listed.status,
                source: listed.source,
                url: listed.web_url,
                actor: None,
                occurred: listed.updated_at,
            }
            .run()
        })
        .collect())
}

/// The path, under a project, of its newest `page` pipelines, asking GitLab
/// for only those `choice` announces where one parameter can say so.
pub(super) fn runs_path(choice: &RunChoice, page: usize) -> String {
    let path = format!("/pipelines?per_page={page}&order_by=updated_at&sort=desc");
    let scope = match choice.only_action() {
        Some(RunAction::Completed) => Some("finished"),
        Some(RunAction::InProgress) => Some("running"),
        Some(RunAction::Requested) => Some("pending"),
        None => None,
    };
    let source = choice.only_trigger().map(|trigger| {
        if trigger == PULL_REQUEST {
            MERGE_REQUEST
        } else {
            trigger
        }
    });
    super::queried(
        path,
        &[
            ("scope", scope),
            ("source", source),
            ("ref", choice.only_branch()),
        ],
    )
}
