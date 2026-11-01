//! Records Discord's fixtures from a real bot, channel, forum and webhooks,
//! sending the sample announcement the replay tests send. Each recording is
//! scrubbed to the ids the tests use and written next to the reference
//! fixtures only when it holds no credential (ADR-0004, ADR-0008).

use std::env;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use herald_core::{Chore, Entry, Facts, Sent, Step};
use herald_dest_discord::{Credential, Discord, DiscordMarkers, RouteOptions};
use herald_native::{NativeHttp, Reach};
use herald_ports::{HttpError, Markers, Secret};
use herald_testing::sample::{announcement, delivery, marker};
use herald_testing::{Ids, Recorder, Scrub};

const TOKEN: &str = "HERALD_DISCORD_TOKEN";
const CHANNEL: &str = "HERALD_DISCORD_CHANNEL";
const ROLE: &str = "HERALD_DISCORD_ROLE";
const FORUM: &str = "HERALD_DISCORD_FORUM";
const WEBHOOK: &str = "HERALD_DISCORD_WEBHOOK";
const FORUM_WEBHOOK: &str = "HERALD_DISCORD_FORUM_WEBHOOK";

const TEST_CHANNEL: &str = "1566184281406177531";
const TEST_FORUM: &str = "1566184281406177600";
const TEST_ROLE: &str = "1566190000000000001";
const TEST_BOT: &str = "1566190000000000099";
const FIRST_ID: u64 = 1_566_200_000_000_000_001;

struct Setup {
    token: Secret,
    channel: String,
    role: String,
    forum: Option<String>,
    webhook: Option<Secret>,
    forum_webhook: Option<Secret>,
}

impl Setup {
    fn from_env() -> Result<Self, String> {
        let required = |name: &str| optional(name).ok_or(format!("{name} is not set"));
        Ok(Self {
            token: Secret::new(TOKEN, required(TOKEN)?),
            channel: required(CHANNEL)?,
            role: required(ROLE)?,
            forum: optional(FORUM),
            webhook: optional(WEBHOOK).map(|url| Secret::new(WEBHOOK, url)),
            forum_webhook: optional(FORUM_WEBHOOK).map(|url| Secret::new(WEBHOOK, url)),
        })
    }

