//! A first run's test post (ADR-0091 decision 7): the test message shown
//! as `herald preview` shows it, then, once the operator says so, posted
//! on the config's first route by `herald doctor --post`'s trial. Each
//! credential the post reads comes from the environment where its variable
//! is set, else from the terminal without showing it as it is typed; it is
//! held in memory for that one post, known to the scrubber of everything
//! the post prints, and never written or printed.

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::Composed;
use herald_config::{Config, Uses};
use herald_environment::SystemClock;
use herald_flows::trial_previewed;
use herald_ports::Clock as _;

use crate::asking::Asking;
use crate::connect::Connect;
use crate::doctoring::{Posting, code, gapped, posted};
use crate::output::{Scrubbing, say};
use crate::stores::Native;
use crate::{DONE, STOPPED};

/// What a first run's test post reads its credentials from and posts
/// through.
pub(crate) struct Trying<'a> {
    /// The environment, where a credential's variable may be set.
    pub(crate) vars: &'a BTreeMap<String, String>,
    /// What makes each connection.
    pub(crate) connect: Connect<'a>,
    /// A line read from the terminal without showing it as it is typed;
    /// none where no terminal is there to read it from.
    pub(crate) unechoed: Option<&'a dyn Fn() -> std::io::Result<String>>,
}

/// The test message of `config`, written to `path`, shown, then posted on
/// its first route where the operator says so, its credentials read as
/// `trying` reads them: the doctor's code, or done where nothing is posted.
pub(crate) async fn tried(
    config: &Config,
    path: &str,
    trying: &Trying<'_>,
    asking: &mut Asking<'_>,
) -> u8 {
    let Some(first) = config.routes.first() else {
        asking.say("The config holds no route, so no test message is posted.");
        return DONE;
    };
    let secrets = config.secrets_used(Uses::Announces);
    let named = secrets.iter().copied().collect::<Vec<_>>().join(" and ");
    let command = format!("herald --config {path} doctor --online --post");
    let later = format!("{command} posts it once the environment sets {named}");
    let run = crate::place(trying.vars);
    asking.say("");
    asking.say("The test message:");
    crate::preview::rendered(
        &trial_previewed(config, first, (&run, SystemClock.now())),
        asking.out,
    );
    let asked = format!("Post it to {}?", first.route.destination.as_str());
    if asking.yes || !asking.confirmed(&asked).await {
        asking.say(&format!("Nothing was posted; {later}."));
        return DONE;
    }
    let mut vars = trying.vars.clone();
    for name in secrets {
        if crate::var(&vars, name).is_some() {
            continue;
        }
        let Some(unechoed) = trying.unechoed else {
            asking.say(&format!(
                "{name} is not set, and no terminal is there to type it on unseen, so nothing was posted; {later}."
            ));
            return DONE;
        };
        let _ = write!(asking.out, "{name}, not shown as it is typed: ");
        let _ = asking.out.flush();
        match unechoed() {
            Ok(typed) if !typed.is_empty() => {
                vars.insert(name.to_owned(), typed);
            }
            _ => {
                asking.say(&format!(
                    "Nothing was typed for {name}, so nothing was posted; {later}."
                ));
                return DONE;
            }
        }
    }
    posting(
        config,
        (&vars, &run),
        trying.connect,
        (asking.out, &command, &named),
    )
    .await
}

/// The test message posted in `run` on `config`'s first route alone,
/// through `connect`, with the credentials `vars` set, each line printed to `out`
/// scrubbed of every one of them; where it was not posted, the secrets
/// `named` to check and the `command` that posts it again.
async fn posting(
    config: &Config,
    (vars, run): (&BTreeMap<String, String>, &str),
    connect: Connect<'_>,
    (out, command, named): (&mut dyn Write, &str, &str),
) -> u8 {
    let (within, scrubber, _) = crate::guarded(config, vars, connect);
    let out = &mut Scrubbing::new(out, scrubber);
    let mut first = config.clone();
    first.routes.truncate(1);
    let (lines, code) =
        match Composed::dry(Native::new(vars, &within), first, Uses::Announces, run).await {
            Ok(composed) => {
                let findings = posted(config, &composed.run(Posting).await);
                let lines = findings
                    .notes
                    .iter()
                    .cloned()
                    .chain(findings.gaps.iter().map(gapped))
                    .collect::<Vec<_>>();
                (lines, code(&findings))
            }
            Err(why) => (
                vec![format!("No test message was posted, as {why}.")],
                STOPPED,
            ),
        };
    for line in &lines {
        say(out, line);
    }
    if code != DONE {
        say(
            out,
            &format!(
                "The files stay written; check {named} and what the chat answered above, then run {command} with {named} set."
            ),
        );
    }
    let _ = out.flush();
    code
}
