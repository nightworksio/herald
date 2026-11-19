//! `[models.*]` and `[tasks.*]` (ADR-0057): each model read against the
//! kinds and presets herald knows, and each task against the models it
//! names.

use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroUsize};
use std::ops::Range;

use herald_aws::{ACCESS_KEY_ID, SECRET_ACCESS_KEY, SESSION_TOKEN, endpoint};
use herald_core::{Address, Options, SecretRef};
use herald_models::{
    MOST_ANSWER, MOST_TOKENS, Speaks, TYPESAFE_BASE, WORKERS_AI, anthropic, bedrock, gemini, named,
};

use super::{Loader, Value, list};
use crate::models::{BedrockConfig, MOST_ASKED, ModelConfig, ModelFile, Task, TaskFile};

/// The kind of a model reached through TypeSafe's API.
const TYPESAFE: &str = "typesafe";

/// The kind of a model reached through any OpenAI-compatible server.
const OPENAI_COMPATIBLE: &str = "openai-compatible";

/// The kind of a model reached through Anthropic's Messages API.
const ANTHROPIC: &str = "anthropic";

/// The kind of a model reached through Gemini's API.
const GEMINI: &str = "gemini";

/// The kind of a model reached through Amazon Bedrock's Converse API.
const BEDROCK: &str = "bedrock";

/// The kind of a model Spin runs, through its `llm` interface.
const SPIN: &str = "spin";

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
            (ANTHROPIC, _) => Some((Speaks::Anthropic, anthropic::BASE, Some(anthropic::HEADER))),
            (GEMINI, _) => Some((Speaks::Gemini, gemini::BASE, Some(gemini::HEADER))),
            (BEDROCK, _) => Some((Speaks::Bedrock, "", None)),
            (SPIN, _) => Some((Speaks::Spin, "", None)),
            (WORKERS_AI, _) if file.binding.is_some() => Some((Speaks::WorkersAi, "", None)),
            (_, Some(preset)) => Some((Speaks::OpenAi, preset.base, preset.header)),
            (_, None) => {
                let kinds: Vec<&str> = [
                    TYPESAFE,
                    OPENAI_COMPATIBLE,
                    ANTHROPIC,
                    GEMINI,
                    BEDROCK,
                    SPIN,
                ]
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

    /// How the model `file` at `path`, its table at `span`, is reached
    /// through Amazon Bedrock where it `speaks` Bedrock's API: its region,
    /// the secrets its requests are signed with, and its region's base;
    /// none where it speaks another, which names none of them.
    fn bedrock(
        &mut self,
        file: &ModelFile,
        (path, span): (&str, &Range<usize>),
        speaks: Speaks,
    ) -> Result<Option<(BedrockConfig, String)>, ()> {
        if speaks != Speaks::Bedrock {
            let named = [
                ("region", file.region.is_some()),
                ("access_key_id", file.access_key_id.is_some()),
                ("secret_access_key", file.secret_access_key.is_some()),
                ("session_token", file.session_token.is_some()),
            ];
            let mut sound = true;
            for (key, _) in named.into_iter().filter(|(_, written)| *written) {
                self.error(
                    span,
                    &format!("{path}.{key}"),
                    format!("only a \"{BEDROCK}\" model takes {key}"),
                );
                sound = false;
            }
            return if sound { Ok(None) } else { Err(()) };
        }
        let key = format!("{path}.region");
        let Some(region) = file.region.as_deref() else {
            self.error(
                span,
                &key,
                format!(
                    "a \"{BEDROCK}\" model names the region it is reached in, such as region = \"eu-west-1\""
                ),
            );
            return Err(());
        };
        if !bedrock::region(region) {
            self.error(
                span,
                &key,
                format!(
                    "\"{region}\" is no region; write it as AWS names it, such as region = \"eu-west-1\""
                ),
            );
            return Err(());
        }
        let secret = |named: Option<&SecretRef>, otherwise: &str| {
            named.cloned().unwrap_or_else(|| SecretRef {
                secret: otherwise.to_owned(),
            })
        };
        let config = BedrockConfig {
            region: region.to_owned(),
            access_key_id: secret(file.access_key_id.as_ref(), ACCESS_KEY_ID),
            secret_access_key: secret(file.secret_access_key.as_ref(), SECRET_ACCESS_KEY),
            session_token: secret(file.session_token.as_ref(), SESSION_TOKEN),
        };
        Ok(Some((config, endpoint(bedrock::ENDPOINT, region))))
    }

    /// Whether the model `file` at `path`, its table at `span`, names only
    /// what its kind takes: a binding where its host runs it through
    /// Workers AI, and neither a base nor a key where its host runs it.
    fn bound(
        &mut self,
        file: &ModelFile,
        (path, span): (&str, &Range<usize>),
        speaks: Speaks,
    ) -> bool {
        let runs = speaks.bound();
        let mut sound = true;
        if file.binding.is_some() && speaks != Speaks::WorkersAi {
            self.error(
                span,
                &format!("{path}.binding"),
                format!("only a \"{WORKERS_AI}\" model takes binding"),
            );
            sound = false;
        }
        let unreached = [("base", file.base.is_some()), ("key", file.key.is_some())];
        for (key, _) in unreached
            .into_iter()
            .filter(|(_, written)| runs && *written)
        {
            self.error(
                span,
                &format!("{path}.{key}"),
                format!("a model its host runs through a binding takes no {key}"),
            );
            sound = false;
        }
        sound
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
            let reached = self.bedrock(&file, (&path, &span), speaks);
            let (Ok(reached), true) = (reached, self.bound(&file, (&path, &span), speaks)) else {
                continue;
            };
            let shipped = reached.as_ref().map_or(shipped, |(_, base)| base.as_str());
            let base = if speaks.bound() {
                Some(String::new())
            } else {
                self.model_base(&file, (&path, &span), shipped)
            };
            let Some(base) = base else {
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
                    tokens: file.tokens.map_or(MOST_TOKENS, NonZeroU32::get),
                    bedrock: reached.map(|(config, _)| config),
                    binding: file.binding,
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
