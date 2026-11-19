//! The static signed log written at the end of a run that may have changed
//! a feed, its files also written to the directory `[federation.pages]
//! artifact` names (ADR-0060).

use std::io::Write;
use std::path::Path;

use herald_compose::told::why;
use herald_compose::{Composed, Host};
use herald_flows::Publishing;

use crate::output::say;
use crate::{DONE, STOPPED};

/// The static signed log written where it is due, said on `out`, and the
/// code it ends with: one where it was due and could not be written.
pub(crate) async fn logged<H: Host>(composed: &Composed<'_, H>, out: &mut dyn Write) -> u8 {
    let artifact = composed
        .config()
        .federation
        .pages
        .as_ref()
        .and_then(|pages| pages.artifact.as_deref());
    match composed.publish().await {
        Ok(None) => DONE,
        Ok(Some(Publishing::Current(sequence))) => {
            say(
                out,
                &format!("The static log stands at {sequence}; no feed changed."),
            );
            DONE
        }
        Ok(Some(Publishing::Written {
            sequence,
            files,
            changed,
        })) => {
            if let Some(directory) = artifact
                && let Err(why) = filled(Path::new(directory), &files)
            {
                say(
                    out,
                    &format!("The static log was not written to {directory}: {why}"),
                );
                return STOPPED;
            }
            say(
                out,
                &format!(
                    "The static log was written under {sequence}: {} files, {changed} written or removed on its branch.",
                    files.len()
                ),
            );
            DONE
        }
        Err(stop) => {
            say(
                out,
                &format!("The static log was not written: {}", why(&stop)),
            );
            STOPPED
        }
    }
}

/// `directory` filled with `files`, each by its path under it.
fn filled(directory: &Path, files: &[(String, String)]) -> Result<(), String> {
    for (path, body) in files {
        let file = directory.join(path);
        std::fs::create_dir_all(file.parent().unwrap_or(directory))
            .map_err(|error| error.to_string())?;
        std::fs::write(&file, body).map_err(|error| format!("{path}: {error}"))?;
    }
    Ok(())
}
