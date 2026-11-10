//! Whatever an app sends as its event, the app source answers an event or
//! why not, and an event it reads holds the app's identity, a project, a
//! title and the attributes its kind's events carry.

#![no_main]

use herald_core::KindSpecs;
use herald_sources_app::event;
use jiff::Timestamp;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let kinds = KindSpecs::default();
    if let Ok(read) = event("ci", bytes, Timestamp::UNIX_EPOCH, &kinds) {
        assert!(read.identity().as_str().starts_with("app/ci/"));
        assert!(!read.project().is_empty());
        assert!(!read.title().is_empty());
        let spec = kinds.get(read.kind()).expect("a known kind");
        assert_eq!(spec.check(read.attributes()), Ok(()));
    }
});
