use std::collections::BTreeMap;
use std::path::Path;

use herald_crypto::RingSigner;
use herald_crypto::keygen::Bits;
use herald_ports::{Algorithm, Secret, Signer as _};

use crate::keygen::keygen;
use crate::*;

/// What `herald activitypub keygen` with `extra` printed and ended with.
fn generated(file: &Path, extra: &[&str]) -> (u8, String, String) {
    let mut args = vec![
        "herald".to_owned(),
        "--config".to_owned(),
        "/nonexistent/herald.toml".to_owned(),
        "activitypub".to_owned(),
        "keygen".to_owned(),
        file.display().to_string(),
    ];
    args.extend(extra.iter().map(|arg| (*arg).to_owned()));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = pollster::block_on(crate::run(
        &args,
        &BTreeMap::new(),
        &Connector::new(&|_, _| Err(String::new()), &|_, _| Err(String::new())),
        &mut out,
        &mut err,
    ));
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// The public key herald's signer reads from the private key in `file`, as
/// an actor publishes it.
fn public_of(file: &Path) -> String {
    let private = std::fs::read_to_string(file).unwrap();
    let secret = Secret::new("KEY", private.as_str());
    let der =
        pollster::block_on(RingSigner::default().public(Algorithm::RsaSha256, &secret)).unwrap();
    herald_core::public_pem(&der)
}

fn private_lines(file: &Path) -> Vec<String> {
    std::fs::read_to_string(file)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(unix)]
fn mode(file: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(file).unwrap().permissions().mode() & 0o777
}

#[test]
fn keygen_writes_a_key_its_owner_alone_reads_and_prints_only_its_public_key() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("actor.pem");
    let (code, out, err) = generated(&file, &[]);
    assert_eq!((code, err.as_str()), (DONE, ""), "{out}");
    let private = std::fs::read_to_string(&file).unwrap();
    assert!(herald_core::der::private_key(&private).is_some());
    assert!(out.starts_with(&public_of(&file)), "{out}");
    assert!(
        out.contains("The 2048-bit private key is in ") && out.contains("HERALD_ACTIVITYPUB_KEY"),
        "{out}"
    );
    assert!(!out.contains("PRIVATE KEY"), "{out}");
    for line in private_lines(&file) {
        assert!(!out.contains(&line), "{out}");
    }
    #[cfg(unix)]
    assert_eq!(mode(&file), 0o600);
    let left: Vec<_> = std::fs::read_dir(directory.path()).unwrap().collect();
    assert_eq!(left.len(), 1, "no temporary file is left");
}

#[test]
fn keygen_refuses_a_file_that_exists_unless_forced_and_then_replaces_it_whole() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("actor.pem");
    std::fs::write(&file, "kept").unwrap();
    let (code, out, err) = generated(&file, &[]);
    assert_eq!(code, STOPPED);
    assert_eq!(out, "");
    assert_eq!(
        err,
        format!(
            "{} exists; give --force to replace it, or name another file\n",
            file.display()
        )
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "kept");
    let (code, out, _) = generated(&file, &["--force", "--bits", "3072"]);
    assert_eq!(code, DONE);
    assert!(out.starts_with(&public_of(&file)), "{out}");
    assert!(out.contains("The 3072-bit private key"), "{out}");
    #[cfg(unix)]
    assert_eq!(mode(&file), 0o600);
}

#[test]
fn keygen_says_why_where_no_key_could_be_made_or_written_and_leaves_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("actor.pem");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        keygen(&file, (Bits::Rsa2048, false), None, &mut out, &mut err),
        STOPPED
    );
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "the system's random generator failed, so no key was made\n"
    );
    assert!(!file.exists());
    let missing = directory.path().join("absent").join("actor.pem");
    let mut err = Vec::new();
    assert_eq!(
        keygen(
            &missing,
            (Bits::Rsa2048, false),
            herald_native::random_secret(),
            &mut Vec::new(),
            &mut err
        ),
        STOPPED
    );
    let err = String::from_utf8(err).unwrap();
    assert!(
        err.starts_with(&format!("{} could not be written: ", missing.display())),
        "{err}"
    );
    let raced = directory.path().join("raced.pem");
    std::fs::write(&raced, "kept").unwrap();
    assert_eq!(
        crate::keygen::written(
            &raced,
            "pem",
            false,
            &herald_native::random_secret().unwrap()
        ),
        Err(format!(
            "{} exists; give --force to replace it, or name another file",
            raced.display()
        ))
    );
    assert_eq!(std::fs::read_to_string(&raced).unwrap(), "kept");
    let left: Vec<_> = std::fs::read_dir(directory.path()).unwrap().collect();
    assert_eq!(left.len(), 1, "only the file raced for is there");
}

#[test]
fn each_size_the_command_takes_is_the_size_made() {
    use crate::cli::KeyBits;
    assert_eq!(
        [KeyBits::B2048, KeyBits::B3072, KeyBits::B4096].map(Bits::from),
        Bits::ALL
    );
}
