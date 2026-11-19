//! The static signed log committed to its branch in one commit, the branch
//! moved to it only where its head is still the commit's parent
//! (ADR-0060).

use herald_core::{Publication, status};
use herald_kinds::GITHUB_REPOSITORY;
use herald_peers::digest::content_digest;
use herald_peers::pages::Log;
use herald_ports::{Http, Request, Response};
use herald_wire::git::{self, Branch, Entry};
use herald_wire::github::GITHUB;

use crate::announce::Stop;
use crate::prepare::PreparedPages;

/// The commit writing `files`, each by its path, to the branch `name` of
/// `pages`, and how many entries it wrote or removed: where the branch's
/// head is the commit `held` names, the files whose digest `held` does not
/// hold, over the head's tree, with each file `held` holds that `files` no
/// longer does removed; else every file, over no tree.
///
/// # Errors
///
/// The branch's repository is unknown, GitHub could not be reached or
/// refused a step, or another run moved the branch first.
pub(super) async fn committed<H: Http>(
    http: &H,
    (pages, name): (&PreparedPages, &str),
    files: &[(String, String)],
    held: &Publication,
    log: Log,
) -> Result<(String, usize), Stop> {
    let (Some(repository), Some(token)) = (pages.repository.as_deref(), pages.token.as_ref())
    else {
        return Err(Stop::Refused(format!(
            "{GITHUB_REPOSITORY} is not set; name the repository in [federation.pages], or run herald in the repository's CI"
        )));
    };
    let branch = Branch {
        api: &pages.api,
        repository,
        branch: name,
        token,
    };
    let answered = sent(http, branch.head()).await?;
    let tip = match answered.status {
        status::NOT_FOUND => None,
        _ => Some(read(&answered, git::head, "the branch's head")?),
    };
    let kept = tip.is_some() && held.commit == tip;
    let base = match tip.as_deref().filter(|_| kept) {
        Some(tip) => Some(read(
            &sent(http, branch.commit_of(tip)).await?,
            git::tree,
            "the head's commit",
        )?),
        None => None,
    };
    let mut entries: Vec<Entry<'_>> = files
        .iter()
        .filter(|(path, body)| {
            !kept || held.files.get(path) != Some(&content_digest(body.as_bytes()))
        })
        .map(|(path, body)| Entry {
            path,
            text: Some(body),
        })
        .collect();
    entries.extend(
        held.files
            .keys()
            .filter(|path| kept && !files.iter().any(|(written, _)| written == *path))
            .map(|path| Entry { path, text: None }),
    );
    let tree = read(
        &sent(http, branch.tree(base.as_deref(), &entries)).await?,
        git::made,
        "the tree",
    )?;
    let message = format!("herald: static log {}", log.sequence);
    let commit = read(
        &sent(http, branch.commit(&message, &tree, tip.as_deref())).await?,
        git::made,
        "the commit",
    )?;
    let moving = match tip {
        Some(_) => branch.moved(&commit),
        None => branch.made(&commit),
    };
    let moved = sent(http, moving).await?;
    match moved.status {
        _ if moved.succeeded() => Ok((commit, entries.len())),
        status::CONFLICT | status::UNPROCESSABLE => Err(Stop::Store(
            "another run moved the static log's branch first; the next run writes the log again"
                .to_owned(),
        )),
        _ => Err(refused(&moved, "the branch")),
    }
}

/// The answer to `request`.
async fn sent<H: Http>(http: &H, request: Request) -> Result<Response, Stop> {
    http.send(request)
        .await
        .map_err(|error| Stop::Unreachable(error.to_string()))
}

/// What `reader` reads of `response`, GitHub's answer for `what`.
fn read(
    response: &Response,
    reader: fn(&[u8]) -> Option<String>,
    what: &str,
) -> Result<String, Stop> {
    if !response.succeeded() {
        return Err(refused(response, what));
    }
    reader(&response.body)
        .ok_or_else(|| Stop::Refused(format!("{GITHUB}'s answer for {what} could not be read")))
}

/// Why GitHub's `response` for `what` stopped the writing.
fn refused(response: &Response, what: &str) -> Stop {
    Stop::Refused(format!(
        "{GITHUB} answered {} for {what} of the static log",
        response.status
    ))
}
