use std::fmt;

/// One mistake in the config: where it is, the key it is under, and what to
/// write instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// The line, counted from one.
    pub line: usize,
    /// The column, counted from one, in characters.
    pub column: usize,
    /// The key's path, such as `routes[0].destination`; empty for the file.
    pub path: String,
    /// What is wrong, and what to write instead.
    pub message: String,
}

/// Every mistake in one config file, in the order they appear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Errors {
    /// The file, as it was named.
    pub file: String,
    /// The mistakes.
    pub errors: Vec<ConfigError>,
}

impl fmt::Display for Errors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.errors.iter().enumerate() {
            if index > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{}:{}:{} ", self.file, error.line, error.column)?;
            if !error.path.is_empty() {
                write!(f, "{}: ", error.path)?;
            }
            f.write_str(&error.message)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_one_a_line_with_their_place_and_key() {
        let errors = Errors {
            file: "herald.toml".to_owned(),
            errors: vec![
                ConfigError {
                    line: 17,
                    column: 15,
                    path: "routes[0].destination".to_owned(),
                    message: "no destination is named \"x\"".to_owned(),
                },
                ConfigError {
                    line: 1,
                    column: 1,
                    path: String::new(),
                    message: "bad".to_owned(),
                },
            ],
        };
        assert_eq!(
            errors.to_string(),
            "herald.toml:17:15 routes[0].destination: no destination is named \"x\"\nherald.toml:1:1 bad"
        );
    }
}
