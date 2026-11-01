//! Whatever text arrives, no dialect's conversion of it mentions anyone or
//! links anywhere it does not show, and its literal form is only its letters.

#![no_main]

use herald_core::{Dialect, Text, convert, literal};
use libfuzzer_sys::fuzz_target;

#[path = "../../crates/herald-core/src/markup/inert.rs"]
mod inert;

const DIALECTS: [Dialect; 4] = [Dialect::Discord, Dialect::Slack, Dialect::Matrix, Dialect::Plain];

fuzz_target!(|text: &str| {
    for dialect in DIALECTS {
        let converted: Text = convert(text, dialect);
        if let Err(problem) = inert::inert(&converted, dialect) {
            panic!("{dialect:?}: {problem}");
        }
        if let Err(problem) = inert::literal_inert(&literal(text, dialect), dialect) {
            panic!("{dialect:?} literal: {problem}");
        }
    }
});
