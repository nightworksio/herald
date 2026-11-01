use std::collections::BTreeMap;

use herald_ports::{Secret, SecretError, Secrets};

/// Secrets from a map.
#[derive(Debug, Default)]
pub struct FakeSecrets {
    values: BTreeMap<String, String>,
}

impl FakeSecrets {
    /// The secrets with `name` set to `value`.
    #[must_use]
    pub fn with(mut self, name: &str, value: &str) -> Self {
        self.values.insert(name.to_owned(), value.to_owned());
        self
    }
}

impl Secrets for FakeSecrets {
    async fn secret(&self, name: &str) -> Result<Secret, SecretError> {
        self.values
            .get(name)
            .map(|value| Secret::new(name, value.clone()))
            .ok_or_else(|| SecretError::Missing {
                name: name.to_owned(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_set_is_read_and_one_not_set_is_missing() {
        let secrets = FakeSecrets::default().with("TOKEN", "hunter2");
        let secret = pollster::block_on(secrets.secret("TOKEN")).unwrap();
        assert_eq!(secret.expose(), "hunter2");
        assert_eq!(
            pollster::block_on(secrets.secret("OTHER")),
            Err(SecretError::Missing {
                name: "OTHER".to_owned()
            })
        );
    }
}
