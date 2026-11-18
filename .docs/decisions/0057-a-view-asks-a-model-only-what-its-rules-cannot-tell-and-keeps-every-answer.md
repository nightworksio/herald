# ADR-0057: A view asks a model only what its rules cannot tell, and keeps every answer

**Status:** Accepted
**Date:** 2026-11-18

## Context

A view tells whose turn each pull request is by herald's rules, the
forge's status or labels (ADR-0055). Some closed-set questions the rules
leave untold: whose turn it is where nothing tells, how urgent a pull
request is, who should review one nobody was asked to, whether a stale
one is waiting or noise, and what kind of failure a failed check is. A
decision model such as TypeSafe's Jev answers such questions with
calibrated probabilities, and any server speaking OpenAI's chat
completions, a local one included, can answer them with a JSON schema.
A pull request's title is written by whoever opens it, so whatever a
model is shown is untrusted.

## Decision

1. **Models and tasks.** `[models.<name>]` names a model: `kind`
   (`typesafe`, `openai-compatible`, or a preset of it such as `ollama`,
   `llama-cpp`, `lm-studio`, `vllm`, `openai`, `mistral`, `openrouter`,
   `groq`, `azure` or `workers-ai`), a pinned `model`, `base` (the
   kind's where absent), `key` as a secret, `header`, `most` (65536
   bytes of an answer read) and `insecure`. `[tasks.<name>]` names one
   question: `models` asked in turn, `instructions`, from 2 to 32
   `options` each with what it means, `least` (the least probability an
   answer is taken at) and `most` (50 pull requests asked a round).
2. **A view decides by slot.** `[views.<name>] decide` names the task for
   each of `turn`, `priority`, `reviewer`, `noise` and `failure`. Rules
   come first: whose turn is asked only where the view tells nobody's
   (never of a pull request read past `most_pulls`), a reviewer only
   where none was asked, noise only of a stale pull request, the failure
   only where a check failed. A task deciding whose turn it is offers
   only states.
3. **An answer is taken only among the options, sure enough.** An answer
   off the options, one below `least`, one with no probability where
   `least` is given, a timeout, a refusal and an unreadable answer read
   as unknown, never as a guess. A settled turn moves the row with the
   reason `decided: <task>`; every settled slot is the row's `decided`.
   A model's decision is marked so, narrows nothing a forge says, and
   never by itself drives a standing rule's apply mode or any write:
   writes re-check the forge's own state when they act.
4. **Every answer is kept.** A `decisions` record per task, pull request
   and input (what the model is shown, which leaves out how long it has
   waited) holds the model that answered, its probabilities, its answer
   and when. It is reused while the pull request is unchanged, and
   housekeeping forgets it after `[store] keep_decisions` (30 days). Every
   store keeps decisions but markers; where none are kept, no task is
   asked, and an answer that cannot be kept is not taken.
5. **Untrusted data stays data.** What a model is shown is one JSON
   value: a JSON field of TypeSafe's request, and one JSON-encoded string
   as the user's message to an OpenAI-compatible server, whose system
   message names it untrusted. No text it holds can end it.
6. **Private data needs an opt-in.** No model is shown a private or
   internal repository's pull request unless it says `private = true`,
   on this machine or elsewhere; with none, the task answers unknown.
7. **Off in release artefacts.** `herald-models` is always built and
   tested with fake models; the `models` feature of `herald-compose` and
   `herald` lets a build ask them, and release artefacts leave it off.
8. **Doctor** names which view asks which task of which models and what
   each is shown, which models may see private data, a model reached over
   plain HTTP, a model id that moves, a store that keeps no decisions,
   and a build without the feature.

## Consequences

- A queue can tell whose turn an untold pull request is, how urgent each
  is and what failed, and every such answer can be traced to the model
  that gave it.
- A decision costs one request per pull request and task the first time
  its input is seen, and none while it stands.
