# Deciders: what a model decides where a view's rules cannot tell

A view tells whose turn each open pull request is by herald's rules. Where
they leave a closed-set question untold, a view may ask a task, and the
task asks its models in turn. Rules come first, a model answers only among
the options the config wrote, and every answer is kept with the model that
gave it ([ADR-0057](decisions/0057-a-view-asks-a-model-only-what-its-rules-cannot-tell-and-keeps-every-answer.md)).

Only a build with the `models` feature asks models; release artefacts are
built without it, and there every task answers unknown.

```toml
[models.jev]
kind = "typesafe"
model = "jev-1.14.0"
key = { secret = "HERALD_TYPESAFE_KEY" }

[models.local]
kind = "ollama"
model = "qwen3:8b"
private = true

[tasks.urgency]
models = ["jev", "local"]
instructions = "How urgent is this pull request?"
options = { high = "it blocks a release", normal = "it should land this week", low = "it can wait" }
least = 0.8

[views.review]
decide = { priority = "urgency" }
```

## Models

| Key | What it sets | Where absent |
|-----|--------------|--------------|
| `kind` | `typesafe`, `openai-compatible`, `anthropic` (Anthropic's Messages API), `gemini` (Gemini's API), `bedrock` (Amazon Bedrock's Converse API), `spin` (a model Spin runs), or a preset of `openai-compatible`: `ollama`, `llama-cpp`, `lm-studio`, `vllm`, `localai`, `jan`, `koboldcpp`, `gpt4all`, `openai`, `mistral`, `openrouter`, `groq`, `together`, `deepseek`, `cohere`, `azure`, `scaleway`, `huggingface`, `workers-ai` | required |
| `model` | the model's id, pinned | required |
| `base` | the API's base | the kind's; the region's endpoint for `bedrock`; required for `openai-compatible`, `azure` and `workers-ai` over REST |
| `key` | its key, as a secret; a Bedrock API key for `bedrock`, sent in place of a signature | none |
| `header` | the header the key goes in | `Authorization: Bearer`; `api-key` for `azure`, `x-api-key` for `anthropic`, `x-goog-api-key` for `gemini` |
| `private` | whether it may be shown private and internal repositories' pull requests | `false` |
| `insecure` | whether it may be reached over plain HTTP away from this machine | `false` |
| `most` | the most bytes of its answer read | `65536` |
| `tokens` | the most tokens `anthropic`, `gemini`, `bedrock`, `workers-ai` through a binding and `spin` answer within | `4096` |
| `region` | the AWS region a `bedrock` model is reached in, in any partition | required for `bedrock` |
| `access_key_id`, `secret_access_key`, `session_token` | the secrets a `bedrock` request is signed with where it names no `key`; a session token is sent where it is set | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN` |
| `binding` | the Worker's binding a `workers-ai` model is reached through, in place of Workers AI's REST API | none |

A base is HTTPS, or plain HTTP on this machine (`localhost`, `127.0.0.1`,
`[::1]`), or plain HTTP elsewhere with `insecure = true`.

### Amazon Bedrock

A `bedrock` model is asked through the Converse API with a JSON schema in
`outputConfig`, at `bedrock-runtime` in its `region`'s partition
(GovCloud, China and the ISO regions too); `base` reaches a FIPS or VPC
endpoint instead. Each request is signed with AWS Signature Version 4,
with the access key a Lambda's role provides where the secrets are not
named, and the role needs `bedrock:InvokeModel`. A `key` sends a Bedrock
API key instead. Where a prompt router chose the model, the answer is
kept as that model's.

```toml
[models.claude]
kind = "bedrock"
region = "eu-west-1"
model = "eu.anthropic.claude-sonnet-x-v1:0"
private = true
```

### Models a host runs beside herald

A model the host runs is reached through its binding, never through a
URL, and takes neither `base` nor `key`; only the host that runs it asks
it, and doctor names the gap on every other.

- On a Cloudflare Worker, `kind = "workers-ai"` with `binding` asks
  Workers AI through the Worker's binding, named in `wrangler.toml` as
  `[ai] binding = "AI"`; without `binding` it is Workers AI's REST API.
- On Spin, `kind = "spin"` asks a model Spin runs through its `llm`
  interface, which the component lists in `spin.toml` as
  `ai_models = ["llama2-chat"]`. Spin takes one prompt, so the questions,
  the JSON schema and the state go in it in that order, the state last as
  one JSON string, and the answer is the JSON object its text holds.

A bound model is counted as one outbound request a time it is asked, and
its answer is read within `most`, as every other model's is.

## Tasks

| Key | What it sets | Where absent |
|-----|--------------|--------------|
| `models` | the models asked, in turn, until one answers | required |
| `instructions` | what to decide | required |
| `options` | 2 to 32 options, each a key and what it means | required |
| `least` | the least probability an answer is taken at; no answer without a probability is taken where it is given | any answer among the options |
| `most` | the most pull requests asked a round | `50` |

## What a view asks

`decide` names a task for each slot:

| Slot | Asked of | What it settles |
|------|----------|-----------------|
| `turn` | a pull request whose turn the view tells nobody's | its state, reason `decided: <task>`; the options are states |
| `priority` | every pull request | `row.decided.priority` |
| `reviewer` | one waiting on review that nobody was asked to review | `row.decided.reviewer` |
| `noise` | a stale one | `row.decided.noise` |
| `failure` | one whose check failed | `row.decided.failure` |

A model is shown each pull request's repository, number, title, author,
labels, reviewers, whether it is a draft or a bot's, and the state and
reason the rules gave it, as one JSON value it is told is untrusted. An
answer off the options, too unsure, late or unreadable reads as unknown.
A decision never by itself merges, labels or writes anything.

## What is kept

Each answer is kept per task, pull request and input, with the model, its
probabilities and the time, and reused while the pull request is
unchanged. `[store] keep_decisions` (`"30d"` where absent, or `"forever"`)
sets how long. The markers store keeps none, so there no task is asked.
