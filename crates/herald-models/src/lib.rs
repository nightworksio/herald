//! The decision models herald may ask a closed-set question (ADR-0057):
//! TypeSafe's Jev, every server that speaks OpenAI's chat completions,
//! from a local Ollama to a hosted provider, and Anthropic's, Gemini's and
//! Amazon Bedrock's own APIs, and the models Cloudflare Workers AI and
//! Spin run beside herald (ADR-0059). Each request carries the
//! state as data and the questions with the options the config wrote, and
//! each answer is read within the model's cap and checked against those
//! options by its caller.

use std::collections::BTreeMap;

use herald_core::{Answer, Question};
use herald_ports::{Binding, Inference, Method, Request, Response, Secret, SecretText};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub mod anthropic;
mod asked;
pub mod bedrock;
pub mod gemini;
mod openai;
mod spin;
mod typesafe;
mod workers;

/// The most bytes of a model's answer read where its config names no
/// other cap.
pub const MOST_ANSWER: usize = 65_536;

/// The most tokens a model that takes a cap answers within where its
/// config names no other.
pub const MOST_TOKENS: u32 = 4096;

/// How a model is spoken to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaks {
    /// TypeSafe's System One API, `POST <base>/systemone`.
    TypeSafe,
    /// OpenAI's chat completions, `POST <base>/chat/completions`, with a
    /// JSON schema the answer holds to.
    OpenAi,
    /// Anthropic's Messages API, `POST <base>/messages`, with a JSON
    /// schema the answer holds to.
    Anthropic,
    /// Gemini's `POST <base>/models/<model>:generateContent`, with a JSON
    /// schema the answer holds to.
    Gemini,
    /// Amazon Bedrock's `POST <base>/model/<model>/converse`, with a JSON
    /// schema the answer holds to.
    Bedrock,
    /// Cloudflare Workers AI through a Worker's binding, with a JSON schema
    /// the answer holds to.
    WorkersAi,
    /// Spin's `llm` interface, asked in one prompt.
    Spin,
}

impl Speaks {
    /// Whether its host runs it beside herald, reached through a binding.
    #[must_use]
    pub const fn bound(self) -> bool {
        matches!(self, Self::WorkersAi | Self::Spin)
    }
}

/// The header a model's key is sent in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyHeader {
    /// `Authorization: Bearer <key>`.
    Bearer,
    /// A header of this name holding the key alone, as Azure's `api-key`.
    Named(String),
}

/// A model herald may ask, as the config names it.
#[derive(Debug, Clone)]
pub struct Model {
    /// How it is spoken to.
    pub speaks: Speaks,
    /// The API's base, such as `https://api.typesafe.ai/v1`.
    pub base: String,
    /// The model's id, pinned, such as `jev-1.14.0`.
    pub model: String,
    /// Its key, where it takes one.
    pub key: Option<Secret>,
    /// The header the key is sent in.
    pub header: KeyHeader,
    /// The most bytes of its answer read.
    pub most: usize,
    /// The most tokens it answers within, where its API takes a cap.
    pub tokens: u32,
    /// What its requests are signed with, where it is reached through
    /// Amazon Bedrock with no API key.
    pub aws: Option<bedrock::Keys>,
    /// The Worker's binding it is reached through, where it speaks Workers
    /// AI's binding.
    pub binding: Option<String>,
}

/// Why a model gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unanswered {
    /// It answered with this status.
    Refused(u16),
    /// Its answer was larger than the most read of it, in bytes.
    TooLarge(usize),
    /// Its answer was not JSON of the shape asked for.
    Unreadable(String),
}

/// `bytes` read as JSON of the shape `T`, where they are no more than
/// `most`.
fn read<T: DeserializeOwned>(most: usize, bytes: &[u8]) -> Result<T, Unanswered> {
    if bytes.len() > most {
        return Err(Unanswered::TooLarge(most));
    }
    serde_json::from_slice(bytes).map_err(|error| Unanswered::Unreadable(error.to_string()))
}

/// How a model is asked: a request sent to its API, or an inference its
/// host runs through a binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// The request sent.
    Sent(Request),
    /// What the host's model is asked.
    Bound(Inference),
}

