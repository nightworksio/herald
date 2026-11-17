//! A body's `Content-Digest` (RFC 9530), which an HTTP message signature
//! covers in place of the body.

use sha2::{Digest as _, Sha256, Sha512};

use crate::structured::{Bare, Member, dictionary, written};

/// The header a body's digest travels in.
pub const CONTENT_DIGEST: &str = "content-digest";
/// The algorithm herald digests with.
const SHA_256: &str = "sha-256";
/// The other algorithm herald checks a digest by.
const SHA_512: &str = "sha-512";

/// The `Content-Digest` value of `body`, by SHA-256.
#[must_use]
pub fn content_digest(body: &[u8]) -> String {
    format!(
        "{SHA_256}={}",
        written(&Bare::Bytes(Sha256::digest(body).to_vec()))
    )
}

/// Whether the `Content-Digest` value `field` names `body`: it holds at
/// most `most` bytes, a SHA-256 or SHA-512 digest, and every such digest it
/// holds is `body`'s.
#[must_use]
pub fn digest_names(field: &str, body: &[u8], most: usize) -> bool {
    if field.len() > most {
        return false;
    }
    let Some(members) = dictionary(field) else {
        return false;
    };
    let mut checked = false;
    for (algorithm, member) in members {
        let expected = match algorithm.as_str() {
            SHA_256 => Sha256::digest(body).to_vec(),
            SHA_512 => Sha512::digest(body).to_vec(),
            _ => continue,
        };
        let Member::Item(item) = member else {
            return false;
        };
        if item.bare != Bare::Bytes(expected) {
            return false;
        }
        checked = true;
    }
    checked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::FIELD_MOST;

    #[test]
    fn a_digest_longer_than_its_bound_is_not_read() {
        let body = b"body";
        let field = content_digest(body);
        assert!(digest_names(&field, body, field.len()));
        assert!(!digest_names(&field, body, field.len() - 1));
    }

    #[test]
    fn a_body_s_digest_is_its_sha_256_as_a_byte_sequence() {
        assert_eq!(
            content_digest(b"{\"hello\": \"world\"}\n"),
            "sha-256=:RK/0qy18MlBSVnWgjwz6lZEWjP/lF5HF9bvEF8FabDg=:"
        );
    }

    #[test]
    fn a_digest_names_the_body_it_was_made_of() {
        let body = b"{\"hello\": \"world\"}\n";
        assert!(digest_names(&content_digest(body), body, FIELD_MOST));
        assert!(digest_names(
            "sha-512=:YMAam51Jz/jOATT6/zvHrLVgOYTGFy1d6GJiOHTohq4yP+pgk4vf2aCsyRZOtw8MjkM7iw7yZ/WkppmM44T3qg==:",
            body,
            FIELD_MOST,
        ));
        assert!(digest_names(
            &format!("unixsum=:AAAA:, {}", content_digest(body)),
            body,
            FIELD_MOST,
        ));
    }

    #[test]
    fn a_digest_of_another_body_or_of_no_known_algorithm_names_nothing() {
        let body = b"body";
        for field in [
            content_digest(b"other"),
            format!("{}, sha-512=:AAAA:", content_digest(body)),
            "unixsum=:AAAA:".to_owned(),
            "sha-256=(:AAAA:)".to_owned(),
            "sha-256=\"text\"".to_owned(),
            "not a dictionary".to_owned(),
            String::new(),
        ] {
            assert!(!digest_names(&field, body, FIELD_MOST), "{field}");
        }
    }
}
