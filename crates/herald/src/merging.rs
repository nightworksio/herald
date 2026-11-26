//! `herald merge-queue`: each merge queue's entries read from its forge,
//! each with what the queue does next and why; a dry run that writes
//! nothing and reads no write credential, unless `--apply` takes the steps
//! (ADR-0087). Like `herald queue`'s rows, a private or internal
//! repository's entries are withheld and counted unless the output is said
//! private.

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::Hosting;
use herald_compose::told::queue_lines;
use herald_compose::told::why;
use herald_config::{Config, Uses};
use herald_core::Printed;
use herald_native::TokioTimer;

use super::output::say;
use super::stores::composed;
use super::{Connect, DONE};

/// What prints what `herald merge-queue` withholds.
const PRIVATE_HINT: &str = "--private, or [cli] private, prints";

pub(crate) async fn merging(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    (asked, apply): ((Option<&str>, Printed), bool),
    out: &mut dyn Write,
) -> Result<u8, String> {
    let composed = composed(
        config,
        vars,
        connect,
        Hosting::Actions,
        Uses::Polls,
        "merge-queue",
    )
    .await?;
    let read = if apply {
        composed.merge_apply(asked, &TokioTimer).await
    } else {
        composed.merge_queues(asked).await
    }
    .map_err(|stop| why(&stop))?;
    if read.is_empty() {
        say(
            out,
            "no merge queue is configured; add a [merge_queues.<name>] table",
        );
    }
    for line in read.iter().flat_map(|read| queue_lines(read, PRIVATE_HINT)) {
        say(out, &line);
    }
    Ok(DONE)
}
