//! What the CLI prints: every secret the run may resolve masked on Actions
//! and scrubbed from each line, warnings as workflow annotations, and every
//! line kept to one (ADR-0007, ADR-0002); and what each command did, in
//! words.

use std::collections::BTreeMap;
use std::io::Write;

use herald_config::Config;
use herald_core::{Chore, Schedule, Step};
use herald_flows::{Announced, Chored, Gap, Kept, Outcome, Remembered, Stop, Sweep, Swept};
use herald_ports::Secret;

use super::{GITHUB_TOKEN, JOB_TOKEN, RUNTIME_TOKEN, SCRUBBED, gapped};

/// Every secret the run may resolve, by its name, and each line of a value
/// that spans lines on its own as well.
pub(crate) fn hidden(config: &Config, vars: &BTreeMap<String, String>) -> Vec<Secret> {
    let mut names = config.secrets();
    names.extend([GITHUB_TOKEN, RUNTIME_TOKEN, JOB_TOKEN]);
    let mut hidden = Vec::new();
    for (name, value) in names
        .iter()
        .filter_map(|name| vars.get(*name).map(|value| (*name, value)))
    {
        let lines = value.lines().filter(|line| *line != value.as_str());
        for part in std::iter::once(value.as_str()).chain(lines) {
            if !part.trim().is_empty() {
                hidden.push(Secret::new(name, part));
            }
        }
    }
    hidden.sort_by_key(|secret| std::cmp::Reverse(secret.expose().len()));
    hidden
}

/// A writer that writes each line with every secret the run may resolve
/// shown as `«secret:NAME»`, whatever handed the line to it: an answer a
/// server echoed back included (ADR-0007).
pub(crate) struct Scrubbing<'a> {
    inner: &'a mut dyn Write,
    hidden: Vec<&'a Secret>,
    line: Vec<u8>,
}

impl<'a> Scrubbing<'a> {
    pub(crate) fn new(inner: &'a mut dyn Write, hidden: &'a [Secret]) -> Self {
        Self {
            inner,
            hidden: hidden
                .iter()
                .filter(|secret| secret.expose().chars().count() >= SCRUBBED)
                .collect(),
            line: Vec::new(),
        }
    }

    /// `bytes` written with each secret shown by its name, the longest
    /// value matched first, in one pass.
    fn emit(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let text = String::from_utf8_lossy(bytes);
        let mut scrubbed = String::with_capacity(text.len());
        let mut rest = text.as_ref();
        while let Some(first) = rest.chars().next() {
            let found = self
                .hidden
                .iter()
                .find(|secret| rest.starts_with(secret.expose()));
            if let Some(secret) = found {
                scrubbed.push_str(&secret.redacted());
                rest = rest.get(secret.expose().len()..).unwrap_or_default();
            } else {
                scrubbed.push(first);
                rest = rest.get(first.len_utf8()..).unwrap_or_default();
            }
        }
        self.inner.write_all(scrubbed.as_bytes())
    }
}

impl Write for Scrubbing<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.line.extend_from_slice(bytes);
        let held = std::mem::take(&mut self.line);
        for piece in held.split_inclusive(|byte| *byte == b'\n') {
            if piece.ends_with(b"\n") {
                self.emit(piece)?;
            } else {
                self.line = piece.to_vec();
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let line = std::mem::take(&mut self.line);
        if !line.is_empty() {
            self.emit(&line)?;
        }
        self.inner.flush()
    }
}

/// Whether the run is on GitHub Actions.
pub(crate) fn on_actions(vars: &BTreeMap<String, String>) -> bool {
    vars.get("GITHUB_ACTIONS")
        .is_some_and(|value| value == "true")
}

/// On GitHub Actions, every secret the run may resolve masked before
/// anything else is printed, each of its lines on its own as well
/// (ADR-0007).
pub(crate) fn masked(hidden: &[Secret], vars: &BTreeMap<String, String>, out: &mut dyn Write) {
    if !on_actions(vars) {
        return;
    }
    for secret in hidden {
        say(out, &format!("::add-mask::{}", escaped(secret.expose())));
    }
}

/// Each gap as a warning, annotated on GitHub Actions.
pub(crate) fn warned(gaps: &[Gap], vars: &BTreeMap<String, String>, out: &mut dyn Write) {
    let actions = on_actions(vars);
    for gap in gaps {
        let line = gapped(gap);
        if actions {
            say(
                out,
                &format!("::warning title=herald doctor::{}", escaped(&line)),
            );
        } else {
            say(out, &format!("warning: {line}"));
        }
    }
}

