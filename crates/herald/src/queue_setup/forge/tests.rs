use herald_core::Visibility;
use herald_kinds::{Branched, Queued};

use super::*;

fn said(queue: Option<bool>, unhanded: Option<&str>) -> BranchSaid {
    BranchSaid {
        branched: Branched {
            visibility: Visibility::Public,
            head: "b1".to_owned(),
            required: Vec::new(),
            queue,
            queued: Queued {
                unhanded: unhanded.map(ToOwned::to_owned),
                ..Queued::default()
            },
            whole: true,
        },
        methods: None,
    }
}

#[test]
fn each_thing_the_forge_says_of_its_queue_is_a_line() {
    let unsaid = "The forge does not say which methods the repository allows.";
    assert_eq!(
        said_lines(&said(Some(false), None), "main"),
        ["The forge has no merge queue on main.", unsaid]
    );
    assert_eq!(
        said_lines(&said(Some(true), Some("no enqueuePullRequest")), "main"),
        [
            "The forge has a merge queue on main.",
            "no enqueuePullRequest.",
            unsaid
        ]
    );
    assert_eq!(
        said_lines(&said(None, None), "main"),
        [
            "The forge does not say whether main has a merge queue.",
            unsaid
        ]
    );
}
