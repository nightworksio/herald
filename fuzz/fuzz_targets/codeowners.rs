//! Whatever a CODEOWNERS file holds and whatever paths a pull request
//! changes, each forge's reading of it ends, names only lines the file
//! has, in order, and a file it refuses, a path past the most matched and
//! unread paths fall to no one.

#![no_main]

use herald_core::{
    Changed, Codeowners, Dialect, MOST_AREA_NAME, MOST_AREA_PATH, MOST_LINE_OWNERS, Owned,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let (file, paths) = text.split_once("\n\n\n").unwrap_or((text, ""));
    let changed = Changed {
        paths: paths.lines().map(str::to_owned).collect(),
        whole: true,
    };
    let long = Changed {
        paths: changed
            .paths
            .iter()
            .filter(|path| path.chars().count() > MOST_AREA_PATH)
            .cloned()
            .collect(),
        whole: true,
    };
    let lines = file.lines().count();
    for dialect in [Dialect::GitHub, Dialect::GitLab, Dialect::Forgejo] {
        let read = Codeowners::read(dialect, file);
        assert!(read.skipped().iter().all(|skipped| (1..=lines).contains(&skipped.line)));
        assert!(read.skipped().windows(2).all(|pair| pair[0].line < pair[1].line));
        let owned = read.owning(Some(&changed));
        if read.refused().is_some() {
            assert_eq!(owned, Owned::default());
        }
        assert!(owned.owners.len() <= lines * MOST_LINE_OWNERS);
        assert!(owned.owners.iter().all(|owner| owner.written().chars().count() <= MOST_AREA_NAME + 1));
        assert!(owned.sections.iter().all(|section| section.chars().count() <= MOST_AREA_NAME));
        if dialect != Dialect::GitLab {
            assert!(owned.sections.is_empty());
        }
        assert_eq!(read.owning(Some(&long)), Owned::default());
        assert_eq!(read.owning(None), Owned::default());
    }
});
