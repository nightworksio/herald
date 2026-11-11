#![allow(
    clippy::unused_async_trait_impl,
    reason = "a fake answers at once, and is written as the async trait it fakes"
)]

use std::future::ready;

use herald_core::{
    Bullet, Emphases, Escape, Features, Fences, Heading, Limits, Links, Markup, Measure,
    MentionStyle, Pins, Symbols, Threads,
};
use herald_ports::{Http, Request, Response};
use herald_testing::sample;

use super::*;

/// A destination of no kind: it answers each call with what it was given,
/// and keeps every default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plain(pub(crate) &'static str);

/// Plain text, as a destination with no markup writes it.
pub(crate) const PLAIN: Markup = Markup {
    escape: Escape::Neutral,
    links: Links::Spelled,
    heading: Heading::Bare,
    marks: Emphases {
        emphasis: "",
        strong: "",
        strike: "",
    },
    fences: Fences::None,
    bullet: Bullet::Symbol('-'),
    html: false,
    pills: None,
};

pub(crate) const PROFILE: Profile = Profile {
    markup: PLAIN,
    measure: Measure::Chars,
    limits: Limits {
        title: 1,
        text: 2,
        followup: 3,
        thread_name: 4,
    },
    features: Features {
        threads: Threads::None,
        crosspost: false,
        pins: Pins::None,
        reads_back: false,
        edits: false,
        mentions: MentionStyle::None,
        interactions: false,
    },
    symbols: Symbols::DEFAULT,
};

impl Reach for Plain {
    fn profile(&self) -> Profile {
        PROFILE
    }

    fn api(&self) -> &str {
        self.0
    }

    fn send<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        signing: Signing<'a>,
        step: &'a Step,
        _: &'a Announcement,
        _: &'a Delivery,
        _: &'a Entry,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(ready(Ok(Sent::Refused(format!(
            "{step:?} at {}",
            signing.now
        )))))
    }

    fn housekeep<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        chore: Chore,
        thread: &'a str,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(ready(Ok(Sent::Refused(format!("{chore:?} {thread}")))))
    }

    fn posts<'a>(&'a self, _: &'a dyn DynHttp) -> Box<dyn DynMarkers + 'a> {
        Box::new(Unread("plain reads nothing back"))
    }
}

/// Another kind, never the same as a [`Plain`].
#[derive(Debug, PartialEq)]
struct Other;

impl Reach for Other {
    fn profile(&self) -> Profile {
        PROFILE
    }

    fn api(&self) -> &'static str {
        ""
    }

    fn send<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Signing<'a>,
        _: &'a Step,
        _: &'a Announcement,
        _: &'a Delivery,
        _: &'a Entry,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(ready(Ok(Sent::Refused(String::new()))))
    }

    fn housekeep<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Chore,
        _: &'a str,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(ready(Ok(Sent::Refused(String::new()))))
    }

    fn posts<'a>(&'a self, _: &'a dyn DynHttp) -> Box<dyn DynMarkers + 'a> {
        Box::new(Landed)
    }
}

/// Posts that say every announcement landed, and open no thread.
struct Landed;

impl Markers for Landed {
    async fn entry(&self, key: &Key, _: &Marker) -> Result<Option<Entry>, MarkersError> {
        Ok(Some(Entry::new(*key)))
    }

    async fn threads(&self, _: &Marker) -> Result<Vec<MarkedThread>, MarkersError> {
        Ok(Vec::new())
    }
}

pub(crate) struct Silent;

impl Http for Silent {
    async fn send(&self, _: Request) -> Result<Response, HttpError> {
        Err(HttpError::TimedOut)
    }
}

struct Unsigned;

impl herald_ports::Signer for Unsigned {
    async fn sign(
        &self,
        _: herald_ports::Algorithm,
        _: &herald_ports::Secret,
        _: &[u8],
    ) -> Result<Vec<u8>, herald_ports::CryptoError> {
        Ok(Vec::new())
    }

    async fn public(
        &self,
        _: herald_ports::Algorithm,
        _: &herald_ports::Secret,
    ) -> Result<Vec<u8>, herald_ports::CryptoError> {
        Ok(Vec::new())
    }
}

#[test]
fn a_reached_destination_is_its_kind_equal_only_to_the_same_and_keeps_every_default() {
    let reached = Reached::new(Plain("https://plain.example"));
    assert_eq!(reached, reached.clone());
    assert_eq!(reached, Reached::new(Plain("https://plain.example")));
    assert_ne!(reached, Reached::new(Plain("https://other.example")));
    assert_ne!(reached, Reached::new(Other));
    assert_eq!(reached.of::<Plain>(), Some(&Plain("https://plain.example")));
    assert_eq!(reached.of::<Other>(), None);
    assert_eq!(format!("{reached:?}"), "Plain(\"https://plain.example\")");
    assert_eq!(reached.api(), "https://plain.example");
    assert_eq!(reached.profile(), PROFILE);
    let announcement = sample::announcement();
    assert_eq!(reached.follows(), None);
    assert_eq!(reached.published(&announcement), None);
    assert!(reached.admits("https://anyone.example/users/a"));
    assert!(reached.federates().is_none());
    let delivery = Delivery::default();
    assert_eq!(
        pollster::block_on(reached.probe(&Silent, &delivery)),
        Vec::new()
    );
}

