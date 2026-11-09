#![allow(
    clippy::unused_async_trait_impl,
    reason = "a fake answers at once, and is written as the async trait it fakes"
)]

use herald_core::{Command, DestinationName, Measure, Place, Replies, Wording};
use herald_ports::{Method, Secrets, Text as Url};

use super::*;
use crate::same;

/// A destination that takes commands and keeps every default.
#[derive(Debug, PartialEq, Eq)]
struct Plain(&'static str);

impl Interacts for Plain {
    fn api(&self) -> &str {
        self.0
    }

    fn answering(&self) -> Answering {
        Answering {
            commands: vec![Command::Latest],
            replies: Replies::default(),
            prereleases: false,
            template: None,
            wording: Wording::default(),
        }
    }

    fn room(&self) -> (Dialect, Room) {
        (
            Dialect::Plain,
            Room {
                measure: Measure::Chars,
                first: 10,
                rest: 10,
            },
        )
    }

    fn answered(&self, replying: &Replying, parts: &[Text], _public: bool) -> Vec<Request> {
        replying
            .of::<&'static str>()
            .map(|place| {
                parts
                    .iter()
                    .map(|_| Request::new(Method::Post, Url::plain(*place)))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn took(&self, response: &Response) -> Result<(), String> {
        (response.status == 200)
            .then_some(())
            .ok_or_else(|| format!("{} answered {}", self.0, response.status))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

struct Named;

impl Secrets for Named {
    async fn secret(&self, name: &str) -> Result<Secret, SecretError> {
        if name == "MISSING" {
            return Err(SecretError::Missing {
                name: name.to_owned(),
            });
        }
        Ok(Secret::new(name, "value"))
    }
}

fn response(status: u16) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: Vec::new(),
    }
}

#[test]
fn an_interacting_destination_is_its_kind_equal_only_to_the_same_and_keeps_every_default() {
    let interacted = Interacted::new(Plain("https://plain.example"));
    assert_eq!(interacted, interacted.clone());
    assert_eq!(interacted, Interacted::new(Plain("https://plain.example")));
    assert_ne!(interacted, Interacted::new(Plain("https://other.example")));
    assert_eq!(
        interacted.of::<Plain>(),
        Some(&Plain("https://plain.example"))
    );
    assert_eq!(
        format!("{interacted:?}"),
        "Plain(\"https://plain.example\")"
    );
    assert_eq!(interacted.api(), "https://plain.example");
    assert_eq!(interacted.sessions(), None);
    assert!(interacted.answering().takes(Command::Latest));
    assert_eq!(interacted.room().0, Dialect::Plain);
    assert_eq!(interacted.took(&response(200)), Ok(()));
    assert_eq!(
        interacted.took(&response(500)),
        Err("https://plain.example answered 500".to_owned())
    );
}

#[test]
fn where_an_answer_goes_is_its_kinds_own_and_equal_only_to_the_same_place() {
    let replying = Replying::new("https://answers.example");
    assert_eq!(replying, replying.clone());
    assert_eq!(replying, Replying::new("https://answers.example"));
    assert_ne!(replying, Replying::new("https://elsewhere.example"));
    assert_ne!(replying, Replying::new(7_u8));
    assert_eq!(replying.of::<&str>(), Some(&"https://answers.example"));
    assert_eq!(replying.of::<u8>(), None);
    assert_eq!(format!("{replying:?}"), "\"https://answers.example\"");
    let interacted = Interacted::new(Plain("https://plain.example"));
    let parts = [Text {
        body: "said".to_owned(),
        html: None,
    }];
    assert_eq!(interacted.answered(&replying, &parts, false).len(), 1);
    assert_eq!(interacted.answered(&Replying::new(7_u8), &parts, false), []);
    let asked = Asked {
        destination: DestinationName::new("plain"),
        place: Place::new("here"),
        user: "member".to_owned(),
        command: Command::Latest,
        project: None,
        share: false,
        toggles: false,
        member: None,
    };
    assert_eq!(interacted.deferred(&replying, &asked), None);
}

#[test]
fn an_optional_secret_is_read_only_where_one_is_named() {
    let named = |secret: &str| SecretRef {
        secret: secret.to_owned(),
    };
    assert_eq!(
        pollster::block_on(optional(&Named, Some(&named("HERALD_KEY")))),
        Ok(Some(Secret::new("HERALD_KEY", "value")))
    );
    assert_eq!(pollster::block_on(optional(&Named, None)), Ok(None));
    assert_eq!(
        pollster::block_on(optional(&Named, Some(&named("MISSING")))),
        Err(SecretError::Missing {
            name: "MISSING".to_owned()
        })
    );
}
