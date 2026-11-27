use herald_kinds::{Answer, Ask, FirstRun, Lands, Variant, Written};
use toml_edit::{InlineTable, Table, Value};

const ROUTE: &str = "[[routes]]\nid = \"first\"\nsources = [\"app\"]\nkinds = [\"release\"]\ndestination = \"chat\"\ntemplate = \"release\"\n";
const REST: &str = "[sources.app]\nkind = \"github\"\nrepository = \"owner/app\"\n\n[templates.release]\ntitle = \"{{ event.project }} {{ event.attributes.version }}\"\nbody = \"{{ event.body }}\"\n";

fn first_runs() -> Vec<FirstRun> {
    herald_compose::kinds().first_runs()
}

fn answered(ask: &Ask) -> String {
    ask.answer.default().unwrap_or(ask.example).to_owned()
}

fn config(kind: &str, variant: &Variant) -> String {
    let answers: Vec<String> = variant.asks.iter().map(answered).collect();
    let mut destination = Table::new();
    destination.insert("kind", toml_edit::value(kind));
    let mut route = Table::new();
    for key in variant.written(&answers) {
        let value: Value = match key.value {
            Written::Secret(name) => {
                let mut secret = InlineTable::new();
                secret.insert("secret", name.into());
                secret.into()
            }
            Written::Text(text) => text.into(),
        };
        match key.lands {
            Lands::Destination => destination.insert(key.key, toml_edit::value(value)),
            Lands::Route => route.insert(key.key, toml_edit::value(value)),
        };
    }
    format!(
        "version = 1\n\n[store]\nkind = \"markers\"\n\n[destinations.chat]\n{destination}\n{ROUTE}{route}\n{REST}"
    )
}

#[test]
fn discord_mastodon_matrix_and_slack_are_offered_first() {
    let kinds: Vec<&str> = first_runs().iter().map(|run| run.kind).collect();
    assert_eq!(kinds, ["discord", "mastodon", "matrix", "slack"]);
}

#[test]
fn every_variant_s_answers_load_and_read_back_as_it_says() {
    for run in first_runs() {
        assert!(!run.described.is_empty(), "{}", run.kind);
        assert!(!run.variants.is_empty(), "{}", run.kind);
        for variant in run.variants {
            let text = config(run.kind, variant);
            let loaded = herald_config::load(
                "herald.toml",
                &text,
                &|_| Err(String::new()),
                &herald_compose::kinds(),
            )
            .unwrap_or_else(|errors| panic!("{} {}: {errors}\n{text}", run.kind, variant.name));
            assert_eq!(
                loaded.routes[0].profile.features.reads_back, variant.reads_back,
                "{} {}",
                run.kind, variant.name
            );
        }
    }
}

#[test]
fn every_question_s_example_is_an_answer_it_takes() {
    for run in first_runs() {
        for variant in run.variants {
            for ask in variant.asks {
                let taken = match ask.answer {
                    Answer::Choice { .. } => answered(ask),
                    Answer::Secret { .. } | Answer::Text => ask.example.to_owned(),
                };
                assert_eq!(
                    ask.answer.checked(&taken),
                    Ok(()),
                    "{} {}",
                    run.kind,
                    ask.key
                );
            }
        }
    }
}
