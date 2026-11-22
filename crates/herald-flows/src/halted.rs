//! A read that stopped `herald queue` or `herald rerun`, said in full only
//! where the operator said the output is private, and otherwise by the
//! source it stopped at and the kind of stop alone (ADR-0069).

use herald_core::{Printed, SourceName};

use crate::Stop;

/// What a stop said in short ends with.
pub const UNSAID_FAILURE: &str = "--private, or [cli] private, prints why";

/// A read of `source` that stopped a command with `stop`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Halted {
    pub(crate) source: SourceName,
    pub(crate) stop: Stop,
}

impl Halted {
    /// The read of `source` that stopped with `stop`.
    pub(crate) fn of(source: &SourceName, stop: Stop) -> Self {
        Self {
            source: source.clone(),
            stop,
        }
    }

    /// The stop as an output `printed` says it: in full where it is said
    /// private, and else naming the source and the kind of stop alone,
    /// never a word of the stop's own text.
    pub(crate) fn said(self, printed: Printed) -> Stop {
        if printed.prints_failures() {
            return self.stop;
        }
        let source = self.source.as_str();
        let short = |kind: &str| {
            format!("the source \"{source}\" could not be read: {kind}; {UNSAID_FAILURE}")
        };
        match self.stop {
            Stop::RetryAfter(after) => Stop::RetryAfter(after),
            Stop::Refused(_) => Stop::Refused(short("a request was refused")),
            Stop::Unreachable(_) => Stop::Unreachable(short("its forge could not be reached")),
            Stop::Unread(_) => Stop::Unread(short("what was sent could not be read back")),
            Stop::Store(_) => Stop::Store(short("the store could not be read or written")),
            Stop::Template(_) => Stop::Template(short("a template could not render")),
            Stop::Marker(_) => Stop::Marker(short("a marker could not be computed")),
            Stop::Proof(_) => Stop::Proof(short("an envelope could not be proved")),
            Stop::Source(_) => Stop::Source(short("its forge's answer could not be read")),
            Stop::RateLimited(_) => Stop::RateLimited(short("its forge's rate limit was reached")),
            Stop::Limit(_) => Stop::Limit(short("the run's limit on time or requests was near")),
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::SignedDuration;

    use super::*;

    const NAMED: &str = "https://api.github.com/repos/acme/secret-plans/pulls 7041";

    #[test]
    fn a_stop_is_said_in_full_only_where_the_output_is_said_private() {
        let source = SourceName::new("work");
        let stops = [
            Stop::Refused(NAMED.to_owned()),
            Stop::Unreachable(NAMED.to_owned()),
            Stop::Unread(NAMED.to_owned()),
            Stop::Store(NAMED.to_owned()),
            Stop::Template(NAMED.to_owned()),
            Stop::Marker(NAMED.to_owned()),
            Stop::Proof(NAMED.to_owned()),
            Stop::Source(NAMED.to_owned()),
            Stop::RateLimited(NAMED.to_owned()),
            Stop::Limit(NAMED.to_owned()),
        ];
        let mut kinds = Vec::new();
        for stop in stops {
            for printed in [Printed::Configured, Printed::Asked] {
                assert_eq!(Halted::of(&source, stop.clone()).said(printed), stop);
            }
            let said = Halted::of(&source, stop.clone()).said(Printed::Withheld);
            assert_eq!(std::mem::discriminant(&said), std::mem::discriminant(&stop));
            let text = format!("{said:?}");
            assert!(!text.contains("acme") && !text.contains("7041"), "{text}");
            assert!(
                text.contains("the source \\\"work\\\" could not be read: "),
                "{text}"
            );
            assert!(text.contains(UNSAID_FAILURE), "{text}");
            kinds.push(text);
        }
        kinds.dedup();
        assert_eq!(kinds.len(), 10);
        assert!(kinds[8].ends_with(&format!(
            "could not be read: its forge's rate limit was reached; {UNSAID_FAILURE}\")"
        )));
        let after = Stop::RetryAfter(SignedDuration::from_secs(5));
        assert_eq!(
            Halted::of(&source, after.clone()).said(Printed::Withheld),
            after
        );
    }
}
