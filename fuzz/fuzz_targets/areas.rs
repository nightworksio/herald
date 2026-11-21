//! Whatever areas a table names and whatever paths a pull request changes,
//! a table past its bounds is refused, and the areas a pull request falls
//! in are areas the table names, in name order, a path past the most
//! matched falling in none.

#![no_main]

use std::collections::BTreeMap;

use herald_core::{Areas, Changed, MOST_AREA_PATH, MOST_AREAS};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let (table, paths) = text.split_once("\n\n").unwrap_or((text, ""));
    let table: BTreeMap<String, Vec<String>> = table
        .lines()
        .map(|line| {
            let (name, globs) = line.split_once('=').unwrap_or((line, ""));
            let globs = globs.split(',').map(str::to_owned).collect();
            (name.to_owned(), globs)
        })
        .collect();
    let Ok(areas) = Areas::of(table) else {
        return;
    };
    let names: Vec<&str> = areas.names().collect();
    assert!(names.len() <= MOST_AREAS);
    let changed = Changed {
        paths: paths.lines().map(str::to_owned).collect(),
        whole: true,
    };
    let fallen = areas.of_changed(Some(&changed));
    assert!(fallen.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(fallen.iter().all(|area| names.contains(&area.as_str())));
    let long = Changed {
        paths: changed
            .paths
            .iter()
            .filter(|path| path.chars().count() > MOST_AREA_PATH)
            .cloned()
            .collect(),
        whole: true,
    };
    assert!(areas.of_changed(Some(&long)).is_empty());
});