impl Model {
    /// The binding its host reaches it through, where its host runs it.
    #[must_use]
    pub fn binding(&self) -> Option<Binding<'_>> {
        match self.speaks {
            Speaks::WorkersAi => self.binding.as_deref().map(Binding::WorkersAi),
            Speaks::Spin => Some(Binding::SpinLlm),
            Speaks::TypeSafe
            | Speaks::OpenAi
            | Speaks::Anthropic
            | Speaks::Gemini
            | Speaks::Bedrock => None,
        }
    }

    /// What its requests are signed with, where it is reached through
    /// Amazon Bedrock with no API key: its caller signs each with AWS
    /// Signature Version 4, which names its own content type.
    #[must_use]
    pub fn signed_by(&self) -> Option<&bedrock::Keys> {
        self.aws
            .as_ref()
            .filter(|_| self.speaks == Speaks::Bedrock && self.key.is_none())
    }

    /// How `questions` of `state` are asked, the state sent as data apart
    /// from the instructions, never as text an instruction may be written
    /// into.
    #[must_use]
    pub fn asked(&self, state: &str, questions: &[Question<'_>]) -> Asked {
        let (path, body) = match self.speaks {
            Speaks::TypeSafe => (
                typesafe::PATH.to_owned(),
                typesafe::body(&self.model, state, questions),
            ),
            Speaks::OpenAi => (
                openai::PATH.to_owned(),
                openai::body(&self.model, state, questions),
            ),
            Speaks::Anthropic => (
                anthropic::PATH.to_owned(),
                anthropic::body(&self.model, self.tokens, state, questions),
            ),
            Speaks::Gemini => (
                gemini::path(&self.model),
                gemini::body(self.tokens, state, questions),
            ),
            Speaks::Bedrock => (
                bedrock::path(&self.model),
                bedrock::body(self.tokens, state, questions),
            ),
            Speaks::WorkersAi => {
                return self.inferred(workers::inputs(self.tokens, state, questions));
            }
            Speaks::Spin => return self.inferred(spin::prompt(state, questions)),
        };
        Asked::Sent(self.request(&path, &body))
    }

    /// What its host's model is given as `inputs`.
    fn inferred(&self, inputs: Vec<u8>) -> Asked {
        Asked::Bound(Inference {
            model: self.model.clone(),
            inputs,
            tokens: self.tokens,
            most: self.most,
        })
    }

    /// The request of `body` to `path` below its base, left for its caller
    /// to sign where it is signed.
    fn request(&self, path: &str, body: &Value) -> Request {
        let base = self.base.trim_end_matches('/');
        let request = Request::new(Method::Post, SecretText::plain(format!("{base}{path}")))
            .body(body.to_string())
            .at_most(self.most);
        if self.signed_by().is_some() {
            return request;
        }
        let request = request.header("Content-Type", SecretText::plain(herald_core::media::JSON));
        let request = if self.speaks == Speaks::Anthropic {
            request.header(
                anthropic::VERSION_HEADER,
                SecretText::plain(anthropic::VERSION),
            )
        } else {
            request
        };
        match (&self.key, &self.header) {
            (None, _) => request,
            (Some(key), KeyHeader::Bearer) => request.header(
                "Authorization",
                SecretText::plain("Bearer ").then_secret(key.clone()),
            ),
            (Some(key), KeyHeader::Named(name)) => {
                request.header(name.clone(), SecretText::secret(key.clone()))
            }
        }
    }

    /// The answer `response` holds to each of `questions`, by question id,
    /// read within the model's cap; a question it did not answer is left
    /// out.
    ///
    /// # Errors
    ///
    /// Where it answered with a status other than success, or its answer is
    /// too large or not the shape asked for.
    pub fn answers(
        &self,
        response: &Response,
        questions: &[Question<'_>],
    ) -> Result<BTreeMap<String, Answer>, Unanswered> {
        if !(200..300).contains(&response.status) {
            return Err(Unanswered::Refused(response.status));
        }
        self.answered(&response.body, questions)
    }

    /// The answer `body`, all it answered, holds to each of `questions`, by
    /// question id, read within the model's cap; a question it did not
    /// answer is left out.
    ///
    /// # Errors
    ///
    /// Where its answer is too large or not the shape asked for.
    pub fn answered(
        &self,
        body: &[u8],
        questions: &[Question<'_>],
    ) -> Result<BTreeMap<String, Answer>, Unanswered> {
        let ids: Vec<&str> = questions.iter().map(|question| question.id).collect();
        let most = self.most;
        let named = (body, self.model.as_str());
        match self.speaks {
            Speaks::TypeSafe => typesafe::answers(most, body, &ids),
            Speaks::OpenAi => openai::answers(most, body, &ids),
            Speaks::Anthropic => anthropic::answers(most, body, &ids),
            Speaks::Gemini => gemini::answers(most, named, &ids),
            Speaks::Bedrock => bedrock::answers(most, named, &ids),
            Speaks::WorkersAi => workers::answers(most, named, &ids),
            Speaks::Spin => spin::answers(most, named, &ids),
        }
    }
}

/// A server that speaks OpenAI's chat completions, by the name a config
/// gives it: its base where the config names none, and the header its key
/// goes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// Its name, as a config's `kind` names it.
    pub name: &'static str,
    /// Its base.
    pub base: &'static str,
    /// The header its key goes in, where it is not `Authorization: Bearer`.
    pub header: Option<&'static str>,
}

/// Every preset herald knows, the servers that run on a machine of the
/// operator's own first.
pub const PRESETS: [Preset; 19] = [
    Preset {
        name: "ollama",
        base: "http://127.0.0.1:11434/v1",
        header: None,
    },
    Preset {
        name: "llama-cpp",
        base: "http://127.0.0.1:9931/v1",
        header: None,
    },
    Preset {
        name: "lm-studio",
        base: "http://localhost:1234/v1",
        header: None,
    },
    Preset {
        name: "vllm",
        base: "http://localhost:8000/v1",
        header: None,
    },
    Preset {
        name: "localai",
        base: "http://localhost:8080/v1",
        header: None,
    },
    Preset {
        name: "jan",
        base: "http://127.0.0.1:1337/v1",
        header: None,
    },
    Preset {
        name: "koboldcpp",
        base: "http://localhost:5001/v1",
        header: None,
    },
    Preset {
        name: "gpt4all",
        base: "http://localhost:4891/v1",
        header: None,
    },
    Preset {
        name: "openai",
        base: "https://api.openai.com/v1",
        header: None,
    },
    Preset {
        name: "mistral",
        base: "https://api.mistral.ai/v1",
        header: None,
    },
    Preset {
        name: "openrouter",
        base: "https://openrouter.ai/api/v1",
        header: None,
    },
    Preset {
        name: "groq",
        base: "https://api.groq.com/openai/v1",
        header: None,
    },
    Preset {
        name: "together",
        base: "https://api.together.ai/v1",
        header: None,
    },
    Preset {
        name: "deepseek",
        base: "https://api.deepseek.com",
        header: None,
    },
    Preset {
        name: "cohere",
        base: "https://api.cohere.ai/compatibility/v1",
        header: None,
    },
    Preset {
        name: "azure",
        base: "",
        header: Some("api-key"),
    },
    Preset {
        name: "scaleway",
        base: "https://api.scaleway.ai/v1",
        header: None,
    },
    Preset {
        name: "huggingface",
        base: "https://router.huggingface.co/v1",
        header: None,
    },
    Preset {
        name: WORKERS_AI,
        base: "",
        header: None,
    },
];

/// The preset reaching Workers AI's REST API, and with a binding, the
/// models Workers AI runs beside herald.
pub const WORKERS_AI: &str = "workers-ai";

/// The base TypeSafe's API is reached at where a config names none.
pub const TYPESAFE_BASE: &str = "https://api.typesafe.ai/v1";

/// The preset named `name`.
#[must_use]
pub fn named(name: &str) -> Option<Preset> {
    PRESETS.into_iter().find(|preset| preset.name == name)
}

#[cfg(test)]
mod tests;