    fn bot(&self, channel: &str, forum: bool) -> Discord {
        Discord::new(
            Credential::Bot(self.token.clone()),
            RouteOptions {
                channel: Some(channel.to_owned()),
                forum,
            },
            None,
        )
    }
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn webhook(url: &Secret, forum: bool) -> Discord {
    Discord::new(
        Credential::Webhook(url.clone()),
        RouteOptions {
            channel: None,
            forum,
        },
        None,
    )
}

fn landed(step: &dyn fmt::Debug, sent: Result<Sent, HttpError>) -> Result<Facts, String> {
    match sent {
        Ok(Sent::Landed(facts)) => Ok(facts),
        Ok(Sent::RetryAfter(wait)) => Err(format!(
            "Discord asked herald to wait {wait:#} at {step:?}; run the recorder again after that"
        )),
        Ok(Sent::Refused(reason)) => Err(format!("{step:?}: {reason}")),
        Err(error) => Err(format!("{step:?}: {error}")),
    }
}

async fn post(
    discord: &Discord,
    recorder: &Recorder<NativeHttp>,
    steps: &[Step],
    role: &str,
) -> Result<Entry, String> {
    let sample = announcement();
    let mut entry = Entry::new(sample.key);
    for step in steps {
        let sent = discord
            .send(recorder, step, &sample, &delivery(role), &entry)
            .await;
        entry = entry.landed(step.clone(), landed(step, sent)?);
    }
    Ok(entry)
}

async fn record(setup: &Setup) -> Result<Vec<(&'static str, Recorder<NativeHttp>)>, String> {
    let bot = setup.bot(&setup.channel, false);
    let reach = Reach::new([bot.api()]).map_err(|error| error.to_string())?;
    let http = NativeHttp::new(reach).map_err(|error| error.to_string())?;
    let mut recordings = Vec::new();

    let recorder = Recorder::new(http.clone());
    let steps = [
        Step::Post,
        Step::Crosspost,
        Step::OpenThread,
        Step::Chunk(1),
        Step::Pin,
    ];
    let entry = post(&bot, &recorder, &steps, &setup.role).await?;
    let thread = entry
        .facts(&Step::OpenThread)
        .and_then(|facts| facts.thread.clone())
        .ok_or("Discord opened no thread")?;
    recordings.push(("announcement", recorder));

    let recorder = Recorder::new(http.clone());
    DiscordMarkers::new(&bot, &recorder)
        .entry(&announcement().key, &marker())
        .await
        .map_err(|error| error.to_string())?
        .ok_or("the announcement just posted was not read back")?;
    recordings.push(("markers", recorder));

    let recorder = Recorder::new(http.clone());
    for chore in [Chore::Archive, Chore::Lock] {
        landed(&chore, bot.housekeep(&recorder, chore, &thread).await)?;
    }
    recordings.push(("housekeeping", recorder));

    let followed = [Step::Post, Step::Chunk(1)];
    if let Some(forum) = &setup.forum {
        let recorder = Recorder::new(http.clone());
        post(&setup.bot(forum, true), &recorder, &followed, &setup.role).await?;
        recordings.push(("forum", recorder));
    }
    let hooks = [
        ("webhook", &setup.webhook, false),
        ("webhook-forum", &setup.forum_webhook, true),
    ];
    for (name, url, forum) in hooks {
        if let Some(url) = url {
            let recorder = Recorder::new(http.clone());
            post(&webhook(url, forum), &recorder, &followed, &setup.role).await?;
            recordings.push((name, recorder));
        }
    }
    Ok(recordings)
}

fn bot_id(recorded: &[(&str, Recorder<NativeHttp>)]) -> Result<String, String> {
    let (_, markers) = recorded
        .iter()
        .find(|(name, _)| *name == "markers")
        .ok_or("the markers were not recorded")?;
    let fixture = markers.fixture(&Scrub::default())?;
    fixture
        .exchanges
        .first()
        .and_then(|exchange| exchange.response.body.as_ref())
        .and_then(|body| body.get("id"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| "Discord did not say which user the bot is".to_owned())
}

fn scrub(setup: &Setup, bot: String) -> Scrub {
    let mut replace = vec![
        (setup.channel.clone(), TEST_CHANNEL.to_owned()),
        (setup.role.clone(), TEST_ROLE.to_owned()),
        (bot, TEST_BOT.to_owned()),
    ];
    replace.extend(
        setup
            .forum
            .iter()
            .map(|forum| (forum.clone(), TEST_FORUM.to_owned())),
    );
    Scrub {
        replace,
        ids: Some(Ids {
            digits: 17..=20,
            first: FIRST_ID,
        }),
        headers: vec!["content-type".to_owned(), "retry-after".to_owned()],
    }
}

async fn run() -> Result<Vec<PathBuf>, String> {
    let setup = Setup::from_env()?;
    let recorded = record(&setup).await?;
    let scrub = scrub(&setup, bot_id(&recorded)?);
    let fixtures: Vec<(&str, String)> = recorded
        .iter()
        .map(|(name, recorder)| Ok((*name, recorder.fixture(&scrub)?.to_json())))
        .collect::<Result<_, String>>()?;
    let directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../herald-dest-discord/fixtures");
    fixtures
        .into_iter()
        .map(|(name, json)| {
            let path = directory.join(format!("{name}.json"));
            fs::write(&path, json).map_err(|error| format!("{}: {error}", path.display()))?;
            Ok(path)
        })
        .collect()
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(written) => {
            for path in written {
                println!("wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("the recording stopped: {error}");
            ExitCode::FAILURE
        }
    }
}
