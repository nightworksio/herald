//! herald's own queue records what the forge said of a step's head, and
//! mints a write App's token for each step with that step's permissions
//! alone, recording the step unsent where none could be minted and
//! saying why, never the credential.

use herald_core::merging::{StepKind, Stepped};
use herald_flows::{Acted, Budget, Next};
use serde_json::{Value, json};

use super::{OWN, PULL, answer, applied, github, own_pull, run_config, steps, the_secrets};
use crate::harness::healthy;

const INSTALLATION: &str = "GET https://api.github.com/repos/o/r/installation";
const TOKENS: &str = "POST https://api.github.com/app/installations/3/access_tokens";

/// The queue of [`OWN`] writing as the App `merger`.
fn as_app() -> String {
    let app = "[github_apps.merger]\nclient_id = \"Iv23liMerger\"\nprivate_key = { secret = \"HERALD_MERGER_KEY\" }\n";
    format!("{OWN}{app}").replace(
        "write = { token = { secret = \"HERALD_MERGE\" } }",
        "write = { app = \"merger\" }",
    )
}

/// A token GitHub minted holding `permissions` on `o/r`.
fn minted(permissions: &Value) -> Value {
    json!({ "token": "minted-value", "expires_at": "2099-01-01T00:00:00Z",
        "permissions": permissions, "repositories": [{ "name": "r" }] })
}

#[test]
fn a_merge_github_had_not_judged_clean_is_recorded_as_not_clean() {
    let store = healthy();
    let unjudged = answer(200, &own_pull("h7", (None, "clean"), false));
    let forge = github("h7", &[], 0).answering(PULL, &[unjudged]);
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took))]
    );
    assert!(!steps(&store)[0].3);
}

#[test]
fn each_step_mints_its_own_permissions_and_one_never_minted_is_recorded_unsent() {
    let held = the_secrets().with("HERALD_MERGER_KEY", "the App's key");
    let installed = answer(200, &json!({ "id": 3, "app_id": 40 }));
    for (behind, kind, permissions) in [
        (
            1,
            StepKind::Update,
            json!({ "contents": "write", "pull_requests": "write" }),
        ),
        (0, StepKind::Merge, json!({ "contents": "write" })),
    ] {
        let mut holding = permissions.clone();
        holding["metadata"] = json!("read");
        let forge = github("h7", &[], behind)
            .answering(INSTALLATION, std::slice::from_ref(&installed))
            .answering(TOKENS, &[answer(201, &minted(&holding))]);
        let store = healthy();
        assert_eq!(
            run_config(
                &as_app(),
                (&store, &forge, &held),
                (true, Budget::UNBOUNDED)
            ),
            [Next::Acted(Acted::Stepped(kind, Stepped::Took))]
        );
        assert_eq!(forge.bodies(TOKENS)[0]["permissions"], permissions);
        let mut wider = holding.clone();
        wider["administration"] = json!("write");
        let widened = github("h7", &[], behind)
            .answering(INSTALLATION, std::slice::from_ref(&installed))
            .answering(TOKENS, &[answer(201, &minted(&wider))]);
        for unminted in [github("h7", &[], behind), widened] {
            let store = healthy();
            let nexts = run_config(
                &as_app(),
                (&store, &unminted, &held),
                (true, Budget::UNBOUNDED),
            );
            assert!(
                matches!(&nexts[..], [Next::Acted(Acted::Unsent(unsent, why))]
                    if *unsent == kind
                        && why.starts_with("the write App's token could not be minted: ")
                        && !why.contains("minted-value")
                        && !why.contains("the App's key")),
                "{nexts:?}"
            );
            assert_eq!(steps(&store)[0].4, Stepped::Unsent);
        }
    }
}
