//! Whatever text arrives, no markup's conversion of it mentions anyone or
//! links anywhere it does not show, and its literal form is only its letters.

#![no_main]

use herald_core::{Escape, Markup, Text, convert, literal};
use libfuzzer_sys::fuzz_target;

#[path = "../../crates/herald-core/src/markup/inert.rs"]
mod inert;

const MARKUPS: [Markup; 4] = [
    herald_dest_discord::MARKUP,
    herald_dest_slack::MARKUP,
    herald_dest_matrix::MARKUP,
    herald_dest_fediverse::MASTODON_MARKUP,
];

fuzz_target!(|text: &str| {
    for markup in MARKUPS {
        let converted: Text = convert(text, markup);
        if let Err(problem) = inert::inert(&converted, markup) {
            panic!("{markup:?}: {problem}");
        }
        if let Err(problem) = inert::literal_inert(&literal(text, markup), markup) {
            panic!("{markup:?} literal: {problem}");
        }
    }
});
