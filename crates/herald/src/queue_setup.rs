//! `herald setup merge-queue`: a `[merge_queues.<name>]` table written by
//! asking for each of its keys, each with its default chosen and what the
//! forge says shown beside it (ADR-0087). It names the write credential's
//! secret and never asks for, reads or writes its value, starts no queue,
//! and changes nothing else in the config.

use std::collections::BTreeMap;
use std::num::NonZeroU8;

use herald_config::{Config, MergeQueue};
use herald_core::merging::{
    AppliedBy, ENTER_LABEL, Enter, Message, Method, RoleLost, Runs, SKEW, UPDATE_WAIT,
    UPDATE_WAIT_MOST, Updated, Vouches, WAIT, Write,
};
use herald_core::{Duration, Lasting, Permission, Printed, kind_name};
use herald_flows::BranchSaid;
use herald_kinds::{MergeAsked, Merging, Source};
use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use tokio::io::AsyncBufRead;
use toml_edit::{Item, TableLike, Value};

use self::asking::{Asking, NONE, Question, choices, default_name};
use self::forge::{forge_said, offered_methods};
use self::table::{Keys, inline, listed, text};
use super::{Connect, DONE, STOPPED};

mod asking;
mod credential;
mod forge;
mod table;

/// Why there is nothing to set a merge queue up on.
const FORGELESS: &str = "no source of kind github, gitlab or forgejo is configured; add one under [sources.<name>] first";

/// Why herald's own queue's keys were given to a queue the forge runs.
const HERALD_ONLY: &str = "--may-fail, --wait, --retry, --retry-times, --updated, --signer and --update-wait hold for herald's own queue alone; give --runs herald, or leave them out";

/// Why the forge's queue's key was given to herald's own queue.
const FORGE_ONLY: &str =
    "--role-lost holds for a queue the forge runs alone; give --runs forge, or leave it out";

/// Why the keys of a signed update were given where `updated` is another.
const SIGNED_ONLY: &str = "--signer and --update-wait hold where updated is signed alone; give --updated signed, or leave them out";

/// The refused answers to one question after which the wizard stops.
pub(crate) const MOST_REFUSED: u8 = 3;

/// The re-runs on one head offered where a queue re-runs checks.
const RETRY_TIMES: &str = "2";

/// The answers given as flags; each question none answers is asked.
#[derive(Debug, clap::Args)]
pub(crate) struct Answers {
    /// The queue, as `[merge_queues.<name>]` names it: one to edit, or a
    /// new one.
    pub(crate) name: Option<String>,
    /// The source whose repository it merges in.
    #[arg(long)]
    pub(crate) source: Option<String>,
    /// The repository, by full path, where the source is an owner.
    #[arg(long)]
    pub(crate) repository: Option<String>,
    /// The base branch it merges into.
    #[arg(long)]
    pub(crate) branch: Option<String>,
    /// Who runs it: forge or herald.
    #[arg(long)]
    pub(crate) runs: Option<String>,
    /// The label that queues a pull request.
    #[arg(long, conflicts_with = "comment")]
    pub(crate) label: Option<String>,
    /// The comment command that queues a pull request, in place of a label.
    #[arg(long)]
    pub(crate) comment: Option<String>,
    /// The least role of who queues: write, maintain or admin.
    #[arg(long)]
    pub(crate) least: Option<String>,
    /// Which head a gesture vouches for: gesture, seen or approved.
    #[arg(long)]
    pub(crate) vouches: Option<String>,
    /// How far apart the forge's stamps may be, such as 5s.
    #[arg(long)]
    pub(crate) skew: Option<String>,
    /// How it merges: merge, squash, rebase, fast-forward or repository.
    #[arg(long)]
    pub(crate) method: Option<String>,
    /// The commit message: forge or pull.
    #[arg(long)]
    pub(crate) message: Option<String>,
    /// A check that may be red or missing, once per check; "none" for none.
    #[arg(long)]
    pub(crate) may_fail: Vec<String>,
    /// How long it waits for a head's checks, such as 2h.
    #[arg(long)]
    pub(crate) wait: Option<String>,
    /// A check whose failed job is re-run, once per check; "none" for none.
    #[arg(long)]
    pub(crate) retry: Vec<String>,
    /// The most re-runs on one head.
    #[arg(long)]
    pub(crate) retry_times: Option<String>,
    /// Whether a gesture vouches for the head herald's own update made:
    /// signed, never or approved.
    #[arg(long)]
    pub(crate) updated: Option<String>,
    /// The committer a GitHub instance signs the commits it makes for;
    /// "none" for none.
    #[arg(long)]
    pub(crate) signer: Option<String>,
    /// How long it pauses, at most, for the head it updated to move, such
    /// as 10s.
    #[arg(long)]
    pub(crate) update_wait: Option<String>,
    /// What becomes of a pull request in the forge's queue once who queued
    /// it lost the least role: take-out or stay.
    #[arg(long)]
    pub(crate) role_lost: Option<String>,
    /// Where its steps are taken: command or server.
    #[arg(long)]
    pub(crate) applied_by: Option<String>,
    /// The name of the secret the write token is kept under; never the
    /// token.
    #[arg(long, conflicts_with = "write_app")]
    pub(crate) write_token: Option<String>,
    /// The GitHub App its writes are made as, by its `[github_apps]` name.
    #[arg(long)]
    pub(crate) write_app: Option<String>,
    /// The id of the App's installation; "none" to look it up.
    #[arg(long, requires = "write_app")]
    pub(crate) installation: Option<String>,
    /// Take the default of each question no flag answers, and write
    /// without asking.
    #[arg(long)]
    pub(crate) yes: bool,
    /// Print the table in place of writing it.
    #[arg(long)]
    pub(crate) print: bool,
    /// Say this output is private, so why the forge was not read is
    /// printed.
    #[arg(long)]
    pub(crate) private: bool,
}

