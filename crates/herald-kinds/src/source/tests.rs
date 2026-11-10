use std::future::ready;

use herald_core::{Cursor, Kind, Polled, SecretRef};
use herald_ports::{Boxed, DynHttp, Secret};

use super::*;

/// Options of a kind that keeps every default.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plain {
    token: Option<SecretRef>,
}

/// A source that answers nothing new.
#[derive(Debug)]
struct Quiet(Option<Secret>, String);

impl Poll for Quiet {
    fn api(&self) -> &str {
        &self.1
    }

    fn remembered(&self) -> usize {
        self.0.as_ref().map_or(1, |_| 2)
    }

    fn polled<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Option<&'a Cursor>,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>> {
        Box::pin(ready(Ok(None)))
    }
}

impl SourceConfig for Plain {
    fn kind(&self) -> &'static str {
        "plain"
    }

    fn token(&self) -> Option<&SecretRef> {
        self.token.as_ref()
    }

    fn kinds(&self) -> &'static [Kind] {
        &[Kind::RELEASE]
    }

    fn backfill(&self) -> usize {
        3
    }

    fn poller(&self, token: Option<Secret>) -> Arc<dyn Poll> {
        Arc::new(Quiet(token, "https://plain.example".to_owned()))
    }
}

fn tokened() -> Plain {
    Plain {
        token: Some(SecretRef {
            secret: "TOKEN".to_owned(),
        }),
    }
}

#[test]
fn a_source_takes_no_webhook_and_no_moving_tag_where_its_kind_keeps_the_defaults() {
    let source = Source::new(tokened());
    assert_eq!(source.webhook(), None);
    assert_eq!(source.scheme(), None);
    assert!(!source.moving_tags());
    assert_eq!(
        source.secret_refs(),
        vec![&SecretRef {
            secret: "TOKEN".to_owned()
        }]
    );
    assert_eq!(source.kind(), "plain");
    assert_eq!(source.kinds(), &[Kind::RELEASE]);
    assert_eq!(source.backfill(), 3);
    assert_eq!(source.hooked(&[], b"{}"), Ok(Hooked::Nothing(UNHOOKED)));
}

#[test]
fn a_source_polls_with_the_token_it_is_given() {
    let source = Source::new(tokened());
    let poll = source.poller(Some(Secret::new("TOKEN", "t")));
    assert_eq!(poll.api(), "https://plain.example");
    assert_eq!(poll.remembered(), 2);
    assert_eq!(source.poller(None).remembered(), 1);
    let http = crate::reach::tests::Silent;
    assert_eq!(pollster::block_on(poll.polled(&http, None)), Ok(None));
}

#[test]
fn a_source_is_read_as_its_own_kind_and_equal_only_to_the_same_options() {
    let source = Source::new(tokened());
    assert_eq!(source.of::<Plain>(), Some(&tokened()));
    assert_eq!(source, source.clone());
    assert_ne!(source, Source::new(Plain { token: None }));
    assert_eq!(format!("{source:?}"), format!("{:?}", tokened()));
}

#[test]
fn a_poll_that_finds_nothing_to_read_says_why() {
    assert_eq!(
        Unpolled::unreachable("down"),
        Unpolled::Unreachable("down".to_owned())
    );
    assert_eq!(
        Unpolled::unreadable("garbled"),
        Unpolled::Unreadable("garbled".to_owned())
    );
}
