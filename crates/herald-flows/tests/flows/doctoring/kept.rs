//! What doctor says of each view reading kept: which sources, and how
//! often each is listed again; and of a view asking to read kept of a store
//! that keeps no pull requests (ADR-0061).

use herald_config::Config;
use herald_flows::Here;

use super::{loaded, on, server};

fn viewing(store: &str, view: &str) -> Config {
    loaded(&format!(
        r#"version = 1
[store]
{store}
[sources.hooked]
kind = "github"
repository = "o/hooked"
webhook = {{ secret = "HERALD_WEBHOOK" }}
[sources.quiet]
kind = "github"
repository = "o/quiet"
[views.queue]
{view}
"#
    ))
}

fn said(config: &Config) -> (Vec<String>, Vec<String>) {
    let findings = on(config, &server(), &Here::default());
    let viewed = |said: Vec<String>| {
        said.into_iter()
            .filter(|said| said.starts_with("The view"))
            .collect()
    };
    (viewed(findings.notes), viewed(findings.mistakes))
}

const READS: &str = "kept: from the records the store keeps, each webhook delivery's change folded in, its open pull requests listed again";

#[test]
fn doctor_names_each_source_a_view_reads_kept_and_how_often_it_is_listed() {
    assert_eq!(
        said(&viewing("kind = \"sqlite\"", "")),
        (
            vec![format!(
                "The view queue reads hooked {READS} every round, each page conditional on its last answer, and only those that changed read in full."
            )],
            Vec::new()
        )
    );
    assert_eq!(
        said(&viewing(
            "kind = \"sqlite\"",
            "mode = \"kept\"\nreconcile = \"1h\""
        ))
        .0,
        [
            format!(
                "The view queue reads hooked {READS} at most once every 1h, each page conditional on its last answer, and only those that changed read in full."
            ),
            format!(
                "The view queue reads quiet {READS} at most once every 1h, each page conditional on its last answer, and only those that changed read in full; the source takes no webhooks, so its records move only when it is listed."
            ),
        ]
    );
    assert_eq!(
        said(&viewing("kind = \"sqlite\"", "mode = \"live\"")),
        (Vec::new(), Vec::new())
    );
}

#[test]
fn a_view_reading_kept_of_a_store_keeping_no_pull_requests_is_a_mistake() {
    assert_eq!(
        said(&viewing("kind = \"markers\"", "mode = \"kept\"")),
        (
            Vec::new(),
            vec!["The view queue reads kept pull requests, and the store keeps none; choose a store that keeps them, or set its mode = \"live\".".to_owned()]
        )
    );
    assert_eq!(
        said(&viewing("kind = \"markers\"", "")),
        (Vec::new(), Vec::new())
    );
}
