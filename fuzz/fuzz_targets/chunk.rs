//! Whatever text arrives, every part of its chunking fits its room.

#![no_main]

use herald_core::{Dialect, Measure, Room, chunk, size};
use libfuzzer_sys::fuzz_target;

const DIALECTS: [Dialect; 4] = [Dialect::Discord, Dialect::Slack, Dialect::Matrix, Dialect::Plain];

fuzz_target!(|input: (u8, u8, &str)| {
    let (first, rest, text) = input;
    let room = Room { measure: Measure::Chars, first: usize::from(first) + 64, rest: usize::from(rest) + 64 };
    for dialect in DIALECTS {
        for (index, part) in chunk(text, dialect, room).iter().enumerate() {
            let limit = if index == 0 { room.first } else { room.rest };
            assert!(size(part, room.measure) <= limit, "{dialect:?} part {index}: {:?}", part.body);
        }
    }
});
