//! What the CLI prints: every secret the run may resolve masked on Actions
//! and scrubbed from each line, warnings as workflow annotations, and every
//! line kept to one (ADR-0007, ADR-0002); and what each command did, in
//! words.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;
use std::ops::RangeInclusive;

use herald_compose::{Log, Told};
use herald_config::{Config, LogFormat};
use herald_flows::{Gap, ROUND_SECRET};
use herald_ports::{Scrubber, Secret};
use herald_store_actions_cache::RUNTIME_TOKEN;

use super::doctoring::gapped;
use super::lambda::CREDENTIALS;
use super::{GITHUB_TOKEN, JOB_TOKEN};

/// Every secret the run may resolve, by its name, and each line of a value
/// that spans lines on its own as well.
pub(crate) fn hidden(config: &Config, vars: &BTreeMap<String, String>) -> Vec<Secret> {
    let mut names = config.secrets();
    names.extend([GITHUB_TOKEN, RUNTIME_TOKEN, JOB_TOKEN, ROUND_SECRET]);
    names.extend(CREDENTIALS);
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

/// A writer that writes each line with every secret its scrubber knows
/// shown as `«secret:NAME»`, whatever handed the line to it: an answer a
/// server echoed back, and a token minted as the run went, included
/// (ADR-0007, ADR-0089).
pub(crate) struct Scrubbing<'a> {
    inner: &'a mut dyn Write,
    scrubber: Scrubber,
    line: Vec<u8>,
}

impl<'a> Scrubbing<'a> {
    pub(crate) fn new(inner: &'a mut dyn Write, scrubber: Scrubber) -> Self {
        Self {
            inner,
            scrubber,
            line: Vec::new(),
        }
    }

    fn emit(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let scrubbed = self.scrubber.scrubbed(&String::from_utf8_lossy(bytes));
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

/// What a run that tells what it did writes to, and the log it writes as
/// (ADR-0089).
pub(crate) struct Logged<'a> {
    out: &'a mut dyn Write,
    log: &'a Log,
}

impl<'a> Logged<'a> {
    pub(crate) fn new(out: &'a mut dyn Write, log: &'a Log) -> Self {
        Self { out, log }
    }

    /// `told`, as the log writes it.
    pub(crate) fn told(&mut self, told: &Told) {
        if let Some(line) = self.log.written(told) {
            match self.log.format() {
                LogFormat::Text => say(self.out, &line),
                LogFormat::Json => say_json(self.out, &line),
            }
        }
    }

    /// What herald did, as the log writes it.
    pub(crate) fn info(&mut self, text: impl Into<String>) {
        self.told(&Told::info(text));
    }

    /// Each of `told`, as the log writes it.
    pub(crate) fn all(&mut self, told: impl IntoIterator<Item = Told>) {
        for told in told {
            self.told(&told);
        }
    }

    /// What a dry run prints its requests to, as they are.
    pub(crate) fn out(&mut self) -> &mut dyn Write {
        self.out
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

/// Each gap as a warning, annotated on GitHub Actions and logged
/// elsewhere.
pub(crate) fn warned(gaps: &[Gap], vars: &BTreeMap<String, String>, out: &mut Logged<'_>) {
    let actions = on_actions(vars);
    for gap in gaps {
        let line = gapped(gap);
        if actions {
            say(
                out.out(),
                &format!("::warning title=herald doctor::{}", escaped(&line)),
            );
        } else {
            out.told(&Told::warn(format!("warning: {line}")));
        }
    }
}

/// The Unicode format characters (general category Cf, Unicode 16) a line
/// drops: each can make a line read otherwise than it is, as a bidi
/// override, an isolate, a zero-width character, a tag or a soft hyphen
/// does.
const FORMATTING: [RangeInclusive<char>; 20] = [
    '\u{ad}'..='\u{ad}',
    '\u{600}'..='\u{605}',
    '\u{61c}'..='\u{61c}',
    '\u{6dd}'..='\u{6dd}',
    '\u{70f}'..='\u{70f}',
    '\u{890}'..='\u{891}',
    '\u{8e2}'..='\u{8e2}',
    '\u{180e}'..='\u{180e}',
    '\u{200b}'..='\u{200f}',
    '\u{202a}'..='\u{202e}',
    '\u{2060}'..='\u{2064}',
    '\u{2066}'..='\u{206f}',
    '\u{feff}'..='\u{feff}',
    '\u{fff9}'..='\u{fffb}',
    '\u{110bd}'..='\u{110bd}',
    '\u{110cd}'..='\u{110cd}',
    '\u{13430}'..='\u{1343f}',
    '\u{1bca0}'..='\u{1bca3}',
    '\u{1d173}'..='\u{1d17a}',
    '\u{e0000}'..='\u{e007f}',
];

/// The line and paragraph breaks a line said holds as a space.
const BREAKS: [char; 4] = ['\r', '\n', '\u{2028}', '\u{2029}'];

/// Whether `char` is kept off a line as it is: a control character, a
/// line or paragraph break, or one of [`FORMATTING`].
fn unshown(char: char) -> bool {
    char.is_control()
        || BREAKS.contains(&char)
        || FORMATTING.iter().any(|range| range.contains(&char))
}

/// Writes `line` as one line, every control and format character it holds
/// from a name or an answer herald was handed dropped: a line break would
/// start a line of its own, which GitHub Actions could read as a workflow
/// command, an escape could make the terminal do what the forge says, such
/// as setting the clipboard, and a bidi override could make a name read as
/// another. A line or paragraph break becomes a space.
pub(crate) fn say(to: &mut dyn Write, line: &str) {
    let shown: String = line
        .chars()
        .map(|char| if BREAKS.contains(&char) { ' ' } else { char })
        .filter(|char| !unshown(*char))
        .collect();
    let _ = writeln!(to, "{shown}");
}

/// Writes `json`, serialised JSON, as one line: the line breaks between its
/// values become spaces, and every character [`say`] drops, which can stand
/// only within a string there, is escaped as `\uXXXX` (a character above
/// U+FFFF as its UTF-16 surrogate pair), so the JSON reads back unchanged.
pub(crate) fn say_json(to: &mut dyn Write, json: &str) {
    let mut shown = String::with_capacity(json.len());
    for char in json.chars() {
        if matches!(char, '\r' | '\n') {
            shown.push(' ');
        } else if unshown(char) {
            let mut units = [0; 2];
            for unit in char.encode_utf16(&mut units) {
                let _ = write!(shown, "\\u{unit:04x}");
            }
        } else {
            shown.push(char);
        }
    }
    let _ = writeln!(to, "{shown}");
}

/// `message` as one workflow command's message: a line break would start
/// another command.
pub(crate) fn escaped(message: &str) -> String {
    message
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}
