use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::route::SecretRef;

/// `user` and `secret` as HTTP Basic credentials take them: the pair
/// joined by a colon, in base64 (RFC 7617).
#[must_use]
pub fn basic(user: &str, secret: &str) -> String {
    STANDARD.encode(format!("{user}:{secret}"))
}

/// Whether a `platform` destination given `token` and `webhook` posts as a
/// bot, or else through the webhook.
///
/// # Errors
///
/// When it names neither, or both.
pub fn bot_or_webhook(
    platform: &str,
    token: Option<&SecretRef>,
    webhook: Option<&SecretRef>,
) -> Result<bool, String> {
    match (token, webhook) {
        (Some(_), None) => Ok(true),
        (None, Some(_)) => Ok(false),
        (None, None) => Err(format!(
            "a {platform} destination needs a token or a webhook; give one"
        )),
        (Some(_), Some(_)) => Err(format!(
            "a {platform} destination takes a token or a webhook, not both; keep one"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_destination_posts_as_a_bot_or_through_a_webhook_never_both() {
        let secret = SecretRef {
            secret: "S".to_owned(),
        };
        let given = Some(&secret);
        assert_eq!(bot_or_webhook("Slack", given, None), Ok(true));
        assert_eq!(bot_or_webhook("Slack", None, given), Ok(false));
        assert_eq!(
            bot_or_webhook("Slack", None, None),
            Err("a Slack destination needs a token or a webhook; give one".to_owned())
        );
        assert_eq!(
            bot_or_webhook("Slack", given, given),
            Err("a Slack destination takes a token or a webhook, not both; keep one".to_owned())
        );
    }

    #[test]
    fn a_pair_is_joined_by_a_colon_in_base64() {
        assert_eq!(basic("token", "hunter2"), "dG9rZW46aHVudGVyMg==");
        assert_eq!(
            basic("Aladdin", "open sesame"),
            "QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
    }
}
