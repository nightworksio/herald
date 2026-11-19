use std::fmt;
use std::future::Future;

use crate::erased::Boxed;

/// A model a host runs itself, reached through its binding rather than a
/// URL (ADR-0059).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding<'a> {
    /// Cloudflare Workers AI, through the Worker's binding of this name.
    WorkersAi(&'a str),
    /// Spin's `llm` interface.
    SpinLlm,
}

impl fmt::Display for Binding<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkersAi(name) => write!(f, "the Workers AI binding {name}"),
            Self::SpinLlm => f.write_str("Spin's llm interface"),
        }
    }
}

/// What a host's model is asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inference {
    /// The model's id, as its host names it.
    pub model: String,
    /// What it is given, as its binding takes it: Workers AI's JSON
    /// inputs, or Spin's prompt.
    pub inputs: Vec<u8>,
    /// The most tokens it answers within.
    pub tokens: u32,
    /// The most bytes of its answer read.
    pub most: usize,
}

/// Why a host's model gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferError {
    /// Its answer was larger than the most read of it, in bytes.
    TooLarge {
        /// The most read.
        most: usize,
    },
    /// The host or its model failed.
    Failed {
        /// What the host said.
        reason: String,
    },
}

impl fmt::Display for InferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { most } => write!(f, "the model's answer is larger than {most} bytes"),
            Self::Failed { reason } => write!(f, "the host's model failed: {reason}"),
        }
    }
}

impl InferError {
    /// The host or its model failed, for `reason`.
    #[must_use]
    pub fn failed(reason: impl fmt::Display) -> Self {
        Self::Failed {
            reason: reason.to_string(),
        }
    }
}

/// A model a host runs itself.
pub trait Infer: Sync {
    /// What the model answers `inference`, at most `inference.most` bytes.
    fn infer(
        &self,
        inference: Inference,
    ) -> impl Future<Output = Result<Vec<u8>, InferError>> + Send;
}

/// [`Infer`] as a trait object: every `Infer` is one.
pub trait DynInfer: Sync + Send + fmt::Debug {
    /// What the model answers `inference`.
    fn infer_boxed(&self, inference: Inference) -> Boxed<'_, Result<Vec<u8>, InferError>>;
}

impl<I: Infer + Send + fmt::Debug> DynInfer for I {
    fn infer_boxed(&self, inference: Inference) -> Boxed<'_, Result<Vec<u8>, InferError>> {
        Box::pin(self.infer(inference))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Echo;

    impl Infer for Echo {
        fn infer(
            &self,
            inference: Inference,
        ) -> impl Future<Output = Result<Vec<u8>, InferError>> + Send {
            std::future::ready(Ok(inference.inputs))
        }
    }

    #[test]
    fn a_host_s_model_answers_through_its_trait_object() {
        let erased: &dyn DynInfer = &Echo;
        let answered = pollster::block_on(erased.infer_boxed(Inference {
            model: "m".to_owned(),
            inputs: b"asked".to_vec(),
            tokens: 1,
            most: 8,
        }));
        assert_eq!(answered, Ok(b"asked".to_vec()));
    }

    #[test]
    fn a_binding_is_named_as_its_host_names_it() {
        assert_eq!(
            Binding::WorkersAi("AI").to_string(),
            "the Workers AI binding AI"
        );
        assert_eq!(Binding::SpinLlm.to_string(), "Spin's llm interface");
    }

    #[test]
    fn why_a_host_s_model_gave_no_answer_is_said() {
        assert_eq!(
            InferError::TooLarge { most: 8 }.to_string(),
            "the model's answer is larger than 8 bytes"
        );
        assert_eq!(
            InferError::failed("no binding AI").to_string(),
            "the host's model failed: no binding AI"
        );
    }
}
