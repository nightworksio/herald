//! A dry run's requests printed: what `herald_flows::Recording` kept.

use std::io::Write;

use herald_flows::Recording;

use super::output::say;

/// Each request `dry` kept, printed as the requests a first announcement
/// sends.
pub(super) fn print(dry: &Recording, out: &mut dyn Write) {
    say(
        out,
        "A dry run sends nothing and keeps no record: these are the requests a first announcement sends.",
    );
    for request in dry.taken() {
        say(out, "");
        for line in request.with_body().to_string().lines() {
            say(out, line);
        }
    }
    say(out, "");
}
