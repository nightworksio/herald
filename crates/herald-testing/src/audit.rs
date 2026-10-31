//! What a recording must never hold: a credential, in any shape herald's
//! destinations and sources use.

const REDACTED: &str = "«secret:";

const PREFIXES: [(&str, &str); 13] = [
    ("xoxb-", "a Slack bot token"),
    ("xoxp-", "a Slack user token"),
    ("xoxa-", "a Slack app token"),
    ("xoxr-", "a Slack refresh token"),
    ("xoxs-", "a Slack session token"),
    ("xapp-", "a Slack app-level token"),
    ("ghp_", "a GitHub token"),
    ("gho_", "a GitHub token"),
    ("ghs_", "a GitHub token"),
    ("ghu_", "a GitHub token"),
    ("github_pat_", "a GitHub token"),
    ("syt_", "a Matrix access token"),
    ("glpat-", "a GitLab token"),
];

const SECRET_HEADERS: [&str; 6] = [
    "authorization",
    "cookie",
    "set-cookie",
    "x-gitlab-token",
    "x-slack-signature",
    "proxy-authorization",
];

/// Each credential `text` holds, by what it looks like.
#[must_use]
pub fn credentials(text: &str) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = PREFIXES
        .iter()
        .filter(|(prefix, _)| text.contains(prefix))
        .map(|(_, what)| *what)
        .collect();
    if webhook_token(text) {
        found.push("a webhook's token in its URL");
    }
    if text
        .split(|character: char| !(character.is_ascii_alphanumeric() || "._-".contains(character)))
        .any(discord_token)
    {
        found.push("a Discord bot token");
    }
    found
}

/// Whether a header named `name` with `value` carries a credential in the
/// clear: a header that only ever carries one, with no secret redacted in it.
#[must_use]
pub fn secret_header(name: &str, value: &str) -> bool {
    SECRET_HEADERS.contains(&name.to_ascii_lowercase().as_str()) && !value.contains(REDACTED)
}

fn webhook_token(text: &str) -> bool {
    text.match_indices("/webhooks/").any(|(index, marker)| {
        let rest = text.get(index + marker.len()..).unwrap_or_default();
        let mut parts = rest.splitn(3, '/');
        let id = parts.next().unwrap_or_default();
        let token = parts.next().unwrap_or_default();
        !id.is_empty()
            && id.chars().all(|digit| digit.is_ascii_digit())
            && !token.is_empty()
            && !token.starts_with(REDACTED)
    })
}

fn discord_token(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    let base64url = |part: &str| {
        part.chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-".contains(character))
    };
    matches!(parts.as_slice(), [id, stamp, mac]
        if id.len() >= 23 && stamp.len() == 6 && mac.len() >= 27 && [id, stamp, mac].iter().all(|part| base64url(part)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shaped(parts: &[&str]) -> String {
        parts.concat()
    }

    #[test]
    fn each_kind_of_credential_is_found() {
        for (prefix, what) in PREFIXES {
            let text = shaped(&["Bearer ", prefix, "0123456789abcdef"]);
            assert_eq!(credentials(&text), [what], "{text}");
        }
    }

    #[test]
    fn a_webhook_token_in_a_url_is_found_unless_redacted() {
        let url = shaped(&[
            "https://discord.com/api",
            "/webhooks/",
            "123456",
            "/",
            "abc-DEF",
        ]);
        assert_eq!(credentials(&url), ["a webhook's token in its URL"]);
        assert_eq!(
            credentials(&shaped(&[
                "https://discord.com/api/webhooks/123/",
                REDACTED,
                "HOOK»"
            ])),
            Vec::<&str>::new()
        );
        assert_eq!(
            credentials("https://discord.com/api/webhooks/name/token"),
            Vec::<&str>::new()
        );
        assert_eq!(
            credentials("https://discord.com/api/webhooks/123"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn a_discord_bot_token_is_found_by_its_shape() {
        let token = shaped(&["M", &"T".repeat(23), ".", "GaBcDe", ".", &"x".repeat(27)]);
        assert_eq!(
            credentials(&format!("Bot {token}")),
            ["a Discord bot token"]
        );
        let short = shaped(&["M", &"T".repeat(10), ".", "GaBcDe", ".", &"x".repeat(27)]);
        assert_eq!(credentials(&short), Vec::<&str>::new());
        assert_eq!(credentials("v1.2.3"), Vec::<&str>::new());
    }

    #[test]
    fn a_header_that_only_carries_credentials_must_carry_a_redacted_one() {
        assert!(secret_header("Authorization", "Bot abc"));
        assert!(!secret_header("Authorization", "Bot «secret:TOKEN»"));
        assert!(secret_header("x-slack-signature", "v0=abc"));
        assert!(!secret_header("Content-Type", "application/json"));
    }
}