#[test]
fn a_route_is_given_what_it_needs_where_its_platform_answers_with_success() {
    let answered = |status: u16| {
        Probe::route(
            "Plain",
            Some("channel"),
            &Ok(Response {
                status,
                headers: Vec::new(),
                body: Vec::new(),
            }),
            &|status| format!("withheld with {status}"),
        )
    };
    let withheld = |why: &str| {
        Some(Probe::Route {
            key: Some("channel"),
            why: why.to_owned(),
        })
    };
    assert_eq!(answered(200), None);
    assert_eq!(answered(401), withheld("withheld with 401"));
    assert_eq!(answered(403), withheld("withheld with 403"));
    assert_eq!(answered(404), withheld("withheld with 404"));
    assert_eq!(answered(500), withheld("Plain answered 500 when asked"));
    assert_eq!(
        Probe::route("Plain", Some("channel"), &Err(HttpError::TimedOut), &|_| {
            String::new()
        }),
        withheld("Plain could not be asked: the host did not answer in time")
    );
}

#[test]
fn a_credential_is_taken_where_its_platform_answers_with_success() {
    let answered = |status: u16| {
        Probe::credential(
            "Plain",
            &Ok(Response {
                status,
                headers: Vec::new(),
                body: Vec::new(),
            }),
        )
    };
    assert_eq!(answered(200), Probe::Taken);
    assert_eq!(answered(204), Probe::Taken);
    assert_eq!(
        answered(401),
        Probe::Credential("Plain refuses its credential, answering 401".to_owned())
    );
    assert_eq!(
        answered(403),
        Probe::Credential("Plain refuses its credential, answering 403".to_owned())
    );
    assert_eq!(
        answered(404),
        Probe::Credential(
            "Plain answered 404 when asked whether it takes its credential".to_owned()
        )
    );
    assert_eq!(
        Probe::credential("Plain", &Err(HttpError::TimedOut)),
        Probe::Credential("Plain could not be asked: the host did not answer in time".to_owned())
    );
}

#[test]
fn a_reached_destination_sends_and_keeps_house_through_the_ports_it_is_handed() {
    let reached = Reached::new(Plain("https://plain.example"));
    let announcement = sample::announcement();
    let signing = Signing {
        signer: &Unsigned,
        now: Timestamp::UNIX_EPOCH,
    };
    let sent = pollster::block_on(reached.send(
        &Silent,
        signing,
        &Step::Post,
        &announcement,
        &sample::delivery("role"),
        &Entry::new(announcement.key),
    ));
    assert_eq!(
        sent,
        Ok(Sent::Refused("Post at 1970-01-01T00:00:00Z".to_owned()))
    );
    assert_eq!(
        pollster::block_on(reached.housekeep(&Silent, Chore::Lock, "7")),
        Ok(Sent::Refused("Lock 7".to_owned()))
    );
    assert_eq!(
        pollster::block_on(Reached::new(Other).send(
            &Silent,
            signing,
            &Step::Post,
            &announcement,
            &sample::delivery("role"),
            &Entry::new(announcement.key),
        )),
        Ok(Sent::Refused(String::new()))
    );
    assert_eq!(
        pollster::block_on(Reached::new(Other).housekeep(&Silent, Chore::Lock, "7")),
        Ok(Sent::Refused(String::new()))
    );
    assert_eq!(Reached::new(Other).api(), "");
    assert_eq!(Reached::new(Other).profile(), PROFILE);
}

#[test]
fn a_destination_reads_back_what_landed_or_says_why_it_cannot() {
    let reached = Reached::new(Plain("https://plain.example"));
    let key = sample::announcement().key;
    let marker = Marker::of(&[1]);
    let unreadable = MarkersError::Unreadable {
        reason: "plain reads nothing back".to_owned(),
    };
    let posts = reached.read(&Silent);
    assert_eq!(format!("{posts:?}"), "Posts");
    assert_eq!(
        pollster::block_on(posts.entry(&key, &marker)),
        Err(unreadable.clone())
    );
    assert_eq!(pollster::block_on(posts.threads(&marker)), Err(unreadable));
    let other = Reached::new(Other);
    assert_eq!(
        pollster::block_on(other.read(&Silent).entry(&key, &marker)),
        Ok(Some(Entry::new(key)))
    );
    assert_eq!(
        pollster::block_on(other.read(&Silent).threads(&marker)),
        Ok(Vec::new())
    );
}