/// `question` asked with `asked`, answered by `given`, `default` where
/// unanswered.
fn question<'q>(
    asked: &'q str,
    flag: &'static str,
    given: Option<&'q str>,
    default: Option<String>,
) -> Question<'q> {
    Question {
        asked,
        flag,
        given,
        default,
    }
}

/// What `source` gives a merge queue asking `asked`, or why not.
fn merged(source: &Source, asked: &MergeAsked<'_>) -> Result<Merging, String> {
    source.merging(asked).ok_or(FORGELESS)?.map_err(|no| no.why)
}

/// A merge queue's asking of its source: in `repository`, with `method`,
/// re-running checks where `retries`, written with `write`.
const fn asking_of<'a>(
    repository: Option<&'a str>,
    method: Option<Method>,
    retries: bool,
    write: Option<&'a Write>,
) -> MergeAsked<'a> {
    MergeAsked {
        repository,
        method,
        retries,
        write,
    }
}

/// `answer` as a length of time, as the config writes it.
fn duration(answer: &str) -> Result<String, String> {
    Duration::parse(answer)
        .map(Duration::written)
        .ok_or_else(|| "answer a number above 0 and its unit, such as 5s, 30m or 2h".to_owned())
}

/// `answer` as a length of time that may be none, as the config writes
/// it, where a queue's `update_wait` takes it.
fn update_wait(answer: &str) -> Result<String, String> {
    let lasting = Lasting::parse(answer)
        .ok_or_else(|| "answer a number and its unit, such as 0s, 10s or 1m".to_owned())?;
    MergeQueue::unwaited(lasting.0).map_or_else(|| Ok(lasting.written()), Err)
}

/// The name a new queue on `branch` of `source` is offered.
fn named(source: &str, branch: &str) -> String {
    let joined = format!("{source}-{branch}")
        .to_ascii_lowercase()
        .chars()
        .map(|one| {
            if one.is_ascii_alphanumeric() {
                one
            } else {
                '-'
            }
        })
        .collect::<String>();
    joined
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// The value `table` writes at `path`, as text.
fn was(table: Option<&dyn TableLike>, path: &[&str]) -> Option<String> {
    let (last, parents) = path.split_last()?;
    let mut at = table?;
    for parent in parents {
        at = at.get(parent)?.as_table_like()?;
    }
    let item = at.get(last)?;
    item.as_str()
        .map(ToOwned::to_owned)
        .or_else(|| item.as_integer().map(|number| number.to_string()))
}

/// The names `table` lists at `path`.
fn was_listed(table: Option<&dyn TableLike>, path: &[&str]) -> Vec<String> {
    listed_at(table, path).unwrap_or_default()
}

/// The names `table` lists at `path`, where it lists any.
fn listed_at(table: Option<&dyn TableLike>, path: &[&str]) -> Option<Vec<String>> {
    let (last, parents) = path.split_last()?;
    let mut at = table?;
    for parent in parents {
        at = at.get(parent)?.as_table_like()?;
    }
    let names = at.get(last)?.as_array()?;
    Some(
        names
            .iter()
            .filter_map(|name| name.as_str().map(ToOwned::to_owned))
            .collect(),
    )
}

/// Where a merge queue merges, as answered.
struct Sourced<'c> {
    /// The source, by name.
    name: &'c str,
    /// The source.
    source: &'c Source,
    /// What the source gives the queue.
    merging: Merging,
    /// The repository, where the source is an owner.
    repository: Option<String>,
    /// The base branch.
    branch: String,
}

