use std::fmt::Write as _;

use herald_core::{DestinationName, Key, RouteId, Schedule, Scheme, SourceName, Subscription};

/// A name as one segment of a path: letters, digits, `.`, `_` and `-` kept,
/// and every other byte written as `~` and its two hex digits.
pub(crate) fn segment(name: &str) -> String {
    name.bytes().fold(String::new(), |mut segment, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            segment.push(char::from(byte));
        } else {
            let _ = write!(segment, "~{byte:02x}");
        }
        segment
    })
}

pub(crate) fn entry(key: &Key) -> String {
    format!("ledger/{key}.json")
}

pub(crate) fn cursor(source: &SourceName) -> String {
    format!("cursors/{}.json", segment(source.as_str()))
}

pub(crate) const SCHEDULES: &str = "schedules";

pub(crate) fn schedule(schedule: &Schedule) -> String {
    format!(
        "{SCHEDULES}/{}.{}.{:?}.json",
        schedule.key,
        segment(&schedule.thread),
        schedule.chore
    )
}

pub(crate) fn subscribers(destination: &DestinationName, project: &str) -> String {
    format!(
        "subscriptions/{}/{}",
        segment(destination.as_str()),
        segment(project)
    )
}

pub(crate) fn subscribed(subscription: &Subscription) -> String {
    format!(
        "{}/{}.json",
        subscribers(&subscription.destination, &subscription.project),
        segment(&subscription.subscriber)
    )
}

pub(crate) const DELIVERIES: &str = "deliveries";

pub(crate) fn delivery(scheme: Scheme, id: &str) -> String {
    format!("{DELIVERIES}/{}.{}.json", scheme.name(), segment(id))
}

pub(crate) fn pin(route: &RouteId) -> String {
    format!("pins/{}.json", segment(route.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_becomes_one_safe_segment() {
        assert_eq!(segment("php-mutation-gate_1.0"), "php-mutation-gate_1.0");
        assert_eq!(segment("a/b c~"), "a~2fb~20c~7e");
        assert_eq!(segment("é"), "~c3~a9");
    }
}
