//! Whatever text arrives, every part of its chunking fits its room.

#![no_main]

use herald_core::{Markup, Measure, Room, chunk, size};
use libfuzzer_sys::fuzz_target;

const MARKUPS: [Markup; 4] = [
    herald_dest_discord::MARKUP,
    herald_dest_slack::MARKUP,
    herald_dest_matrix::MARKUP,
    herald_dest_fediverse::MASTODON_MARKUP,
];

fuzz_target!(|input: (u8, u8, &str)| {
    let (first, rest, text) = input;
    let room = Room {
        measure: Measure::Chars,
        first: usize::from(first) + 64,
        rest: usize::from(rest) + 64,
    };
    for markup in MARKUPS {
        for (index, part) in chunk(text, markup, room).iter().enumerate() {
            let limit = if index == 0 { room.first } else { room.rest };
            assert!(
                size(part, room.measure) <= limit,
                "{markup:?} part {index}: {:?}",
                part.body
            );
        }
    }
});
