//! The decision models herald may ask a closed-set question (ADR-0057):
//! TypeSafe's Jev, and every server that speaks OpenAI's chat completions,
//! from a local Ollama to a hosted provider. Each request carries the
//! state as data and the questions with the options the config wrote, and
//! each answer is read within the model's cap and checked against those
//! options by its caller.

use std::collections::BTreeMap;

use herald_core::{Answer, Question};
use herald_ports::{Method, Request, Response, Secret, SecretText};
use serde::de::DeserializeOwned;

mod openai;
mod typesafe;

/// The most bytes of a model's answer read where its config names no
/// other cap.
pub const MOST_ANSWER: usize = 65_536;

/// How a model is spoken to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaks {
    /// TypeSafe's System One API, `POST <base>/systemone`.
    TypeSafe,
    /// OpenAI's chat completions, `POST <base>/chat/completions`, with a
    /// JSON schema the answer holds to.
    OpenAi,
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

impl Model {
    /// The request asking `questions` of `state`, the state sent as data
    /// apart from the instructions, never as text an instruction may be
    /// written into.
    #[must_use]
    pub fn request(&self, state: &str, questions: &[Question<'_>]) -> Request {
        let (path, body) = match self.speaks {
            Speaks::TypeSafe => (
                typesafe::PATH,
                typesafe::body(&self.model, state, questions),
            ),
            Speaks::OpenAi => (openai::PATH, openai::body(&self.model, state, questions)),
        };
        let url = format!("{}{path}", self.base.trim_end_matches('/'));
        let request = Request::new(Method::Post, SecretText::plain(url))
            .header("Content-Type", SecretText::plain(herald_core::media::JSON))
            .body(body.to_string())
            .at_most(self.most);
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
        let ids: Vec<&str> = questions.iter().map(|question| question.id).collect();
        match self.speaks {
            Speaks::TypeSafe => typesafe::answers(self.most, &response.body, &ids),
            Speaks::OpenAi => openai::answers(self.most, &response.body, &ids),
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
        name: "workers-ai",
        base: "",
        header: None,
    },
];

/// The base TypeSafe's API is reached at where a config names none.
pub const TYPESAFE_BASE: &str = "https://api.typesafe.ai/v1";

/// The preset named `name`.
#[must_use]
pub fn named(name: &str) -> Option<Preset> {
    PRESETS.into_iter().find(|preset| preset.name == name)
}

#[cfg(test)]
mod tests;
