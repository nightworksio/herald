//! What herald's log writes: each line at its level, as words or as a
//! JSON object, every secret herald resolved or sent scrubbed from it, on
//! every host (ADR-0089).

use std::sync::Arc;

use herald_config::{Level, LogFormat};
use herald_ports::{Clock, Scrubber};

/// The variable naming the least grave line the log writes, in place of
/// the config's `[log] level`.
pub const LOG_LEVEL: &str = "HERALD_LOG_LEVEL";
/// The variable naming how the log writes each line, in place of the
/// config's `[log] format`.
pub const LOG_FORMAT: &str = "HERALD_LOG_FORMAT";

/// A line of herald's log, at its level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Told {
    /// How grave it is.
    pub level: Level,
    /// What it says.
    pub text: String,
}

impl Told {
    /// What herald saw as it went: no line of it carries what a private
    /// repository holds.
    #[must_use]
    pub fn debug(text: impl Into<String>) -> Self {
        Self::at(Level::Debug, text)
    }

    /// What herald did.
    #[must_use]
    pub fn info(text: impl Into<String>) -> Self {
        Self::at(Level::Info, text)
    }

    /// What herald left undone, passed over or refused.
    #[must_use]
    pub fn warn(text: impl Into<String>) -> Self {
        Self::at(Level::Warn, text)
    }

    /// What stopped or failed.
    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self::at(Level::Error, text)
    }

    /// `text`, at `level`.
    #[must_use]
    pub fn at(level: Level, text: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
        }
    }

    /// The same line at its level, its words as `reworded` makes them.
    #[must_use]
    pub fn reworded(self, reworded: impl FnOnce(String) -> String) -> Self {
        Self::at(self.level, reworded(self.text))
    }
}

impl PartialEq<&str> for Told {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

impl PartialEq<String> for Told {
    fn eq(&self, other: &String) -> bool {
        self.text == *other
    }
}

/// How herald's log writes: the least grave line it writes, how it writes
/// each, what scrubs each, and the clock a JSON line takes its time from.
#[derive(Clone)]
pub struct Log {
    level: Level,
    format: LogFormat,
    scrubber: Scrubber,
    clock: Arc<dyn Clock + Send>,
}

impl std::fmt::Debug for Log {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Log")
            .field("level", &self.level)
            .field("format", &self.format)
            .field("scrubber", &self.scrubber)
            .finish_non_exhaustive()
    }
}

impl Log {
    /// The log `config` sets, [`LOG_LEVEL`] and [`LOG_FORMAT`] as `var`
    /// reads them taking their places, each line scrubbed by `scrubber` and
    /// a JSON line timed by `clock`.
    ///
    /// # Errors
    ///
    /// Where a variable names no level or format, saying which it takes.
    pub fn of<'v>(
        config: &herald_config::Log,
        var: impl Fn(&str) -> Option<&'v str>,
        scrubber: Scrubber,
        clock: impl Clock + Send + 'static,
    ) -> Result<Self, String> {
        let named = |variable: &str, name: &str, takes: String| {
            format!("{variable} is \"{name}\"; {takes}")
        };
        let level = var(LOG_LEVEL)
            .map(|name| level(name).map_err(|takes| named(LOG_LEVEL, name, takes)))
            .transpose()?;
        let format = var(LOG_FORMAT)
            .map(|name| format(name).map_err(|takes| named(LOG_FORMAT, name, takes)))
            .transpose()?;
        Ok(Self {
            level: level.unwrap_or(config.level),
            format: format.unwrap_or(config.format),
            scrubber,
            clock: Arc::new(clock),
        })
    }

    /// The log a host writes before a config is read: the level and
    /// format [`LOG_LEVEL`] and [`LOG_FORMAT`] name, where `var` reads one
    /// each names, and else the defaults, a JSON line timed by `clock`.
    #[must_use]
    pub fn unconfigured<'v>(
        var: impl Fn(&str) -> Option<&'v str>,
        clock: impl Clock + Send + 'static,
    ) -> Self {
        let config = herald_config::Log::default();
        Self {
            level: var(LOG_LEVEL)
                .and_then(Level::named)
                .unwrap_or(config.level),
            format: var(LOG_FORMAT)
                .and_then(LogFormat::named)
                .unwrap_or(config.format),
            scrubber: config.scrubber(),
            clock: Arc::new(clock),
        }
    }

    /// The same log, `level` and `format` taking their places where given.
    #[must_use]
    pub fn flagged(self, level: Option<Level>, format: Option<LogFormat>) -> Self {
        Self {
            level: level.unwrap_or(self.level),
            format: format.unwrap_or(self.format),
            ..self
        }
    }

    /// What scrubs each line.
    #[must_use]
    pub const fn scrubber(&self) -> &Scrubber {
        &self.scrubber
    }

    /// How it writes each line.
    #[must_use]
    pub const fn format(&self) -> LogFormat {
        self.format
    }

    /// `told` as the log writes it, scrubbed; nothing where it is less
    /// grave than the least grave line the log writes. A JSON line holds
    /// its `time`, in RFC 3339 and UTC by the log's clock, its `level` and
    /// its `message`.
    #[must_use]
    pub fn written(&self, told: &Told) -> Option<String> {
        (told.level >= self.level).then(|| {
            let text = self.scrubber.scrubbed(&told.text);
            match self.format {
                LogFormat::Text => text,
                LogFormat::Json => {
                    let field = |value: String| serde_json::Value::String(value).to_string();
                    format!(
                        "{{\"time\":{},\"level\":{},\"message\":{}}}",
                        field(self.clock.now().to_string()),
                        field(told.level.name().to_owned()),
                        field(text)
                    )
                }
            }
        })
    }

    /// Each of `told` the log writes, as it writes it.
    #[must_use]
    pub fn lines(&self, told: impl IntoIterator<Item = Told>) -> Vec<String> {
        self.crowded()
            .into_iter()
            .chain(told)
            .filter_map(|told| self.written(&told))
            .collect()
    }

    /// What the log says once, where its scrubber holds more values than
    /// `[log] scrub_warn_past`: each is still scrubbed.
    #[must_use]
    pub fn crowded(&self) -> Option<Told> {
        self.scrubber.crowded().map(|held| {
            Told::warn(format!(
                "herald holds {held} secret values to scrub, more than [log] scrub_warn_past, {}; each is still scrubbed until it can no longer be valid, and scrub_warn_past may be raised where a run sends this many",
                self.scrubber.many()
            ))
        })
    }
}

/// The level `name` names, or which names one takes.
///
/// # Errors
///
/// Where `name` names no level.
pub fn level(name: &str) -> Result<Level, String> {
    Level::named(name).ok_or_else(|| takes(Level::ALL.map(Level::name)))
}

/// The format `name` names, or which names one takes.
///
/// # Errors
///
/// Where `name` names no format.
pub fn format(name: &str) -> Result<LogFormat, String> {
    LogFormat::named(name).ok_or_else(|| takes(LogFormat::ALL.map(LogFormat::name)))
}

fn takes<const N: usize>(names: [&str; N]) -> String {
    format!("it takes {}", names.join(", "))
}

#[cfg(test)]
mod tests;