/// The table of the merge queue `name` among `queues`, where written.
fn table_in<'d>(
    queues: Option<&'d dyn TableLike>,
    name: Option<&str>,
) -> Option<&'d dyn TableLike> {
    queues?.get(name?)?.as_table_like()
}

/// The source, repository and branch asked, each defaulting to `was_here`'s.
async fn sourced<'c>(
    config: &'c Config,
    was_here: Option<&dyn TableLike>,
    asked: &Answers,
    asking: &mut Asking<'_>,
) -> Result<Sourced<'c>, String> {
    let forges = config
        .sources
        .iter()
        .filter(|(_, source)| {
            source
                .merging(&asking_of(None, None, false, None))
                .is_some()
        })
        .map(|(name, source)| (name.as_str(), source))
        .collect::<Vec<_>>();
    let listed = forges
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ");
    let only = match forges.as_slice() {
        [] => return Err(FORGELESS.to_owned()),
        [(only, _)] => Some((*only).to_owned()),
        _ => None,
    };
    let source_asked = format!("The source whose repository it merges in ({listed})");
    let given = asked.source.as_deref();
    let (name, source) = asking
        .answer(
            question(
                &source_asked,
                "source",
                given,
                was(was_here, &["source"]).or(only),
            ),
            |answer| {
                forges
                    .iter()
                    .find(|(name, _)| *name == answer)
                    .copied()
                    .ok_or_else(|| format!("answer one of {listed}"))
            },
        )
        .await?;
    let given = asked.repository.as_deref();
    let (merging, repository) = match merged(source, &asking_of(None, None, false, None)) {
        Ok(merging) if given.is_none() => (merging, None),
        _ => {
            let default = was(was_here, &["repository"]);
            asking
                .answer(
                    question(
                        "The repository it merges in, by full path",
                        "repository",
                        given,
                        default,
                    ),
                    |answer| {
                        merged(source, &asking_of(Some(answer), None, false, None))
                            .map(|merging| (merging, Some(answer.to_owned())))
                    },
                )
                .await?
        }
    };
    let branch = asking
        .answer(
            question(
                "The base branch it merges into",
                "branch",
                asked.branch.as_deref(),
                was(was_here, &["branch"]),
            ),
            |answer| {
                (!answer.is_empty())
                    .then(|| answer.to_owned())
                    .ok_or_else(|| "give the branch's name, such as main".to_owned())
            },
        )
        .await?;
    Ok(Sourced {
        name,
        source,
        merging,
        repository,
        branch,
    })
}

/// The queue's name: `editing`, the one merging into the same branch, or
/// a new one asked.
async fn naming(
    config: &Config,
    sourced: &Sourced<'_>,
    (editing, given): (Option<String>, Option<&str>),
    asking: &mut Asking<'_>,
) -> Result<String, String> {
    let Sourced {
        name: source,
        merging,
        branch,
        ..
    } = sourced;
    let same = config
        .merge_queues
        .iter()
        .find(|(_, queue)| {
            queue.source.as_str() == *source
                && queue.forge.repository == merging.repository
                && queue.branch == *branch
        })
        .map(|(name, _)| name.clone());
    match (editing, same) {
        (Some(editing), Some(same)) if editing != same => Err(format!(
            "the merge queue \"{same}\" merges into {branch} of {} already; edit it with herald setup merge-queue {same}",
            merging.repository
        )),
        (Some(editing), _) => Ok(editing),
        (None, Some(same)) => {
            asking.say(&format!(
                "The merge queue {same} merges into {branch} of {} already; it is edited.",
                merging.repository
            ));
            Ok(same)
        }
        (None, None) => {
            asking
                .answer(
                    question("The queue's name", "name", given, Some(named(source, branch))),
                    |answer| {
                        if config.merge_queues.contains_key(answer) {
                            Err(format!("the merge queue \"{answer}\" merges into another branch; give another name"))
                        } else if kind_name(answer) {
                            Ok(answer.to_owned())
                        } else {
                            Err("a name is lowercase letters, digits and inner '-'".to_owned())
                        }
                    },
                )
                .await
        }
    }
}

