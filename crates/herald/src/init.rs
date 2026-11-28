//! `herald init`: a first run's wizard (ADR-0091). It asks the forge, the
//! repository, the chat and the chat's own questions, each also a flag
//! with its default chosen; picks the store; writes `herald.toml` once
//! confirmed, never over a config that exists; and offers one test post.
//! It names each secret, never writes a credential's value, and reads one
//! for the test post alone.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use herald_config::{Preset, SHIPPED};
use herald_kinds::{Answer, FirstRun, Variant, Written};
use herald_sources_forge::Forge;
use tokio::io::AsyncBufRead;

use self::remote::Remote;
pub(crate) use self::tested::Trying;
use self::written::{First, Kept, STORE_KEY, STORE_TOKEN};
use crate::asking::{Asking, NONE, Question};
use crate::{DONE, STOPPED};

mod remote;
mod tested;
mod written;

/// herald's page on making each credential a first run names.
const GUIDE: &str = concat!(herald_core::repository!(), "/blob/main/.docs/first-run.md");

/// The preset a first run announces with where none is chosen.
const ANNOUNCED: &str = "herald:announce-releases";

/// The parameters a first run gives each preset it adopts.
const GIVEN: [&str; 2] = ["destination", "sources"];

/// Why a webhook or email is not offered on GitLab.
const UNKEPT: &str = "no store keeps herald's records in a GitLab repository yet, so on GitLab a first run takes only a way in that reads its posts back, which keeps \"sent once\" in the posts themselves";

/// Why `--store-token` was given where no Forgejo branch store is picked.
const TOKENLESS: &str = "--store-token names the token of a Forgejo repository's records branch, which herald picks for a chat way in that cannot read its posts back on Forgejo alone; leave it out";

/// The answers given as flags; each question none answers is asked.
#[derive(Debug, clap::Args)]
pub(crate) struct Answers {
    /// The forge the repository is on: github, gitlab or forgejo.
    #[arg(long)]
    pub(crate) forge: Option<String>,
    /// The repository, by its path on the forge, such as owner/name.
    #[arg(long)]
    pub(crate) repository: Option<String>,
    /// The forge's API base URL, or "none" for its public instance's.
    #[arg(long)]
    pub(crate) api: Option<String>,
    /// The kind of chat it announces to.
    #[arg(long)]
    pub(crate) chat: Option<String>,
    /// The way into the chat, such as webhook or bot.
    #[arg(long)]
    pub(crate) variant: Option<String>,
    /// An answer to one of the chat's own questions, as KEY=VALUE, once
    /// per question.
    #[arg(long = "with", value_name = "KEY=VALUE")]
    pub(crate) with: Vec<String>,
    /// A preset it announces with, once per preset; "none" for none.
    #[arg(long)]
    pub(crate) announce: Vec<String>,
    /// The name of the secret holding a token that may push to the
    /// Forgejo repository's records branch; never the token.
    #[arg(long)]
    pub(crate) store_token: Option<String>,
    /// Take the default of each question no flag answers, and write
    /// without asking; no test message is posted.
    #[arg(long)]
    pub(crate) yes: bool,
    /// Print the config in place of writing it.
    #[arg(long)]
    pub(crate) print: bool,
}

