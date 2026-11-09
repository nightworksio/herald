use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

/// `user` and `secret` as HTTP Basic credentials take them: the pair
/// joined by a colon, in base64 (RFC 7617).
#[must_use]
pub fn basic(user: &str, secret: &str) -> String {
    STANDARD.encode(format!("{user}:{secret}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_is_joined_by_a_colon_in_base64() {
        assert_eq!(basic("token", "hunter2"), "dG9rZW46aHVudGVyMg==");
        assert_eq!(
            basic("Aladdin", "open sesame"),
            "QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
    }
}
