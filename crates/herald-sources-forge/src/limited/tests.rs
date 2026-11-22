use super::*;

const WHAT: &str = "team listing";
const DATE: (&str, &str) = ("Date", "Sun, 22 Nov 2026 12:00:00 GMT");
const RESET_AT: &str = "1795349100";
const PRIMARY: &str = r#"{"message":"API rate limit exceeded for installation ID 7.","documentation_url":"https://docs.github.com/rest/overview/rate-limits-for-the-rest-api"}"#;
const SECONDARY: &str = r#"{"message":"You have exceeded a secondary rate limit. Please wait a few minutes before you try again."}"#;
const DENIED: &str = r#"{"message":"Resource not accessible by integration"}"#;

fn answer(status: u16, headers: &[(&str, &str)], body: &str) -> Response {
    Response {
        status,
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        body: body.as_bytes().to_vec(),
    }
}

fn said(forge: Forge, answer: &Response) -> Option<String> {
    limited(forge, WHAT, answer).map(|stop| match stop {
        Unpolled::RateLimited(reason) => reason,
        other => panic!("{other:?}"),
    })
}

fn resetting(wait: &str) -> String {
    format!("the forge's {WHAT} was refused for its rate limit; the limit resets in {wait}")
}

fn unsaid() -> String {
    format!(
        "the forge's {WHAT} was refused for its rate limit; it did not say when the limit resets"
    )
}

#[test]
fn github_s_primary_limit_as_403_or_429_is_a_rate_limit_resetting_when_its_headers_say() {
    let spent = [
        DATE,
        ("X-RateLimit-Remaining", "0"),
        ("X-RateLimit-Reset", RESET_AT),
    ];
    for status in [403, 429] {
        let primary = answer(status, &spent, PRIMARY);
        assert_eq!(
            said(Forge::GitHub, &primary),
            Some(resetting("5m")),
            "{status}"
        );
    }
    let unbodied = answer(403, &spent, "");
    assert_eq!(said(Forge::GitHub, &unbodied), Some(resetting("5m")));
    let undated = answer(403, &spent[1..], "");
    assert_eq!(said(Forge::GitHub, &undated), Some(unsaid()));
}

#[test]
fn github_s_secondary_limit_is_told_by_its_retry_after_or_its_message() {
    let waiting = answer(403, &[("Retry-After", "60")], SECONDARY);
    assert_eq!(said(Forge::GitHub, &waiting), Some(resetting("1m")));
    let unheadered = answer(403, &[], SECONDARY);
    assert_eq!(said(Forge::GitHub, &unheadered), Some(unsaid()));
    let shouted = answer(403, &[], r#"{"message":"API RATE LIMIT exceeded"}"#);
    assert_eq!(said(Forge::GitHub, &shouted), Some(unsaid()));
}

#[test]
fn a_github_403_without_a_limit_s_sign_is_a_refusal() {
    let left = [
        DATE,
        ("X-RateLimit-Remaining", "4999"),
        ("X-RateLimit-Reset", RESET_AT),
    ];
    assert_eq!(said(Forge::GitHub, &answer(403, &left, DENIED)), None);
    assert_eq!(said(Forge::GitHub, &answer(403, &[], "rate limit")), None);
    assert_eq!(said(Forge::GitHub, &answer(401, &[], PRIMARY)), None);
    assert_eq!(said(Forge::GitHub, &answer(404, &[], PRIMARY)), None);
    assert_eq!(said(Forge::GitHub, &answer(200, &[], PRIMARY)), None);
}

#[test]
fn gitlab_s_limit_is_a_429_resetting_when_its_ratelimit_headers_say() {
    let limited = answer(
        429,
        &[
            DATE,
            ("RateLimit-Remaining", "0"),
            ("RateLimit-Reset", RESET_AT),
        ],
        "Retry later\n",
    );
    assert_eq!(said(Forge::GitLab, &limited), Some(resetting("5m")));
    let waiting = answer(
        429,
        &[DATE, ("Retry-After", "30"), ("RateLimit-Reset", RESET_AT)],
        "",
    );
    assert_eq!(said(Forge::GitLab, &waiting), Some(resetting("30s")));
    let spent = answer(403, &[("RateLimit-Remaining", "0")], "");
    assert_eq!(said(Forge::GitLab, &spent), Some(unsaid()));
    let github_s = answer(
        403,
        &[
            DATE,
            ("X-RateLimit-Remaining", "0"),
            ("X-RateLimit-Reset", RESET_AT),
        ],
        PRIMARY,
    );
    assert_eq!(said(Forge::GitLab, &github_s), None);
    assert_eq!(
        said(
            Forge::GitLab,
            &answer(403, &[("RateLimit-Remaining", "3")], "")
        ),
        None
    );
}

#[test]
fn forgejo_s_limit_is_a_429_or_a_403_asking_to_wait_and_no_header_of_another_forge() {
    let waiting = answer(429, &[("Retry-After", "7")], "");
    assert_eq!(said(Forge::Forgejo, &waiting), Some(resetting("7s")));
    let unsaid_reset = answer(
        429,
        &[
            DATE,
            ("X-RateLimit-Reset", RESET_AT),
            ("RateLimit-Reset", RESET_AT),
        ],
        "",
    );
    assert_eq!(said(Forge::Forgejo, &unsaid_reset), Some(unsaid()));
    assert_eq!(
        said(Forge::Forgejo, &answer(403, &[("Retry-After", "7")], "")),
        Some(resetting("7s"))
    );
    let spent = answer(
        403,
        &[("X-RateLimit-Remaining", "0"), ("RateLimit-Remaining", "0")],
        PRIMARY,
    );
    assert_eq!(said(Forge::Forgejo, &spent), None);
}

#[test]
fn a_checked_answer_is_one_a_limit_or_one_herald_cannot_read() {
    assert_eq!(
        checked(Forge::GitHub, WHAT, &answer(200, &[], "[]")),
        Ok(())
    );
    assert_eq!(
        checked(Forge::GitLab, WHAT, &answer(429, &[], "")),
        Err(Unpolled::RateLimited(unsaid()))
    );
    assert_eq!(
        checked(Forge::GitHub, WHAT, &answer(500, &[], "")),
        Err(Unpolled::Unreadable(format!(
            "{WHAT} was answered with 500"
        )))
    );
}
