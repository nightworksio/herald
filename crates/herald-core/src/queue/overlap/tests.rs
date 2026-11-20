use super::*;
use crate::queue::tests::pull;

fn changing(number: u64, (repository, base): (&str, Option<&str>), paths: &[&str]) -> Pull {
    let mut pull = pull();
    pull.number = number;
    pull.repository = repository.to_owned();
    pull.base = base.map(ToOwned::to_owned);
    pull.files = Some(Changed {
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        whole: true,
    });
    pull
}

#[test]
fn pull_requests_of_one_repository_and_branch_changing_a_path_in_common_overlap() {
    let unread = Pull {
        files: None,
        ..changing(6, ("o/app", Some("main")), &[])
    };
    let pulls = [
        changing(3, ("o/app", Some("main")), &["src/lib.rs", "README.md"]),
        changing(1, ("o/app", Some("main")), &["src/lib.rs"]),
        changing(2, ("o/app", Some("main")), &["docs/a.md"]),
        changing(4, ("o/app", Some("next")), &["src/lib.rs"]),
        changing(5, ("o/other", Some("main")), &["src/lib.rs"]),
        changing(7, ("o/app", None), &["src/lib.rs"]),
        changing(8, ("o/app", Some("main")), &["README.md", "src/lib.rs"]),
        unread,
    ];
    let shared = overlaps(&pulls);
    let of = |number| {
        shared
            .get(&("o/app".to_owned(), number))
            .cloned()
            .unwrap_or_default()
    };
    assert_eq!(of(1), [3, 8]);
    assert_eq!(of(3), [1, 8]);
    assert_eq!(of(8), [1, 3]);
    assert_eq!(of(2), Vec::<u64>::new());
    assert_eq!(of(4), Vec::<u64>::new());
    assert_eq!(of(6), Vec::<u64>::new());
    assert_eq!(of(7), Vec::<u64>::new());
    assert_eq!(shared.len(), 3);
}
