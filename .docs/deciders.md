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
| `kind` | `typesafe`, `openai-compatible`, or a preset: `ollama`, `llama-cpp`, `lm-studio`, `vllm`, `localai`, `jan`, `koboldcpp`, `gpt4all`, `openai`, `mistral`, `openrouter`, `groq`, `together`, `deepseek`, `cohere`, `azure`, `scaleway`, `huggingface`, `workers-ai` | required |
| `model` | the model's id, pinned | required |
| `base` | the API's base | the kind's; required for `openai-compatible`, `azure` and `workers-ai` |
| `key` | its key, as a secret | none |
| `header` | the header the key goes in | `Authorization: Bearer`, `api-key` for `azure` |
| `private` | whether it may be shown private and internal repositories' pull requests | `false` |
| `insecure` | whether it may be reached over plain HTTP away from this machine | `false` |
| `most` | the most bytes of its answer read | `65536` |

A base is HTTPS, or plain HTTP on this machine (`localhost`, `127.0.0.1`,
`[::1]`), or plain HTTP elsewhere with `insecure = true`.

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
