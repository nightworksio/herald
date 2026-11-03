//! What herald signs when it delivers an activity (ADR-0004, ADR-0007): the
//! mirror of [`crate::activity_claim`], as Mastodon verifies it.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha2::Digest as _;

use crate::inbound::Addressed;

/// The headers a delivery's signature covers, as Mastodon requires them.
pub const SIGNED: &str = "(request-target) host date digest content-type";
/// The media type an activity is sent and served as.
pub const ACTIVITY_JSON: &str = "application/activity+json";
/// The widest line of a PEM body.
const PEM_LINE: usize = 64;

/// `body`'s SHA-256, in base64, as `Digest: SHA-256=<it>` carries it.
#[must_use]
pub fn body_sha256(body: &[u8]) -> String {
    STANDARD.encode(sha2::Sha256::digest(body))
}

/// What a delivery of `body` to `addressed`, dated `date`, carries and
/// signs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signing {
    /// Its `Digest` header.
    pub digest: String,
    /// The string its signature is made over.
    pub string: String,
}

/// The digest and signing string of a delivery of `body` to `addressed`,
/// dated `date`, covering [`SIGNED`].
#[must_use]
pub fn signing(addressed: Addressed<'_>, date: &str, body: &[u8]) -> Signing {
    let digest = format!("SHA-256={}", body_sha256(body));
    let string = format!(
        "(request-target): {}\nhost: {}\ndate: {date}\ndigest: {digest}\ncontent-type: {ACTIVITY_JSON}",
        addressed.target, addressed.audience
    );
    Signing { digest, string }
}

/// The `Signature` header of `signature`, made by the key `key_id` over
/// [`SIGNED`].
#[must_use]
pub fn signature_header(key_id: &str, signature: &[u8]) -> String {
    format!(
        r#"keyId="{key_id}",algorithm="rsa-sha256",headers="{SIGNED}",signature="{}""#,
        STANDARD.encode(signature)
    )
}

/// The DER `SubjectPublicKeyInfo` `der` as PEM, as an actor's
/// `publicKeyPem` publishes it.
#[must_use]
pub fn public_pem(der: &[u8]) -> String {
    let body = STANDARD.encode(der);
    let lines: Vec<&str> = body
        .as_bytes()
        .chunks(PEM_LINE)
        .filter_map(|line| std::str::from_utf8(line).ok())
        .collect();
    format!(
        "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
        lines.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delivery_signs_its_target_host_date_digest_and_type_and_names_its_key() {
        let to = Addressed {
            target: "post /inbox",
            audience: "social.example",
        };
        let signed = signing(to, "Tue, 03 Nov 2026 08:00:00 GMT", b"{}");
        assert_eq!(
            signed.digest,
            "SHA-256=RBNvo1WzZ4oRRq0W9+hknpT7T8If536DEMBg9hyq/4o="
        );
        assert_eq!(
            signed.string,
            "(request-target): post /inbox\nhost: social.example\ndate: Tue, 03 Nov 2026 08:00:00 GMT\ndigest: SHA-256=RBNvo1WzZ4oRRq0W9+hknpT7T8If536DEMBg9hyq/4o=\ncontent-type: application/activity+json"
        );
        assert_eq!(
            signature_header("https://herald.example/users/a#main-key", b"sig"),
            r#"keyId="https://herald.example/users/a#main-key",algorithm="rsa-sha256",headers="(request-target) host date digest content-type",signature="c2ln""#
        );
    }

    #[test]
    fn a_public_key_is_written_in_lines_of_sixty_four() {
        let pem = public_pem(&[1; 70]);
        let lines: Vec<&str> = pem.lines().collect();
        assert_eq!(lines[0], "-----BEGIN PUBLIC KEY-----");
        assert_eq!(lines[1].len(), 64);
        assert_eq!(lines[2].len(), 32);
        assert_eq!(lines[3], "-----END PUBLIC KEY-----");
        assert!(pem.ends_with("-----\n"));
        assert_eq!(public_pem(&[]).lines().count(), 3);
    }
}
