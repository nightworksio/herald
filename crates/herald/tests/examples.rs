//! Every config the repository documents or ships holds to the published
//! schema, and the schema refuses what the loader refuses (ADR-0006).

#[cfg(test)]
mod examples {
    use std::path::{Path, PathBuf};

    use herald_config::schema;
    use serde_json::{Value, json};

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(root().join(path)).unwrap()
    }

    fn published() -> Value {
        serde_json::from_str(&schema(&herald_compose::kinds())).unwrap()
    }

    /// The schema with nothing required at the top, for a part of a config.
    fn partial() -> Value {
        let mut schema = published();
        schema.as_object_mut().unwrap().remove("required");
        schema
    }

    fn json(text: &str) -> Value {
        serde_json::to_value(toml::from_str::<toml::Table>(text).unwrap()).unwrap()
    }

    fn mistakes(schema: &Value, text: &str) -> Vec<String> {
        let validator = jsonschema::validator_for(schema).unwrap();
        validator
            .iter_errors(&json(text))
            .map(|error| format!("{}: {error}", error.instance_path()))
            .collect()
    }

    /// Each fenced TOML block of `markdown`, its indent taken off.
    fn blocks(markdown: &str) -> Vec<String> {
        let mut blocks = Vec::new();
        let mut open: Option<(usize, String)> = None;
        for line in markdown.lines() {
            let indent = line.len() - line.trim_start().len();
            match (&mut open, line.trim_start()) {
                (None, "```toml") => open = Some((indent, String::new())),
                (Some(_), "```") => blocks.extend(open.take().map(|(_, block)| block)),
                (Some((indent, block)), _) => {
                    block.push_str(line.get(*indent..).unwrap_or_default());
                    block.push('\n');
                }
                (None, _) => {}
            }
        }
        blocks
    }

    /// The config the Helm chart's values give, its indent taken off.
    fn charted(values: &str) -> String {
        values
            .lines()
            .skip_while(|line| *line != "config: |")
            .skip(1)
            .take_while(|line| line.is_empty() || line.starts_with("  "))
            .fold(String::new(), |mut config, line| {
                config.push_str(line.get(2..).unwrap_or_default());
                config.push('\n');
                config
            })
    }

    fn shipped() -> Vec<PathBuf> {
        let mut found = vec![PathBuf::from("herald.toml")];
        let mut pending = vec![PathBuf::from("deploy"), PathBuf::from(".github/dogfood")];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(root().join(&directory)).unwrap() {
                let entry = entry.unwrap();
                let path = directory.join(entry.file_name());
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().unwrap().is_dir() {
                    if name != "node_modules" {
                        pending.push(path);
                    }
                } else if path
                    .extension()
                    .is_some_and(|extension| extension == "toml")
                    && format!("\n{}", read(&path)).contains("\nversion = 1\n")
                {
                    found.push(path);
                }
            }
        }
        found.sort();
        found
    }

    #[test]
    fn every_shipped_config_holds_to_the_schema() {
        let schema = published();
        let shipped = shipped();
        assert!(shipped.len() >= 10, "{shipped:?}");
        for path in shipped {
            let text = format!("\n{}", read(&path));
            assert_eq!(mistakes(&schema, &text), Vec::<String>::new(), "{path:?}");
        }
        let chart = charted(&read(Path::new("deploy/helm/herald/values.yaml")));
        assert!(chart.contains("[store]"), "{chart}");
        assert_eq!(mistakes(&schema, &chart), Vec::<String>::new());
    }

    #[test]
    fn every_toml_block_of_the_readme_holds_to_the_schema() {
        let schema = partial();
        let blocks = blocks(&read(Path::new("README.md")));
        assert!(blocks.len() >= 6, "{blocks:?}");
        for block in blocks {
            assert_eq!(mistakes(&schema, &block), Vec::<String>::new(), "{block}");
        }
    }

    #[test]
    fn a_fenced_block_is_read_whatever_its_indent() {
        assert_eq!(
            blocks(
                "text\n  ```toml\n  a = 1\n\n  b = 2\n  ```\n```toml\nc = 3\n```\n```sh\nd\n```\n"
            ),
            ["a = 1\n\nb = 2\n", "c = 3\n"]
        );
        assert_eq!(
            charted("image: x\nconfig: |\n  version = 1\n\n  [store]\nserver:\n  jobs: 1\n"),
            "version = 1\n\n[store]\n"
        );
    }

    #[test]
    fn the_schema_refuses_what_the_loader_refuses() {
        let schema = partial();
        let refused = [
            "version = 2",
            "[server]\nsilence = \"+5m\"",
            "[server]\nsilence = \"0s\"",
            "[server]\nbody = \"0MiB\"",
            "[templates.t]\ntitle = \"t\"\nbody = \"b\"\nfile = \"f\"",
            "[templates.t]\ntitle = \"t\"",
            "[apps.ci]\nprojects = [\"herald\"]\ntokens = [\"sha256:…\"]",
            "[apps.ci]\nprojects = []",
            "[apps.ci]\ntokens = []",
            "[server]\nlisten = \"127.0.0.1:8080\"\nunknown = 1",
            "[render]\nfuel = 400001",
            "[server.inbound]\ncovered = 3",
        ];
        for text in refused {
            assert_ne!(mistakes(&schema, text), Vec::<String>::new(), "{text}");
        }
        let taken = [
            "version = 1",
            "[server]\nsilence = \"30s\"\naccept_pause = \"250ms\"",
            "[templates.t]\ntitle = \"t\"\nfile = \"t.md.j2\"",
            "[apps.ci]\nprojects = [\"herald\"]",
        ];
        for text in taken {
            assert_eq!(mistakes(&schema, text), Vec::<String>::new(), "{text}");
        }
    }

    #[test]
    fn the_schema_names_each_definition_once_and_allows_no_null() {
        let schema = published();
        let names: Vec<&String> = schema["$defs"].as_object().unwrap().keys().collect();
        assert!(
            names
                .iter()
                .all(|name| !name.ends_with(|last: char| last.is_ascii_digit())),
            "{names:?}"
        );
        assert!(!schema.to_string().contains("null"));
        assert_eq!(
            schema["$defs"]["Version"],
            json!({ "type": "integer", "const": 1 })
        );
    }
}
