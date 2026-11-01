//! What every herald source shares (ADR-0001, ADR-0003): reading a bounded
//! JSON answer, checking an answer's status, escaping a URL's path segment,
//! naming a URL's host, and the error that names what was read.

mod error;

pub use error::SourceError;

use herald_ports::Response;
use serde::de::DeserializeOwned;

/// What a server answers to a conditional request when nothing changed.
pub const NOT_MODIFIED: u16 = 304;

/// The newest releases or versions one poll reads (ADR-0003).
pub const PAGE: usize = 30;

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