/// The first run of the config at `path`, as `asked` and the operator
/// answer, with the system's random generator for the store's key, its
/// test post as `trying` reads and posts it.
pub(crate) async fn initialising(
    path: &str,
    asked: &Answers,
    (answers, out): (&mut (dyn AsyncBufRead + Send + Unpin), &mut dyn Write),
    (random, trying): (impl FnOnce() -> Option<String>, &Trying<'_>),
) -> Result<u8, String> {
    if Path::new(path).exists() {
        return Err(format!(
            "{path} exists, so nothing was written: add to it with herald setup …, and judge it with herald doctor"
        ));
    }
    let directory = Path::new(path)
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let remote = remote::of(directory);
    let mut asking = Asking {
        answers,
        out,
        yes: asked.yes,
    };
    let (forge, repository, api) = sourced(remote.as_ref(), asked, &mut asking).await?;
    let runs = herald_compose::kinds().first_runs();
    let run = chat(&runs, asked, &mut asking).await?;
    let (variant, store) = way_in(run, forge, asked, &mut asking).await?;
    let keys = variant.written(&answered(run, variant, asked, &mut asking).await?);
    let presets = announced(asked, &mut asking).await?;
    let store = kept((store, variant), asked, &mut asking).await?;
    let first = First {
        forge,
        repository: &repository,
        api: api.as_deref(),
        kind: run.kind,
        keys: &keys,
        presets: &presets,
        store: &store,
    };
    let text = first.text();
    let config =
        crate::loaded(path, &text).map_err(|why| format!("{why}\nNothing was written."))?;
    if asked.print {
        let _ = write!(asking.out, "{text}");
        return Ok(DONE);
    }
    let written = finish((path, &text), (&first, variant), random, &mut asking).await?;
    if written != DONE {
        return Ok(written);
    }
    Ok(tested::tried(&config, path, trying, &mut asking).await)
}

/// One of `offered`, each said on its own line where the operator is
/// asked.
async fn picked<T: Clone>(
    asking: &mut Asking<'_>,
    question: Question<'_>,
    offered: &[(&str, &str, T)],
) -> Result<T, String> {
    if question.given.is_none() && !asking.yes {
        for (name, described, _) in offered {
            asking.say(&format!("  {name}: {described}"));
        }
    }
    let names = offered
        .iter()
        .map(|(name, ..)| *name)
        .collect::<Vec<_>>()
        .join(", ");
    asking
        .answer(question, |answer| {
            offered
                .iter()
                .find(|(name, ..)| *name == answer)
                .map(|(.., value)| value.clone())
                .ok_or_else(|| format!("answer one of {names}"))
        })
        .await
}

/// What each forge is, as a first run offers it.
const fn described(forge: Forge) -> &'static str {
    match forge {
        Forge::GitHub => "GitHub, a GitHub Enterprise Server or a GHE.com tenant",
        Forge::GitLab => "GitLab.com or a self-managed GitLab",
        Forge::Forgejo => "Codeberg or another Forgejo or Gitea",
    }
}

/// The forge, the repository and the API base asked, each defaulting to
/// what `remote` says.
async fn sourced(
    remote: Option<&Remote>,
    asked: &Answers,
    asking: &mut Asking<'_>,
) -> Result<(Forge, String, Option<String>), String> {
    if let Some(remote) = remote {
        asking.say(&format!(
            "The repository's origin is {} on {}.",
            remote.path, remote.host
        ));
    }
    let offered = Forge::ALL.map(|forge| (forge.kind(), described(forge), forge));
    let known = remote.and_then(|remote| Forge::at(&remote.host));
    let forge = picked(
        asking,
        Question {
            asked: "The forge the repository is on",
            flag: "forge",
            given: asked.forge.as_deref(),
            default: known.map(|forge| forge.kind().to_owned()),
        },
        &offered,
    )
    .await?;
    let repository = asking
        .answer(
            Question {
                asked: &format!("The repository, as {}", forge.shape()),
                flag: "repository",
                given: asked.repository.as_deref(),
                default: remote.map(|remote| remote.path.clone()),
            },
            |answer| {
                let names = answer.split('/').collect::<Vec<_>>();
                (forge.names().contains(&names.len()) && names.iter().all(|name| !name.is_empty()))
                    .then(|| answer.to_owned())
                    .ok_or_else(|| format!("give the repository as {}", forge.shape()))
            },
        )
        .await?;
    let host = remote
        .map(|remote| remote.host.as_str())
        .filter(|host| *host != forge.public_host());
    let public = forge.public_host();
    let api = asking
        .answer(
            Question {
                asked: &format!("The forge's API base URL; {NONE} for {public}'s"),
                flag: "api",
                given: asked.api.as_deref(),
                default: Some(host.map_or_else(|| NONE.to_owned(), |host| forge.api_at(host))),
            },
            |answer| match answer {
                NONE => Ok(None),
                _ if answer.starts_with("https://") => {
                    Ok(Some(answer.trim_end_matches('/').to_owned()))
                }
                _ => Err(format!(
                    "give the API's https:// base URL, or {NONE} for {public}'s"
                )),
            },
        )
        .await?;
    Ok((forge, repository, api))
}

/// The kind of chat asked, among the kinds this build sets up.
async fn chat<'r>(
    runs: &'r [FirstRun],
    asked: &Answers,
    asking: &mut Asking<'_>,
) -> Result<&'r FirstRun, String> {
    let offered = runs
        .iter()
        .map(|run| (run.kind, run.described, run))
        .collect::<Vec<_>>();
    picked(
        asking,
        Question {
            asked: "The chat it announces to",
            flag: "chat",
            given: asked.chat.as_deref(),
            default: runs.first().map(|run| run.kind.to_owned()),
        },
        &offered,
    )
    .await
}