/// Who runs the queue, asked after what the forge says of its branch:
/// herald's own offered where the forge has none there, or none herald can
/// hand a pull request to; with what the forge said, where read.
async fn running(
    (vars, connect): (&BTreeMap<String, String>, Connect<'_>),
    (candidate, name, sourced): (&Config, &str, &Sourced<'_>),
    (was_here, asked, printed): (Option<&dyn TableLike>, &Answers, Printed),
    asking: &mut Asking<'_>,
) -> Result<(String, Runs, Option<BranchSaid>), String> {
    let merging = &sourced.merging;
    asking.say(&format!(
        "The merge queue {name} merges into {} of {} on {} at {}.",
        sourced.branch, merging.repository, merging.forge, merging.api
    ));
    let read = if asking.yes && asked.source.is_none() {
        asking.say(&format!(
            "The forge was not asked about {}: under --yes it is read where --source names the source.",
            sourced.branch
        ));
        None
    } else {
        forge_said(
            (candidate, vars, connect),
            (name, &sourced.branch, printed),
            asking,
        )
        .await
    };
    if let Some(why) = merging.queueless {
        asking.say(&format!("{why}."));
    }
    let unhanded = read.as_ref().is_some_and(|said| {
        said.branched.queue == Some(false) || said.branched.queued.unhanded.is_some()
    });
    let queueless = merging.queueless.is_some() || unhanded;
    if queueless {
        asking
            .say("Without the forge's queue, herald's own runs it: runs = \"herald\" is offered.");
    }
    let unknown = read
        .as_ref()
        .is_none_or(|said| said.branched.queue.is_none())
        && asking.yes;
    let default = was(was_here, &["runs"]).or_else(|| {
        if queueless {
            Some(Runs::Herald.name().to_owned())
        } else {
            (!unknown).then(default_name::<Runs>)
        }
    });
    let (named, runs) = asking
        .choice::<Runs>(
            question("Who runs it", "runs", asked.runs.as_deref(), default),
            &choices::<Runs>(),
        )
        .await?;
    Ok((named, runs, read))
}

/// The value of `T` a question with `asked` and `flag` chooses, its
/// default `was_here`'s at `key`, else `T`'s own, added to `keys`.
async fn chosen<T: JsonSchema + DeserializeOwned + Default + PartialEq>(
    (asked, flag, given): (&str, &'static str, Option<&str>),
    (was_here, key): (Option<&dyn TableLike>, &'static str),
    asking: &mut Asking<'_>,
    keys: &mut Keys,
) -> Result<T, String> {
    let default = was(was_here, &[key]).unwrap_or_else(default_name::<T>);
    let (name, value) = asking
        .choice::<T>(question(asked, flag, given, Some(default)), &choices::<T>())
        .await?;
    keys.push((key, text(&name)));
    Ok(value)
}

/// The gesture that queues a pull request, the least role of who makes
/// it, the head it vouches for and the skew, asked and added to `keys`.
async fn gestured(
    (was_here, asked): (Option<&dyn TableLike>, &Answers),
    asking: &mut Asking<'_>,
    keys: &mut Keys,
) -> Result<(), String> {
    let (was_kind, was_text) = was(was_here, &["enter", "comment"]).map_or_else(
        || {
            (
                "label",
                was(was_here, &["enter", "label"]).unwrap_or_else(|| ENTER_LABEL.to_owned()),
            )
        },
        |comment| ("comment", comment),
    );
    let kind_given = asked
        .label
        .as_ref()
        .map(|_| "label")
        .or_else(|| asked.comment.as_ref().map(|_| "comment"));
    let (kind, _) = asking
        .choice::<String>(
            question(
                "The gesture that queues a pull request",
                "label",
                kind_given,
                Some(was_kind.to_owned()),
            ),
            &choices::<Enter>(),
        )
        .await?;
    let (asked_text, flag) = if kind == "label" {
        ("The label", "label")
    } else {
        ("The comment's command", "comment")
    };
    let gesture = asking
        .answer(
            question(
                asked_text,
                flag,
                asked.label.as_deref().or(asked.comment.as_deref()),
                (kind == was_kind).then_some(was_text),
            ),
            |answer| {
                (!answer.is_empty())
                    .then(|| answer.to_owned())
                    .ok_or_else(|| "give it, not blank".to_owned())
            },
        )
        .await?;
    keys.push((
        "enter",
        Item::Value(inline(vec![(kind.as_str(), Value::from(gesture))])),
    ));
    let least = (
        "The least role of who queues",
        "least",
        asked.least.as_deref(),
    );
    chosen::<Permission>(least, (was_here, "least"), asking, keys).await?;
    let vouches = (
        "Which head a gesture vouches for",
        "vouches",
        asked.vouches.as_deref(),
    );
    chosen::<Vouches>(vouches, (was_here, "vouches"), asking, keys).await?;
    let skew = asking
        .answer(
            question(
                "How far apart the forge's stamps of a gesture and a push may be",
                "skew",
                asked.skew.as_deref(),
                Some(was(was_here, &["skew"]).unwrap_or_else(|| Duration(SKEW).written())),
            ),
            duration,
        )
        .await?;
    keys.push(("skew", text(&skew)));
    Ok(())
}

pub(crate) async fn setting_up(
    config: &Config,
    path: &str,
    (vars, connect): (&BTreeMap<String, String>, Connect<'_>),
    asked: &Answers,
    answers: &mut (dyn AsyncBufRead + Send + Unpin),
    out: &mut dyn std::io::Write,
) -> Result<u8, String> {
    let file = super::text(path)?;
    let document = crate::edited::document(&file)?;
    let queues = document.get("merge_queues").and_then(Item::as_table_like);
    let editing = asked
        .name
        .as_deref()
        .filter(|name| config.merge_queues.contains_key(*name))
        .map(ToOwned::to_owned);
    let mut asking = Asking {
        answers,
        out,
        yes: asked.yes,
    };
    let sourced = sourced(
        config,
        table_in(queues, editing.as_deref()),
        asked,
        &mut asking,
    )
    .await?;
    let name = naming(
        config,
        &sourced,
        (editing, asked.name.as_deref()),
        &mut asking,
    )
    .await?;
    let was_here = table_in(queues, Some(&name));
    let mut keys: Keys = vec![("source", text(sourced.name))];
    if let Some(repository) = &sourced.repository {
        keys.push(("repository", text(repository)));
    }
    keys.push(("branch", text(&sourced.branch)));
    let candidate = super::loaded(path, &table::placed(&file, &name, &keys)?)?;
    let printed = Printed::of(asked.private, config.cli.private);
    let (runs_name, runs, said) = running(
        (vars, connect),
        (&candidate, &name, &sourced),
        (was_here, asked, printed),
        &mut asking,
    )
    .await?;
    keys.push(("runs", text(&runs_name)));
    gestured((was_here, asked), &mut asking, &mut keys).await?;
    let repository = sourced.repository.as_deref();
    let own = said
        .as_ref()
        .and_then(|said| said.branched.queued.method)
        .filter(|_| runs == Runs::Forge)
        .map(|method| method.name().to_owned());
    let (method_name, method) = asking
        .choice::<Method>(
            question(
                "How it merges",
                "method",
                asked.method.as_deref(),
                was(was_here, &["method"]).or(own),
            ),
            &offered_methods(sourced.source, repository, (runs, said.as_ref())),
        )
        .await?;
    keys.push(("method", text(&method_name)));
    if method.messaged() {
        let message = ("The commit message", "message", asked.message.as_deref());
        chosen::<Message>(message, (was_here, "message"), &mut asking, &mut keys).await?;
    }
    ran_by(
        runs,
        (&sourced, repository),
        (was_here, asked),
        &mut asking,
        &mut keys,
    )
    .await?;
    let applied = (
        "Where its steps are taken",
        "applied-by",
        asked.applied_by.as_deref(),
    );
    chosen::<AppliedBy>(applied, (was_here, "applied_by"), &mut asking, &mut keys).await?;
    let secret = credential::written(
        (config, &sourced, repository),
        (was_here, asked),
        &mut asking,
        &mut keys,
    )
    .await?;
    finish(
        (path, &file, &name),
        &keys,
        secret.as_deref(),
        asked.print,
        &mut asking,
    )
    .await
}

/// The keys of who runs the queue asked: herald's own queue's, or what
/// becomes of a pull request in the forge's; a key of the other refused.
async fn ran_by(
    runs: Runs,
    (sourced, repository): (&Sourced<'_>, Option<&str>),
    (was_here, asked): (Option<&dyn TableLike>, &Answers),
    asking: &mut Asking<'_>,
    keys: &mut Keys,
) -> Result<(), String> {
    if runs == Runs::Herald {
        if asked.role_lost.is_some() {
            return Err(FORGE_ONLY.to_owned());
        }
        herald_keys((sourced, repository), (was_here, asked), asking, keys).await
    } else if !asked.may_fail.is_empty()
        || asked.wait.is_some()
        || !asked.retry.is_empty()
        || asked.retry_times.is_some()
        || asked.updated.is_some()
        || asked.signer.is_some()
        || asked.update_wait.is_some()
    {
        Err(HERALD_ONLY.to_owned())
    } else {
        let role_lost = (
            "What becomes of a pull request in the forge's queue once who queued it no longer holds the least role",
            "role-lost",
            asked.role_lost.as_deref(),
        );
        chosen::<RoleLost>(role_lost, (was_here, "role_lost"), asking, keys).await?;
        Ok(())
    }
}

/// The keys of herald's own queue asked: the checks that may fail, the
/// wait, what its own update vouches for, and the re-runs where the forge
/// takes them.
async fn herald_keys(
    (sourced, repository): (&Sourced<'_>, Option<&str>),
    (was_here, asked): (Option<&dyn TableLike>, &Answers),
    asking: &mut Asking<'_>,
    keys: &mut Keys,
) -> Result<(), String> {
    let may_fail = asking
        .list(
            question(
                "The checks that may be red or missing",
                "may-fail",
                None,
                None,
            ),
            &asked.may_fail,
            &was_listed(was_here, &["may_fail"]),
        )
        .await?;
    if !may_fail.is_empty() {
        keys.push(("may_fail", Item::Value(listed(&may_fail))));
    }
    let wait = asking
        .answer(
            question(
                "How long it waits for a head's checks",
                "wait",
                asked.wait.as_deref(),
                Some(was(was_here, &["wait"]).unwrap_or_else(|| Duration(WAIT).written())),
            ),
            duration,
        )
        .await?;
    keys.push(("wait", text(&wait)));
    updating(&sourced.merging, (was_here, asked), asking, keys).await?;
    if let Err(refused) = merged(sourced.source, &asking_of(repository, None, true, None)) {
        return if asked.retry.is_empty() && asked.retry_times.is_none() {
            Ok(())
        } else {
            Err(refused)
        };
    }
    let checks = asking
        .list(
            question("The checks whose failed job is re-run", "retry", None, None),
            &asked.retry,
            &was_listed(was_here, &["retries", "checks"]),
        )
        .await?;
    if checks.is_empty() {
        return Ok(());
    }
    let times = asking
        .answer(
            question(
                "The most re-runs on one head",
                "retry-times",
                asked.retry_times.as_deref(),
                Some(
                    was(was_here, &["retries", "times"]).unwrap_or_else(|| RETRY_TIMES.to_owned()),
                ),
            ),
            |answer| {
                answer
                    .parse::<NonZeroU8>()
                    .map_err(|_| "answer a number from 1 to 255".to_owned())
            },
        )
        .await?;
    keys.push((
        "retries",
        Item::Value(inline(vec![
            ("checks", listed(&checks)),
            ("times", Value::from(i64::from(times.get()))),
        ])),
    ));
    Ok(())
}

/// Whether a gesture vouches for the head herald's own update made, asked
/// after why a signed one never does on `merging`, where it never does,
/// `"signed"` chosen where herald can prove that signature and `"never"`
/// elsewhere; with `updated = "signed"`, the committer the forge signs
/// for, on GitHub, and how long herald pauses for that head; each added to
/// `keys`.
async fn updating(
    merging: &Merging,
    (was_here, asked): (Option<&dyn TableLike>, &Answers),
    asking: &mut Asking<'_>,
    keys: &mut Keys,
) -> Result<(), String> {
    let was_signer = was(was_here, &["signer"]);
    let signer = asked.signer.as_deref().or(was_signer.as_deref());
    if let Some(why) = herald_flows::unsigned(merging, signer.filter(|signer| *signer != NONE)) {
        asking.say(&format!("{why}."));
    }
    let updated = (
        "Whether a gesture vouches for the head herald's own update made from the head it vouched for",
        "updated",
        asked.updated.as_deref(),
    );
    let unasked = signer.filter(|signer| *signer != NONE);
    let default = was(was_here, &["updated"]).unwrap_or_else(|| {
        MergeQueue::updated_by_default(merging, unasked)
            .name()
            .to_owned()
    });
    let (named, chose) = asking
        .choice::<Updated>(
            question(updated.0, updated.1, updated.2, Some(default)),
            &choices::<Updated>(),
        )
        .await?;
    keys.push(("updated", text(&named)));
    if chose != Updated::Signed {
        return if asked.signer.is_none() && asked.update_wait.is_none() {
            Ok(())
        } else {
            Err(SIGNED_ONLY.to_owned())
        };
    }
    if MergeQueue::signs(merging.forge) || asked.signer.is_some() {
        let none = herald_wire::github::web_flow(&merging.api)
            .map_or_else(|| "no such committer".to_owned(), ToOwned::to_owned);
        let signer_asked = format!(
            "The committer {} signs the commits it makes for, which you vouch is the instance's own and no user's; {NONE} for {none}",
            merging.api
        );
        let signer = asking
            .answer(
                question(
                    &signer_asked,
                    "signer",
                    asked.signer.as_deref(),
                    Some(was_signer.unwrap_or_else(|| NONE.to_owned())),
                ),
                |answer| match answer {
                    NONE => Ok(None),
                    _ => MergeQueue::unsigning(merging.forge, answer)
                        .map_or_else(|| Ok(Some(answer.to_owned())), |why| Err(why.to_owned())),
                },
            )
            .await?;
        if let Some(signer) = signer {
            keys.push(("signer", text(&signer)));
        }
    }
    let wait_asked = format!(
        "How long it pauses in all, at most, for the head of a pull request it updated to move, up to {}; {} reads it once",
        Duration(UPDATE_WAIT_MOST).written(),
        Lasting(SignedDuration::ZERO).written()
    );
    let wait = asking
        .answer(
            question(
                &wait_asked,
                "update-wait",
                asked.update_wait.as_deref(),
                Some(
                    was(was_here, &["update_wait"])
                        .unwrap_or_else(|| Lasting(UPDATE_WAIT).written()),
                ),
            ),
            update_wait,
        )
        .await?;
    keys.push(("update_wait", text(&wait)));
    Ok(())
}

/// The table `keys` writes for the queue `name` shown, then printed, or
/// written into the config at `path` once confirmed and read back whole.
async fn finish(
    (path, file, name): (&str, &str, &str),
    keys: &Keys,
    secret: Option<&str>,
    print: bool,
    asking: &mut Asking<'_>,
) -> Result<u8, String> {
    let rewritten = table::placed(file, name, keys)?;
    super::loaded(path, &rewritten).map_err(|why| format!("{why}\nNothing was written."))?;
    let shown = table::shown(name, keys);
    if print {
        let _ = write!(asking.out, "{shown}");
        return Ok(DONE);
    }
    asking.say("");
    let _ = write!(asking.out, "{shown}");
    if !asking
        .confirmed(&format!("Write the merge queue {name} into {path}?"))
        .await
    {
        asking.say("Nothing was written.");
        return Ok(STOPPED);
    }
    super::rewrite(path, rewritten)?;
    asking.say(&format!("Wrote the merge queue {name} into {path}."));
    if let Some(secret) = secret {
        asking.say(&format!(
            "Store its write token as the secret {secret} where its steps are taken; herald reads it only to take a step."
        ));
    }
    asking.say(&format!(
        "Nothing is queued or merged yet. See what it would do: herald --config {path} merge-queue {name}"
    ));
    asking.say("herald doctor names anything it still lacks.");
    Ok(DONE)
}
