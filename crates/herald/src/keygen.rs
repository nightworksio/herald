//! `herald activitypub keygen`: a new RSA key for herald's ActivityPub
//! actors, written to a file its owner alone may read, with nothing of it
//! printed (ADR-0004, ADR-0102).

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use herald_core::public_pem;
use herald_crypto::keygen::{Bits, rsa_key};
use herald_ports::{Scrubber, Secret};

use crate::output::{Scrubbing, say};
use crate::{DONE, STOPPED};

/// Why no key was made where no temporary file could be named.
const UNRANDOM: &str = "the system's random generator failed, so no key was made";

/// The name the private key is scrubbed as, wherever a line might hold it.
const KEPT: &str = "activitypub keygen";

/// Who may read the written file, as this system lets herald say.
#[cfg(unix)]
const READERS: &str = "readable by its owner alone";

/// Who may read the written file, as this system lets herald say.
#[cfg(not(unix))]
const READERS: &str = "with the permissions its folder gives a new file, which herald does not narrow to its owner on this system: check who may read it";

/// The file `file` beside its own, named by `nonce`, that the key is
/// written to first.
fn beside(file: &Path, nonce: &str) -> PathBuf {
    let name = file
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    file.with_file_name(format!(".{name}.{nonce}.tmp"))
}

/// A new file at `path` that its owner alone may read and write, made
/// only where nothing is there.
fn created(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// `pem` written to `file` whole or not at all: first to a file of its own
/// beside it, then linked in where nothing is at `file`, or renamed over
/// it where `force` says so.
pub(crate) fn written(file: &Path, pem: &str, force: bool, nonce: &str) -> Result<(), String> {
    let temporary = beside(file, nonce);
    let shown = file.display();
    let made = created(&temporary)
        .and_then(|mut handle| {
            handle.write_all(pem.as_bytes())?;
            handle.sync_all()
        })
        .and_then(|()| {
            if force {
                std::fs::rename(&temporary, file)
            } else {
                std::fs::hard_link(&temporary, file)
            }
        });
    let _ = std::fs::remove_file(&temporary);
    made.map_err(|error| match error.kind() {
        ErrorKind::AlreadyExists => existing(file),
        _ => format!("{shown} could not be written: {error}"),
    })
}

/// Why a key is not written over the file `file`.
fn existing(file: &Path) -> String {
    format!(
        "{} exists; give --force to replace it, or name another file",
        file.display()
    )
}

/// A new RSA key of `bits` written to `file`, replaced where `force` says
/// so, its temporary file named by `nonce`; its public key printed to
/// `out`, and why where it could not be made or written to `err`.
pub(crate) fn keygen(
    file: &Path,
    (bits, force): (Bits, bool),
    nonce: Option<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let scrubber = Scrubber::new(herald_config::Log::SCRUB_FROM);
    let out = &mut Scrubbing::new(out, scrubber.clone());
    let err = &mut Scrubbing::new(err, scrubber.clone());
    if !force && file.exists() {
        say(err, &existing(file));
        return STOPPED;
    }
    let made = nonce
        .ok_or(UNRANDOM)
        .and_then(|nonce| rsa_key(bits).map(|made| (nonce, made)));
    let (nonce, made) = match made {
        Ok(made) => made,
        Err(why) => {
            say(err, why);
            return STOPPED;
        }
    };
    scrubber.keep(&Secret::new(KEPT, made.private.as_str()));
    if let Err(why) = written(file, &made.private, force, &nonce) {
        say(err, &why);
        return STOPPED;
    }
    let _ = write!(out, "{}", public_pem(&made.public));
    say(
        out,
        &format!(
            "The {}-bit private key is in {}, {READERS}. Store its contents as the secret your ActivityPub destination's key names, such as HERALD_ACTIVITYPUB_KEY, then delete the file; herald derives the public key above from it.",
            bits.bits(),
            file.display()
        ),
    );
    let _ = out.flush();
    let _ = err.flush();
    DONE
}
