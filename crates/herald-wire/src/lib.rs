//! What every herald source shares (ADR-0001, ADR-0003): reading a bounded
//! JSON answer, checking an answer's status, escaping a URL's path segment,
//! naming a URL's host, and the error that names what was read.

mod error;

pub use error::SourceError;

use std::num::NonZeroUsize;

use herald_core::Size;
use herald_ports::Response;
use serde::de::DeserializeOwned;

/// What a server answers to a conditional request when nothing changed.
pub const NOT_MODIFIED: u16 = 304;

/// The newest releases or versions one poll reads (ADR-0003).
pub const PAGE: usize = 30;

/// The most bytes of an answer a source reads and parses, where its answers
/// are not known to run larger (ADR-0003).
pub const PARSE_LIMIT: usize = 4_194_304;

/// How a source reads one poll: how many of the newest entries it asks
/// for, how many identities its cursor remembers, and the most of an answer
/// it parses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// How many of the newest entries one poll reads.
    pub page: usize,
    /// How many identities the cursor remembers, newest first.
    pub remembered: usize,
    /// The most bytes of an answer it parses.
    pub parse: usize,
}

impl Reading {
    /// How many identities a cursor remembers where the config says
    /// nothing.
    pub const REMEMBERED: usize = 500;

    /// What a source's options give, each where it is given, and the
    /// defaults otherwise, `parse` among them.
    #[must_use]
    pub fn of(
        page: Option<NonZeroUsize>,
        remembered: Option<NonZeroUsize>,
        parse: Option<Size>,
        default_parse: usize,
    ) -> Self {
        Self {
            page: page.map_or(PAGE, NonZeroUsize::get),
            remembered: remembered.map_or(Self::REMEMBERED, NonZeroUsize::get),
            parse: parse.map_or(default_parse, |parse| parse.0),
        }
    }

    /// Whether the reading is sound for a source whose answers parse
    /// `default_parse` bytes by default.
    ///
    /// # Errors
    ///
    /// When the cursor remembers fewer identities than a poll reads, so it
    /// could forget one it just read, or `parse` passes four times its
    /// default.
    pub fn check(&self, default_parse: usize) -> Result<(), String> {
        if self.remembered < self.page {
            return Err(format!(
                "the cursor remembers {} identities, fewer than the {} one poll reads; make remembered at least page",
                self.remembered, self.page
            ));
        }
        let most = default_parse.saturating_mul(4);
        if self.parse > most {
            return Err(format!(
                "make parse at most {}, four times its default",
                Size::written(most)
            ));
        }
        Ok(())
    }
}

/// `bytes` read as JSON, refused where they are more than `limit` bytes.
///
/// # Errors
///
/// When the bytes are too many, or are not the JSON `T` reads.
pub fn read<T: DeserializeOwned>(
    what: &'static str,
    limit: usize,
    bytes: &[u8],
) -> Result<T, SourceError> {
    if bytes.len() > limit {
        return Err(SourceError::TooLarge { what, limit });
    }
    serde_json::from_slice(bytes).map_err(|error| SourceError::Unreadable {
        what,
        reason: error.to_string(),
    })
}

/// Whether `response` is an answer, and not a refusal or a failure.
///
/// # Errors
///
/// When its status is not a success.
pub fn answered(what: &'static str, response: &Response) -> Result<(), SourceError> {
    if response.succeeded() {
        Ok(())
    } else {
        Err(SourceError::Answered {
            what,
            status: response.status,
        })
    }
}