/// Writes `line` as one line: a line break inside it, from a name or an
/// answer herald was handed, would otherwise start a line of its own, which
/// GitHub Actions could read as a workflow command.
pub(crate) fn say(to: &mut dyn Write, line: &str) {
    let _ = writeln!(to, "{}", line.replace(['\r', '\n'], " "));
}

/// `message` as one workflow command's message: a line break would start
/// another command.
pub(crate) fn escaped(message: &str) -> String {
    message
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

pub(crate) fn polled(swept: &Swept) -> Vec<String> {
    let source = swept.source.as_str();
    match &swept.outcome {
        Sweep::Unchanged => vec![format!("{source}: unchanged since the last poll")],
        Sweep::Stopped(stop) => vec![format!(
            "{source}: stopped: {}. The next poll asks again.",
            why(stop)
        )],
        Sweep::Polled {
            first,
            found,
            announced,
            cursor,
        } => {
            let new = announced.len();
            let head = if *first {
                format!("{source}: first poll found {found}; {new} announced, as backfill asks")
            } else {
                format!("{source}: {found} found, {new} new")
            };
            let tail = match cursor {
                Remembered::Advanced => String::new(),
                Remembered::Kept => {
                    "; a route stopped, so the next poll offers them again".to_owned()
                }
                Remembered::Failed(stop) => format!(
                    "; the cursor stays where it was: {}. The next poll offers them again",
                    why(stop)
                ),
            };
            let mut lines = vec![format!("{head}{tail}")];
            for fresh in announced {
                let event = fresh.event.version().unwrap_or(fresh.event.title());
                lines.extend(
                    fresh
                        .announced
                        .iter()
                        .map(|announced| format!("{source} {event}: {}", report(announced))),
                );
            }
            lines
        }
    }
}

pub(crate) fn chored(kept: &Kept) -> String {
    let Schedule {
        route,
        thread,
        chore,
        ..
    } = &kept.schedule;
    let route = route.as_str();
    let done = match chore {
        Chore::Archive => "archived",
        Chore::Lock => "locked",
    };
    match &kept.outcome {
        Chored::Done => format!("{route}: thread {thread} {done}"),
        Chored::Dropped(reason) => {
            format!("{route}: thread {thread} not {done}, and not tried again: {reason}")
        }
        Chored::Later(stop) => format!(
            "{route}: thread {thread} not {done} yet: {}. The next run tries again.",
            why(stop)
        ),
    }
}

pub(crate) fn why(reason: &Stop) -> String {
    match reason {
        Stop::RetryAfter(after) => format!("the destination asked to wait {after:#}"),
        Stop::Refused(reason)
        | Stop::Unreachable(reason)
        | Stop::Store(reason)
        | Stop::Template(reason)
        | Stop::Marker(reason)
        | Stop::Source(reason) => reason.clone(),
    }
}

fn step(step: &Step) -> String {
    match step {
        Step::Post => "post".to_owned(),
        Step::Crosspost => "crosspost".to_owned(),
        Step::OpenThread => "thread".to_owned(),
        Step::Chunk(index) => format!("chunk {index}"),
        Step::Pin => "pin".to_owned(),
        Step::Unpin => "unpin".to_owned(),
        Step::Deliver(inbox) => format!("delivery to {inbox}"),
    }
}

pub(crate) fn steps(steps: &[Step]) -> String {
    steps.iter().map(step).collect::<Vec<_>>().join(", ")
}

pub(crate) fn report(announced: &Announced) -> String {
    let route = announced.route.as_str();
    match &announced.outcome {
        Outcome::Sent { steps: sent } => format!("{route}: announced ({})", steps(sent)),
        Outcome::AlreadyAnnounced => format!("{route}: already announced; nothing sent"),
        Outcome::Held { by, until } => {
            format!("{route}: run {by} holds it until {until}; nothing sent by this run")
        }
        Outcome::Stopped {
            landed,
            remaining,
            reason,
        } => {
            format!(
                "{route}: stopped: {}. Landed: {}. The next run sends what remains: {}.",
                why(reason),
                if landed.is_empty() {
                    "nothing".to_owned()
                } else {
                    steps(landed)
                },
                if remaining.is_empty() {
                    "nothing".to_owned()
                } else {
                    steps(remaining)
                },
            )
        }
    }
}
