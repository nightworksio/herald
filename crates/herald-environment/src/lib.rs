//! The ports any target with a process implements: secrets from its
//! environment and the files it names, and the time from its clock.

use std::collections::BTreeMap;
use std::fmt;
use std::future::{Future, ready};
use std::path::{Path, PathBuf};

use herald_ports::{Clock, Secret, SecretError, Secrets};
use jiff::Timestamp;

/// The variable naming a directory holding a file per secret, each named as
/// its secret is.
pub const SECRETS_DIR: &str = "HERALD_SECRETS_DIR";

/// What a variable naming the file one secret is in ends with.
pub const FILE_SUFFIX: &str = "_FILE";

/// Secrets from the environment, read once when it is made, and from the
/// files it names, read each time one is asked for (ADR-0007): a secret is a
/// variable, a file `<NAME>_FILE` names, or a file named as it is in
/// [`SECRETS_DIR`]. Its `Debug` names the variables it holds, and shows none
/// of their values.
#[derive(Clone, Default)]
pub struct EnvSecrets {
    values: BTreeMap<String, String>,
}

impl fmt::Debug for EnvSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvSecrets")
            .field("names", &self.values.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl EnvSecrets {
    /// The secrets in this process's environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_vars(std::env::vars())
    }

    /// The secrets `vars` holds, as `(name, value)`.
    #[must_use]
    pub fn from_vars(vars: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            values: vars.into_iter().collect(),
        }
    }
}

impl EnvSecrets {
    fn set(&self, name: &str) -> Option<&String> {
        self.values.get(name).filter(|value| !value.is_empty())
    }

    fn read(&self, name: &str) -> Result<Secret, SecretError> {
        let given = self.set(name);
        let named = self.set(&format!("{name}{FILE_SUFFIX}")).map(PathBuf::from);
        let held = self
            .set(SECRETS_DIR)
            .filter(|_| filed(name))
            .map(|dir| Path::new(dir).join(name))
            .filter(|path| path.is_file());
        let ways = [given.is_some(), named.is_some(), held.is_some()];
        if ways.into_iter().filter(|way| *way).count() > 1 {
            return Err(SecretError::Clashing {
                name: name.to_owned(),
            });
        }
        let value = match (given, named.or(held)) {
            (Some(value), _) => Some(value.clone()),
            (None, Some(path)) => file(name, &path)?,
            (None, None) => None,
        };
        value
            .map(|value| Secret::new(name, value))
            .ok_or_else(|| SecretError::Missing {
                name: name.to_owned(),
            })
    }
}

/// Whether `name` names a file in the secrets directory itself, and not one
/// under or above it.
fn filed(name: &str) -> bool {
    !name.contains(['/', '\\'])
}

/// The secret in the file at `path`, less one line ending, where it holds
/// one.
fn file(name: &str, path: &Path) -> Result<Option<String>, SecretError> {
    let text = std::fs::read_to_string(path).map_err(|error| SecretError::Unreadable {
        name: name.to_owned(),
        path: path.display().to_string(),
        reason: error.kind().to_string(),
    })?;
    let value = text.strip_suffix('\n').map_or(text.as_str(), |line| {
        line.strip_suffix('\r').unwrap_or(line)
    });
    Ok(Some(value.to_owned()).filter(|value| !value.is_empty()))
}

impl Secrets for EnvSecrets {
    fn secret(&self, name: &str) -> impl Future<Output = Result<Secret, SecretError>> + Send {
        ready(self.read(name))
    }
}