/// `text` as one segment of a URL's path: every byte but a letter, a digit,
/// `.`, `_` and `-` percent-encoded.
#[must_use]
pub fn segment(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// The host of `url`, without its scheme, port or path.
#[must_use]
pub fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', ':', '?', '#']).next().unwrap_or(rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use herald_core::Problem;
    use proptest::prelude::*;

    fn answer(status: u16) -> Response {
        Response {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    #[test]
    fn a_reading_takes_what_is_given_and_the_defaults_otherwise() {
        assert_eq!(
            Reading::of(None, None, None, PARSE_LIMIT),
            Reading {
                page: 30,
                remembered: 500,
                parse: 4 << 20,
            }
        );
        assert_eq!(
            Reading::of(
                NonZeroUsize::new(5),
                NonZeroUsize::new(9),
                Some(Size(1 << 20)),
                PARSE_LIMIT
            ),
            Reading {
                page: 5,
                remembered: 9,
                parse: 1 << 20,
            }
        );
    }

    #[test]
    fn a_reading_that_forgets_what_a_poll_reads_or_parses_too_much_is_refused() {
        let reading = |page, remembered, parse| Reading {
            page,
            remembered,
            parse,
        };
        assert_eq!(reading(30, 30, 16 << 20).check(4 << 20), Ok(()));
        assert_eq!(
            reading(30, 29, 4 << 20).check(4 << 20),
            Err("the cursor remembers 29 identities, fewer than the 30 one poll reads; make remembered at least page".to_owned())
        );
        assert_eq!(
            reading(30, 500, (16 << 20) + 1).check(4 << 20),
            Err("make parse at most \"16MiB\", four times its default".to_owned())
        );
    }

    #[test]
    fn json_is_read_up_to_the_limit_and_refused_beyond_it() {
        assert_eq!(read::<Vec<u8>>("x", 3, b"[1]"), Ok(vec![1]));
        assert_eq!(
            read::<Vec<u8>>("x", 2, b"[1]"),
            Err(SourceError::TooLarge {
                what: "x",
                limit: 2
            })
        );
        assert!(matches!(
            read::<Vec<u8>>("x", 3, b"[1,"),
            Err(SourceError::Unreadable { what: "x", .. })
        ));
    }

    #[test]
    fn only_a_success_is_an_answer() {
        assert_eq!(answered("x", &answer(200)), Ok(()));
        assert_eq!(
            answered("x", &answer(404)),
            Err(SourceError::Answered {
                what: "x",
                status: 404
            })
        );
    }

    #[test]
    fn a_segment_escapes_what_a_path_would_read_otherwise() {
        assert_eq!(segment("v1.2.0_rc-1"), "v1.2.0_rc-1");
        assert_eq!(segment("group/sub project"), "group%2Fsub%20project");
        assert_eq!(segment("@scope/name"), "%40scope%2Fname");
    }

    #[test]
    fn a_host_is_named_without_its_scheme_port_or_path() {
        for (url, named) in [
            ("https://github.com/cli/cli", "github.com"),
            ("https://forge.example:8443/o/r", "forge.example"),
            ("https://registry.npmjs.org?x=1", "registry.npmjs.org"),
            ("crates.io/api/v1", "crates.io"),
            ("", ""),
        ] {
            assert_eq!(host(url), named, "{url}");
        }
    }

    #[test]
    fn each_error_reads_as_a_sentence() {
        let errors = [
            (
                SourceError::TooLarge {
                    what: "x",
                    limit: 2,
                },
                "x is larger than the 2 bytes herald reads",
            ),
            (
                SourceError::Unreadable {
                    what: "x",
                    reason: "no".to_owned(),
                },
                "x could not be read: no",
            ),
            (
                SourceError::Invalid {
                    what: "x",
                    problem: Problem::NoTitle,
                },
                "x makes no event: the event has no title",
            ),
            (
                SourceError::Answered {
                    what: "x",
                    status: 500,
                },
                "x was answered with 500",
            ),
        ];
        for (error, sentence) in errors {
            assert_eq!(error.to_string(), sentence);
        }
    }

    proptest! {
        #[test]
        fn a_segment_holds_no_byte_a_path_reads_as_a_separator(text in ".{0,24}") {
            let escaped = segment(&text);
            prop_assert!(escaped.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-%".contains(&byte)));
        }
    }
}
