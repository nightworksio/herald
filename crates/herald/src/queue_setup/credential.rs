//! The write credential of a merge queue asked: the name of the secret
//! its token is kept under, never the token, or on GitHub an App.

use std::num::NonZeroU64;

use toml_edit::{Item, TableLike, Value};

use super::table::{Keys, inline};
use super::{Answers, Sourced, asking_of, merged, question, was};
use crate::asking::{Asking, NONE};
use herald_config::Config;
use herald_core::merging::Write;

/// Why a secret's name was refused, never repeating what was typed.
const UNNAMED_SECRET: &str = "answer the name the platform keeps the token under, never the token: capital letters, digits and '_', not starting with a digit, at most 64; what was typed is not repeated";

/// The longest secret's name taken.
const SECRET_NAME_MOST: usize = 64;

/// What a write credential is, as the wizard asks for it.
const WRITE_KINDS: [&str; 2] = ["token", "app"];

/// `answer` as a secret's name, refused without repeating it.
fn secret_name(answer: &str) -> Result<String, String> {
    let named = answer.len() <= SECRET_NAME_MOST
        && answer
            .bytes()
            .next()
            .is_some_and(|first| !first.is_ascii_digit())
        && answer
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
    named
        .then(|| answer.to_owned())
        .ok_or_else(|| UNNAMED_SECRET.to_owned())
}

/// The write credential asked, by a secret's name or an App's, and added
/// to `keys`; the secret's name, where it is one.
pub(super) async fn written(
    (config, sourced, repository): (&Config, &Sourced<'_>, Option<&str>),
    (was_here, asked): (Option<&dyn TableLike>, &Answers),
    asking: &mut Asking<'_>,
    keys: &mut Keys,
) -> Result<Option<String>, String> {
    let (source, merging) = (sourced.source, &sourced.merging);
    let apps = config.github_apps.keys().cloned().collect::<Vec<_>>();
    let an_app = Write {
        token: None,
        app: apps.first().cloned(),
        installation: None,
    };
    let offers_app = !apps.is_empty()
        && merged(source, &asking_of(repository, None, false, Some(&an_app))).is_ok();
    let kind_given = asked
        .write_token
        .as_ref()
        .map(|_| WRITE_KINDS[0])
        .or_else(|| asked.write_app.as_ref().map(|_| WRITE_KINDS[1]));
    let was_app = was(was_here, &["write", "app"]);
    let offered = if offers_app {
        &WRITE_KINDS[..]
    } else {
        &WRITE_KINDS[..1]
    };
    let kind_default = if was_app.is_some() {
        WRITE_KINDS[1]
    } else {
        WRITE_KINDS[0]
    };
    let kind_asked = format!("Its write credential ({})", offered.join(", "));
    let kind = if offered.len() == 1 && kind_given.is_none() {
        WRITE_KINDS[0].to_owned()
    } else {
        asking
            .answer(
                question(&kind_asked, "write-token", kind_given, Some(kind_default.to_owned())),
                |answer| {
                    offered
                        .contains(&answer)
                        .then(|| answer.to_owned())
                        .ok_or_else(|| {
                            "a GitHub App writes on a GitHub source with [github_apps] alone; answer token".to_owned()
                        })
                },
            )
            .await?
    };
    if kind == WRITE_KINDS[0] {
        let default = was(was_here, &["write", "token", "secret"]).unwrap_or_else(|| {
            format!("HERALD_{}_MERGE_TOKEN", merging.forge.to_ascii_uppercase())
        });
        let name = asking
            .answer(
                question(
                    "The name of the secret its write token is kept under",
                    "write-token",
                    asked.write_token.as_deref(),
                    Some(default),
                ),
                secret_name,
            )
            .await?;
        let secret = inline(vec![("secret", Value::from(name.as_str()))]);
        keys.push(("write", Item::Value(inline(vec![("token", secret)]))));
        return Ok(Some(name));
    }
    by_app((&apps, was_app), (was_here, asked), asking, keys).await
}

/// The GitHub App among `apps` that writes asked, and its installation,
/// added to `keys`.
async fn by_app(
    (apps, was_app): (&[String], Option<String>),
    (was_here, asked): (Option<&dyn TableLike>, &Answers),
    asking: &mut Asking<'_>,
    keys: &mut Keys,
) -> Result<Option<String>, String> {
    let listed_apps = apps.join(", ");
    let app = asking
        .answer(
            question(
                &format!("The GitHub App its writes are made as ({listed_apps})"),
                "write-app",
                asked.write_app.as_deref(),
                was_app.or_else(|| apps.first().filter(|_| apps.len() == 1).cloned()),
            ),
            |answer| {
                apps.iter()
                    .find(|app| *app == answer)
                    .cloned()
                    .ok_or_else(|| format!("answer one of {listed_apps}"))
            },
        )
        .await?;
    let installation = asking
        .answer(
            question(
                "The id of the App's installation, or none to look it up",
                "installation",
                asked.installation.as_deref(),
                Some(was(was_here, &["write", "installation"]).unwrap_or_else(|| NONE.to_owned())),
            ),
            |answer| match answer {
                NONE => Ok(None),
                _ => answer
                    .parse::<NonZeroU64>()
                    .map(Some)
                    .map_err(|_| "answer the installation's id, a number, or none".to_owned()),
            },
        )
        .await?;
    let mut pairs = vec![("app", Value::from(app.as_str()))];
    if let Some(installation) = installation {
        pairs.push((
            "installation",
            Value::from(i64::try_from(installation.get()).unwrap_or(i64::MAX)),
        ));
    }
    keys.push(("write", Item::Value(inline(pairs))));
    Ok(None)
}
