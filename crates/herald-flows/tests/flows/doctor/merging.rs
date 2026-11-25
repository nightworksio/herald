//! What doctor says of each merge queue: where it merges, through whose
//! queue, how, its update style, its write credential and where its steps
//! run; and a gap for each reason one does not start.

use super::*;

const FORGEJO: &str = "[sources.codeberg]\nkind = \"forgejo\"\nrepository = \"acme/app\"\napi = \"https://codeberg.org/api/v1\"\n";

#[test]
fn a_queue_that_can_start_is_said_and_has_no_gap() {
    let config = on(
        "git-branch",
        "[merge_queues.main]\nsource = \"gate\"\nbranch = \"main\"\nmethod = \"squash\"\nwrite = { token = { secret = \"HERALD_MERGE\" } }\n",
    );
    let findings = doctored(&config, &layer(&STORES));
    assert_eq!(keys(&findings.gaps), Vec::<&str>::new());
    assert!(
        findings.notes.contains(&"The merge queue main merges into main of nightworksio/php-mutation-gate on github through the forge's merge queue, with method squash, a gesture vouching for the head at the gesture's time on the forge, a push stamped up to 5s before it counted as after it, a branch brought up to date by merge, the write credential HERALD_MERGE, held apart, its steps taken by `herald merge-queue --apply`, a pull request it handed over taken out of the forge's queue once its gesture's maker no longer holds its least role. It reads with its source's read credential: a GitHub App or fine-grained token with Metadata, Contents, Pull requests, Checks and Commit statuses read on the repository (and on a fork's, for its pushes), on GitHub Enterprise Server and GHE.com too; a classic token needs repo for a private repository.".to_owned()),
        "{:?}",
        findings.notes
    );
    let staying = on(
        "git-branch",
        "[merge_queues.main]\nsource = \"gate\"\nbranch = \"main\"\nmethod = \"squash\"\nrole_lost = \"stay\"\nwrite = { token = { secret = \"HERALD_MERGE\" } }\n",
    );
    let findings = doctored(&staying, &layer(&STORES));
    assert!(
        findings.notes.iter().any(|note| note.contains(", a pull request it handed over left in the forge's queue once its gesture's maker no longer holds its least role. It reads")),
        "{:?}",
        findings.notes
    );
    let served = on(
        "git-branch",
        &format!(
            "{FORGEJO}[merge_queues.main]\nsource = \"codeberg\"\nbranch = \"main\"\nruns = \"herald\"\nvouches = \"approved\"\nskew = \"1m\"\nmethod = \"fast-forward\"\napplied_by = \"server\"\nwrite = {{ token = {{ secret = \"HERALD_MERGE\" }} }}\n"
        ),
    );
    let findings = doctored(&served, &layer(&STORES));
    assert_eq!(keys(&findings.gaps), Vec::<&str>::new());
    assert!(
        findings.notes.contains(&"The merge queue main merges into main of acme/app on forgejo through herald's own queue, with method fast-forward, a gesture vouching for the head herald first reads with the gesture, once one holding its least role approved it, a push stamped up to 1m before it counted as after it, a branch brought up to date by merge, the write credential HERALD_MERGE, held apart, its steps taken by `herald server`, which then reads its write credential. It reads with its source's read credential: a token with read:repository and read:issue, of a user who administers the repository: Forgejo and Gitea say another person's role to an administrator alone.".to_owned()),
        "{:?}",
        findings.notes
    );
}

#[test]
fn a_queue_without_a_method_a_write_credential_a_store_that_keeps_records_or_a_forge_queue_does_not_start()
 {
    let config = on(
        "markers",
        &format!(
            "{FORGEJO}[merge_queues.main]\nsource = \"codeberg\"\nbranch = \"main\"\nvouches = \"seen\"\n"
        ),
    );
    let findings = doctored(&config, &layer(&STORES));
    let queued: Vec<&Gap> = findings
        .gaps
        .iter()
        .filter(|gap| gap.feature == "the merge queue \"main\"")
        .collect();
    assert_eq!(
        queued
            .iter()
            .map(|gap| (gap.key.as_str(), gap.why.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                "merge_queues.main.method",
                "a merge queue without a method does not start; give method = \"merge\", \"squash\", \"rebase\", \"fast-forward\" or \"repository\""
            ),
            (
                "merge_queues.main.write",
                "a merge queue without a write credential does not start; give write = { token = { secret = \"NAME\" } }, or on GitHub write = { app = \"<github_apps name>\" }"
            ),
            (
                "store.kind",
                "the markers store keeps no record to write before each step, so a merge queue on it does not start; choose a store that keeps records"
            ),
            (
                "merge_queues.main.runs",
                "Forgejo and Gitea have no merge queue, so with runs = \"forge\" the queue does not start; give runs = \"herald\" to run herald's own"
            ),
        ]
    );
    assert!(
        findings.notes.iter().any(|note| note.starts_with("The merge queue main merges into main of acme/app on forgejo through the forge's merge queue, with no method, a gesture vouching for the head herald first reads with the gesture, a push stamped up to 5s before it counted as after it, a branch brought up to date by merge, no write credential,")),
        "{:?}",
        findings.notes
    );
}
