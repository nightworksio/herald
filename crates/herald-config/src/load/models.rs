//! `[models.*]` and `[tasks.*]` (ADR-0057): each model read against the
//! kinds and presets herald knows, and each task against the models it
//! names.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::ops::Range;

use herald_core::{Address, Options};
use herald_models::{MOST_ANSWER, Speaks, TYPESAFE_BASE, named};

use super::{Loader, Value, list};
use crate::models::{MOST_ASKED, ModelConfig, ModelFile, Task, TaskFile};

/// The kind of a model reached through TypeSafe's API.
const TYPESAFE: &str = "typesafe";

/// The kind of a model reached through any OpenAI-compatible server.
const OPENAI_COMPATIBLE: &str = "openai-compatible";

/// The hosts that are this machine.
const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];

/// Whether `base` is reached on this machine.
fn loopback(base: &Address) -> bool {
    LOOPBACK.contains(&base.host())
}

impl Loader<'_> {
    /// How the model of `kind`, at `path`, is spoken to, its base where the
    /// kind names one, and the header its key goes in by default.
    fn spoken(
        &mut self,
        file: &ModelFile,
        path: &str,
    ) -> Option<(Speaks, &'static str, Option<&'static str>)> {
        let kind = file.kind.get_ref().as_str();
        match (kind, named(kind)) {
            (TYPESAFE, _) => Some((Speaks::TypeSafe, TYPESAFE_BASE, None)),
            (OPENAI_COMPATIBLE, _) => Some((Speaks::OpenAi, "", None)),
            (_, Some(preset)) => Some((Speaks::OpenAi, preset.base, preset.header)),
            (_, None) => {
                let kinds: Vec<&str> = [TYPESAFE, OPENAI_COMPATIBLE]
                    .into_iter()
                    .chain(herald_models::PRESETS.iter().map(|preset| preset.name))
                    .collect();
                self.error(
                    &file.kind.span(),
                    &format!("{path}.kind"),
                    format!(
                        "there is no model kind \"{kind}\"; the kinds are {}",
                        list(&kinds)
                    ),
                );
                None
            }
        }
    }

    /// The base the model `file` at `path`, its table at `span`, is reached
    /// at: its own or its kind's, HTTPS, or plain HTTP on this machine or
    /// where it says `insecure = true`.
    fn model_base(
        &mut self,
        file: &ModelFile,
        (path, span): (&str, &Range<usize>),
        shipped: &str,
    ) -> Option<String> {
        let (written, at) = file.base.as_ref().map_or((shipped, span.clone()), |base| {
            (base.get_ref().as_str(), base.span())
        });
        let key = format!("{path}.base");
        let Some(base) = Address::parse(written) else {
            self.error(
                &at,
                &key,
                "a model is reached at a base URL, such as base = \"http://127.0.0.1:11434/v1\"",
            );
            return None;
        };
        let reachable = match base.scheme() {
            "https" => true,
            "http" => file.insecure || loopback(&base),
            _ => false,
        };
        if !reachable {
            self.error(
                &at,
                &key,
                format!(
                    "\"{written}\" is reached over HTTPS, or over plain HTTP on this machine; write insecure = true to reach it over plain HTTP elsewhere"
                ),
            );
            return None;
        }
        Some(written.trim_end_matches('/').to_owned())
    }

    /// Each model of `[models]`.
    pub(super) fn models(&mut self, value: Option<Value<'_>>) -> BTreeMap<String, ModelConfig> {
        let mut models = BTreeMap::new();
        for (name, value) in self.named(value, "models") {
            let path = format!("models.{name}");
            let span = value.span();
            let Some(file) = self.typed::<ModelFile>(value, &path) else {
                continue;
            };
            let Some((speaks, shipped, header)) = self.spoken(&file, &path) else {
                continue;
            };
            let Some(base) = self.model_base(&file, (&path, &span), shipped) else {
                continue;
            };
            models.insert(
                name,
                ModelConfig {
                    kind: file.kind.into_inner(),
                    speaks,
                    base,
                    model: file.model.into_inner(),
                    key: file.key,
                    header: file.header.or_else(|| header.map(str::to_owned)),
                    private: file.private,
                    insecure: file.insecure,
                    most: file.most.map_or(MOST_ANSWER, NonZeroUsize::get),
                },
            );
        }
        models
    }

    /// Each task of `[tasks]`, asking only models of `models`.
    pub(super) fn tasks(
        &mut self,
        value: Option<Value<'_>>,
        models: &BTreeMap<String, ModelConfig>,
    ) -> BTreeMap<String, Task> {
        let mut tasks = BTreeMap::new();
        for (name, value) in self.named(value, "tasks") {
            let path = format!("tasks.{name}");
            let Some(file) = self.typed::<TaskFile>(value, &path) else {
                continue;
            };
            let mut sound = !file.models.get_ref().is_empty();
            if !sound {
                self.error(
                    &file.models.span(),
                    &format!("{path}.models"),
                    "a task asks one model or more, in turn, such as models = [\"jev\", \"local\"]",
                );
            }
            for model in file.models.get_ref() {
                if !models.contains_key(model) {
                    let names: Vec<&str> = models.keys().map(String::as_str).collect();
                    self.error(
                        &file.models.span(),
                        &format!("{path}.models"),
                        format!(
                            "no model is named \"{model}\"; name it under [models.{model}]{}",
                            if names.is_empty() {
                                String::new()
                            } else {
                                format!(", or one of {}", list(&names))
                            }
                        ),
                    );
                    sound = false;
                }
            }
            let options = Options::new(file.options.get_ref().clone())
                .inspect_err(|reason| {
                    self.error(
                        &file.options.span(),
                        &format!("{path}.options"),
                        reason.clone(),
                    );
                })
                .ok();
            let least = file.least.as_ref().map(|least| *least.get_ref());
            if let Some(written) = file
                .least
                .as_ref()
                .filter(|least| !(0.0..=1.0).contains(least.get_ref()))
            {
                self.error(
                    &written.span(),
                    &format!("{path}.least"),
                    "an answer's least probability is from 0 to 1, such as least = 0.8",
                );
                sound = false;
            }
            if let (true, Some(options)) = (sound, options) {
                tasks.insert(
                    name,
                    Task {
                        models: file.models.into_inner(),
                        instructions: file.instructions,
                        options,
                        least,
                        most: file.most.map_or(MOST_ASKED, NonZeroUsize::get),
                    },
                );
            }
        }
        tasks
    }
}