/// The way into `run` asked, among those a store is picked for, and that
/// store: on GitLab, only one that reads its posts back.
async fn way_in(
    run: &FirstRun,
    forge: Forge,
    asked: &Answers,
    asking: &mut Asking<'_>,
) -> Result<(&'static Variant, Kept), String> {
    let offered = run
        .variants
        .iter()
        .filter_map(|variant| {
            Kept::picked(forge, variant.reads_back)
                .map(|store| (variant.name, variant.described, (variant, store)))
        })
        .collect::<Vec<_>>();
    let unkept = offered.len() < run.variants.len();
    let refused = asked
        .variant
        .as_deref()
        .is_some_and(|name| !offered.iter().any(|(offered, ..)| *offered == name));
    let named = run
        .variants
        .iter()
        .any(|variant| asked.variant.as_deref() == Some(variant.name));
    if (refused && named) || offered.is_empty() {
        return Err(format!(
            "{UNKEPT}; choose a way into {} that reads its posts back, or another chat",
            run.kind
        ));
    }
    if unkept {
        asking.say(&format!("{UNKEPT}."));
    }
    picked(
        asking,
        Question {
            asked: &format!("The way into {}", run.kind),
            flag: "variant",
            given: asked.variant.as_deref(),
            default: offered.first().map(|(name, ..)| (*name).to_owned()),
        },
        &offered,
    )
    .await
}

/// The answers to `variant`'s own questions, in its order, each taken from
/// its `--with` where given.
async fn answered(
    run: &FirstRun,
    variant: &Variant,
    asked: &Answers,
    asking: &mut Asking<'_>,
) -> Result<Vec<String>, String> {
    let keys = variant
        .asks
        .iter()
        .map(|ask| ask.key)
        .collect::<Vec<_>>()
        .join(", ");
    let mut given = BTreeMap::new();
    for pair in &asked.with {
        let (key, answer) = pair
            .split_once('=')
            .and_then(|(key, answer)| {
                let known = variant.asks.iter().find(|ask| ask.key == key)?;
                Some((known.key, answer))
            })
            .ok_or_else(|| {
                format!(
                    "--with: an unknown key or no KEY=VALUE; the {} {} asks {keys}",
                    run.kind, variant.name
                )
            })?;
        if given.insert(key, answer).is_some() {
            return Err(format!("--with {key}: give it once"));
        }
    }
    let mut answers = Vec::new();
    for ask in variant.asks {
        let shown = match ask.answer {
            Answer::Secret { .. } => ask.asked.to_owned(),
            Answer::Text => format!("{}, such as {}", ask.asked, ask.example),
            Answer::Choice { .. } => format!("{}: {}", ask.asked, ask.example),
        };
        let flag = format!("with {}", ask.key);
        let answer = asking
            .answer(
                Question {
                    asked: &shown,
                    flag: &flag,
                    given: given.get(ask.key).copied(),
                    default: ask.answer.default().map(ToOwned::to_owned),
                },
                |answer| ask.answer.checked(answer).map(|()| answer.to_owned()),
            )
            .await?;
        answers.push(answer);
    }
    Ok(answers)
}

/// The presets asked, among those whose every required parameter a first
/// run gives.
async fn announced(
    asked: &Answers,
    asking: &mut Asking<'_>,
) -> Result<Vec<&'static Preset>, String> {
    let offered = SHIPPED
        .iter()
        .filter(|preset| {
            preset
                .parameters
                .iter()
                .all(|parameter| !parameter.required || GIVEN.contains(&parameter.key))
        })
        .collect::<Vec<_>>();
    if asked.announce.is_empty() && !asking.yes {
        for preset in &offered {
            asking.say(&format!("  {}: {}", preset.name, preset.described));
        }
    }
    let names = asking
        .list(
            Question {
                asked: "The presets it announces with",
                flag: "announce",
                given: None,
                default: None,
            },
            &asked.announce,
            &[ANNOUNCED.to_owned()],
        )
        .await?;
    let mut chosen: Vec<&'static Preset> = Vec::new();
    for name in &names {
        let preset = offered
            .iter()
            .find(|preset| preset.name == name.as_str())
            .ok_or_else(|| {
                let listed = offered
                    .iter()
                    .map(|preset| preset.name)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("--announce: a preset a first run does not adopt; it adopts {listed}")
            })?;
        if !chosen.iter().any(|was| was.name == preset.name) {
            chosen.push(preset);
        }
    }
    Ok(chosen)
}

