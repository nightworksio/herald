use std::collections::BTreeMap;
use std::fmt;
use std::future::{Future, ready};

use herald_ports::{Clock, Secret, SecretError, Secrets};
use jiff::Timestamp;

/// Secrets from the environment, read once when it is made (ADR-0007). Its
/// `Debug` names the variables it holds, and shows none of their values.
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

impl Secrets for EnvSecrets {
    fn secret(&self, name: &str) -> impl Future<Output = Result<Secret, SecretError>> + Send {
        ready(
            self.values
                .get(name)
                .filter(|value| !value.is_empty())
                .map(|value| Secret::new(name, value.clone()))
                .ok_or_else(|| SecretError::Missing {
                    name: name.to_owned(),
                }),
        )
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
        let _ = EnvSecrets::from_env();
    }

    #[test]
    fn the_system_clock_reads_the_time() {
        assert!(SystemClock.now() > Timestamp::constant(1_793_448_000, 0));
    }
}
