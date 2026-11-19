# ADR-0059: A model is asked through its provider's own API, or the binding of the host that runs it

**Status:** Accepted
**Date:** 2026-11-19

## Context

A task asks its models in turn (ADR-0057): TypeSafe's Jev and every
server speaking OpenAI's chat completions. Anthropic, Gemini and Amazon
Bedrock speak APIs of their own, Bedrock behind AWS Signature Version 4,
and a Cloudflare Worker and a Spin component can ask models their host
runs beside them through a binding rather than a URL. A binding is not an
HTTP request: it has no URL, headers or status.

## Decision

1. **Native kinds.** `anthropic` asks `POST <base>/messages` with the
   `anthropic-version` header and a JSON schema in `output_config`, and
   its key goes in `x-api-key`. `gemini` asks
   `POST <base>/models/<model>:generateContent` with
   `responseJsonSchema`, and its key goes in `x-goog-api-key`; an answer
   is kept as the model version Gemini names. Each takes `tokens` (4096),
   and `base` and `header` reach an enterprise endpoint, such as
   Anthropic on Azure AI Foundry.
2. **Bedrock.** `bedrock` asks the Converse API,
   `POST <base>/model/<model>/converse`, with a JSON schema in
   `outputConfig`, at `bedrock-runtime` in `region`'s partition, which
   is required; `base` reaches a FIPS or VPC endpoint. The model's id is
   percent-encoded in the path, and twice in the signature's canonical
   URI. Each request is signed with AWS Signature Version 4 for the
   service `bedrock`, with `access_key_id`, `secret_access_key` and
   `session_token`, read as the dynamodb store reads them
   (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and `AWS_SESSION_TOKEN`
   sent where it is set). A `key` is a Bedrock API key, sent as a bearer
   token in place of a signature. An answer a prompt router gave is kept
   as the model it invoked.
3. **A host's own models are a port.** `Infer` in `herald-ports` takes
   an `Inference` (the model's id, its inputs as the binding takes them,
   `tokens` and `most`) and answers at most `most` bytes. A host offers
   one through `Host::inference(Binding)`; every host refuses by default.
   A Worker answers `Binding::WorkersAi(<binding>)` through the shim's
   `ai(binding).run(model, inputs)`, and a Spin component answers
   `Binding::SpinLlm` through `fermyon:spin/llm@2.0.0`.
4. **Config.** `kind = "workers-ai"` with `binding` asks Workers AI
   through the Worker's binding, with the messages and a JSON schema in
   `response_format`; without `binding` it stays the OpenAI-compatible
   REST preset. `kind = "spin"` asks a model Spin runs in one prompt: the
   instructions, the questions, the JSON schema, and the state last as
   one JSON string; the answer is the JSON object between the text's
   first `{` and last `}`. A bound model takes neither `base` nor `key`.
5. **No way around the limits.** A bound model is counted on the
   composition's outbound count as a request is, the run's budget is
   checked before each, and its answer is read within `most`.
6. **Only the host that runs it asks it.** Composition refuses a config
   whose bound model the host runs no binding for, naming the model.
   Doctor's layers name the models each runs, so every other layer has
   a gap at `models.<name>.kind`, ranked to the first layer that runs it,
   and its note says which binding a bound model is reached through.

## Consequences

- Hosted models, Bedrock in GovCloud, China and the ISO regions, and
  models a Worker or Spin runs beside herald can each decide a task, with
  every answer kept as ADR-0057 keeps it.
- A model Spin runs takes no schema of its own, so a small model may
  answer outside the JSON asked for; such an answer reads as unknown.
- `herald-models` builds every request and reads every answer, sans-IO;
  only the two hosts' bindings are I/O, so a new host's model is one
  `Infer` and one `Binding` variant.