/// The system's clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_names_the_variables_and_shows_no_value() {
        let secrets = EnvSecrets::from_vars([("TOKEN".to_owned(), "hunter2".to_owned())]);
        assert_eq!(format!("{secrets:?}"), "EnvSecrets { names: [\"TOKEN\"] }");
    }

    #[test]
    fn a_secret_set_and_not_empty_is_read_and_any_other_is_missing() {
        let secrets = EnvSecrets::from_vars([
            ("TOKEN".to_owned(), "hunter2".to_owned()),
            ("EMPTY".to_owned(), String::new()),
        ]);
        let secret = pollster::block_on(secrets.secret("TOKEN")).unwrap();
        assert_eq!(secret.expose(), "hunter2");
        for name in ["EMPTY", "ABSENT"] {
            assert_eq!(
                pollster::block_on(secrets.secret(name)).unwrap_err(),
                SecretError::Missing {
                    name: name.to_owned()
                }
            );
        }
    }

    struct Dir(PathBuf);

    impl Dir {
        fn new(test: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("herald-secrets-{test}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn with(self, name: &str, text: &str) -> Self {
            std::fs::write(self.0.join(name), text).unwrap();
            self
        }

        fn path(&self, name: &str) -> String {
            self.0.join(name).display().to_string()
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn vars(pairs: &[(&str, &str)]) -> EnvSecrets {
        EnvSecrets::from_vars(
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
        )
    }

    fn read(secrets: &EnvSecrets, name: &str) -> Result<String, SecretError> {
        pollster::block_on(secrets.secret(name)).map(|secret| secret.expose().to_owned())
    }

    #[test]
    fn a_secret_is_read_from_the_file_its_file_variable_names_less_one_line_ending() {
        let dir = Dir::new("named")
            .with("lf", "hunter2\n")
            .with("crlf", "hunter2\r\n")
            .with("two", "hunter2\n\n")
            .with("empty", "\n");
        let lf = dir.path("lf");
        let crlf = dir.path("crlf");
        let two = dir.path("two");
        let empty = dir.path("empty");
        let secrets = vars(&[
            ("LF_FILE", &lf),
            ("CRLF_FILE", &crlf),
            ("TWO_FILE", &two),
            ("EMPTY_FILE", &empty),
            ("UNSET_FILE", ""),
        ]);
        assert_eq!(read(&secrets, "LF").unwrap(), "hunter2");
        assert_eq!(read(&secrets, "CRLF").unwrap(), "hunter2");
        assert_eq!(read(&secrets, "TWO").unwrap(), "hunter2\n");
        for name in ["EMPTY", "UNSET"] {
            assert_eq!(
                read(&secrets, name).unwrap_err(),
                SecretError::Missing {
                    name: name.to_owned()
                }
            );
        }
    }

    #[test]
    fn a_secret_file_that_cannot_be_read_is_named_with_why() {
        let dir = Dir::new("unreadable");
        let path = dir.path("absent");
        let secrets = vars(&[("TOKEN_FILE", &path)]);
        assert_eq!(
            read(&secrets, "TOKEN").unwrap_err(),
            SecretError::Unreadable {
                name: "TOKEN".to_owned(),
                path,
                reason: "entity not found".to_owned(),
            }
        );
    }

    #[test]
    fn a_secret_is_read_from_its_file_in_the_secrets_directory() {
        let dir = Dir::new("held").with("TOKEN", "hunter2\n");
        let held = dir.0.display().to_string();
        let secrets = vars(&[(SECRETS_DIR, &held)]);
        assert_eq!(read(&secrets, "TOKEN").unwrap(), "hunter2");
        for name in [
            "ABSENT",
            "",
            ".",
            "..",
            "../TOKEN",
            "sub/TOKEN",
            "sub\\TOKEN",
        ] {
            assert_eq!(
                read(&secrets, name).unwrap_err(),
                SecretError::Missing {
                    name: name.to_owned()
                },
                "{name}"
            );
        }
    }

    #[test]
    fn a_secret_named_outside_the_secrets_directory_is_not_read() {
        let outer = Dir::new("outside").with("TOKEN", "above\n");
        let held = outer.0.join("held");
        std::fs::create_dir_all(held.join("sub")).unwrap();
        std::fs::write(held.join("sub").join("TOKEN"), "below\n").unwrap();
        let secrets = vars(&[(SECRETS_DIR, &held.display().to_string())]);
        for name in ["../TOKEN", "sub/TOKEN", "sub\\TOKEN"] {
            assert_eq!(
                read(&secrets, name).unwrap_err(),
                SecretError::Missing {
                    name: name.to_owned()
                },
                "{name}"
            );
        }
    }

    #[test]
    fn a_secret_given_two_ways_is_refused() {
        let dir = Dir::new("clash").with("TOKEN", "a").with("named", "b");
        let held = dir.0.display().to_string();
        let named = dir.path("named");
        let clashing = SecretError::Clashing {
            name: "TOKEN".to_owned(),
        };
        for pairs in [
            vec![("TOKEN", "c"), ("TOKEN_FILE", named.as_str())],
            vec![("TOKEN", "c"), (SECRETS_DIR, held.as_str())],
            vec![("TOKEN_FILE", named.as_str()), (SECRETS_DIR, held.as_str())],
        ] {
            assert_eq!(read(&vars(&pairs), "TOKEN").unwrap_err(), clashing);
        }
        let empty_beside_file = vars(&[("TOKEN", ""), ("TOKEN_FILE", named.as_str())]);
        assert_eq!(read(&empty_beside_file, "TOKEN").unwrap(), "b");
    }

    #[test]
    fn the_environment_s_secrets_are_this_process_s_variables() {
        let secret = pollster::block_on(EnvSecrets::from_env().secret("CARGO_MANIFEST_DIR"));
        assert_eq!(secret.unwrap().expose(), env!("CARGO_MANIFEST_DIR"));
    }

    #[test]
    fn the_system_clock_reads_the_time() {
        assert!(SystemClock.now() > Timestamp::constant(1_793_448_000, 0));
    }
}
