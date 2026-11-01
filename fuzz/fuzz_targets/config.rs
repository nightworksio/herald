//! Whatever text a herald.toml holds, it loads as a config or as mistakes,
//! each mistake placed in the text, in file order, and never more than the
//! text has lines.

#![no_main]

use herald_config::load;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let files = |name: &str| Err(format!("{name} is not in the fuzzed text"));
    if let Err(errors) = load("herald.toml", text, &files) {
        assert!(!errors.errors.is_empty(), "no mistake was named");
        let lines = text.lines().count().max(1) + 1;
        let mut previous = (0, 0);
        for error in &errors.errors {
            assert!(error.line >= 1 && error.line <= lines, "{error:?} is outside {lines} lines");
            assert!(error.column >= 1, "{error:?}");
            assert!((error.line, error.column) >= previous, "{error:?} is out of file order");
            previous = (error.line, error.column);
        }
    }
});