/// `store`, picked for `variant`, said with why on one line, a Forgejo
/// branch's token named as asked.
async fn kept(
    (store, variant): (Kept, &Variant),
    asked: &Answers,
    asking: &mut Asking<'_>,
) -> Result<Kept, String> {
    if asked.store_token.is_some() && !matches!(store, Kept::ForgejoBranch { .. }) {
        return Err(TOKENLESS.to_owned());
    }
    let store = match store {
        Kept::Markers => {
            asking.say(&format!(
                "The {} reads its posts back, so the markers store keeps \"sent once\" in them and needs nothing.",
                variant.name
            ));
            Kept::Markers
        }
        Kept::ActionsCache => {
            asking.say(&format!(
                "A {} cannot read its posts back, so herald keeps its records in the GitHub Actions cache, sealed with a key init makes.",
                variant.name
            ));
            Kept::ActionsCache
        }
        Kept::ForgejoBranch { token } => {
            let secret = Answer::Secret {
                default: STORE_TOKEN,
            };
            let token = asking
                .answer(
                    Question {
                        asked: "The secret that holds a token that may push to the repository's herald-state branch",
                        flag: "store-token",
                        given: asked.store_token.as_deref(),
                        default: Some(token),
                    },
                    |answer| secret.checked(answer).map(|()| answer.to_owned()),
                )
                .await?;
            asking.say(&format!(
                "A {} cannot read its posts back, so herald keeps its records on the repository's herald-state branch, sealed with a key init makes.",
                variant.name
            ));
            Kept::ForgejoBranch { token }
        }
    };
    Ok(store)
}

/// Where `forge` keeps a secret named `name`.
fn kept_as(forge: Forge, name: &str) -> String {
    match forge {
        Forge::GitHub => format!("a repository secret (gh secret set {name})"),
        Forge::GitLab => "a masked CI/CD variable".to_owned(),
        Forge::Forgejo => "a repository secret".to_owned(),
    }
}

/// The config `text` shown, then written to `path` once confirmed, with
/// each secret `first` names said: its name, where the forge keeps it and
/// how it is made.
async fn finish(
    (path, text): (&str, &str),
    (first, variant): (&First<'_>, &Variant),
    random: impl FnOnce() -> Option<String>,
    asking: &mut Asking<'_>,
) -> Result<u8, String> {
    asking.say("");
    let _ = write!(asking.out, "{text}");
    if !asking.confirmed(&format!("Write {path}?")).await {
        asking.say("Nothing was written.");
        return Ok(STOPPED);
    }
    let key = if first.store.sealed() {
        Some(random().ok_or(
            "the system's random generator failed, so no store key was made; nothing was written",
        )?)
    } else {
        None
    };
    crate::created(path, text)?;
    asking.say(&format!("Wrote {path}."));
    let guide = format!("{GUIDE}#{}-{}", first.kind, variant.name);
    for written in first.keys {
        if let Written::Secret(name) = &written.value {
            asking.say(&format!(
                "Store {name} as {}; {guide} says how to make it.",
                kept_as(first.forge, name)
            ));
        }
    }
    if let Kept::ForgejoBranch { token } = first.store {
        asking.say(&format!(
            "Store {token} as {}; {GUIDE}#forgejo-branch says how to make it and the branch.",
            kept_as(first.forge, token)
        ));
    }
    if let Some(key) = key {
        asking.say(&format!(
            "Store {STORE_KEY} as {}: it is the key below, which herald prints only this once.",
            kept_as(first.forge, STORE_KEY)
        ));
        asking.say(&format!("{STORE_KEY}={key}"));
    }
    asking.say(&format!(
        "herald --config {path} doctor names anything it still lacks."
    ));
    Ok(DONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENDS: FirstRun = FirstRun {
        kind: "mail",
        described: "mail",
        variants: &[Variant {
            name: "send",
            described: "it sends only",
            reads_back: false,
            asks: &[],
        }],
    };

    #[test]
    fn a_chat_with_no_way_in_that_reads_back_is_refused_on_gitlab_alone() {
        let asked = Answers {
            forge: None,
            repository: None,
            api: None,
            chat: None,
            variant: None,
            with: Vec::new(),
            announce: Vec::new(),
            store_token: None,
            yes: true,
            print: false,
        };
        let mut out = Vec::new();
        let mut answers: &[u8] = b"";
        let mut asking = Asking {
            answers: &mut answers,
            out: &mut out,
            yes: true,
        };
        let refused = pollster::block_on(way_in(&SENDS, Forge::GitLab, &asked, &mut asking));
        assert_eq!(
            refused.map(|(variant, _)| variant.name),
            Err(format!(
                "{UNKEPT}; choose a way into mail that reads its posts back, or another chat"
            ))
        );
        let taken = pollster::block_on(way_in(&SENDS, Forge::GitHub, &asked, &mut asking));
        assert_eq!(
            taken.map(|(variant, store)| (variant.name, store)),
            Ok(("send", Kept::ActionsCache))
        );
    }
}
