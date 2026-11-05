//! Whatever an app sends as its event, the app source answers an event or
//! why not, and an event it reads holds the app's identity, a project and a
//! title.

#![no_main]

use herald_sources_app::event;
use jiff::Timestamp;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if let Ok(read) = event("ci", bytes, Timestamp::UNIX_EPOCH) {
        assert!(read.identity().as_str().starts_with("app/ci/"));
        assert!(!read.project().is_empty());
        assert!(!read.title().is_empty());
    }
});
