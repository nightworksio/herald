use super::*;

fn limited() -> Response {
    answer(429, &json!({}))
}

fn stopped<T: std::fmt::Debug>(answer: Result<T, Unpolled>, what: &str) {
    match answer {
        Err(Unpolled::RateLimited(reason)) => assert!(reason.contains(what), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_rate_limited_read_of_a_head_an_operator_or_a_revocation_stops_as_one() {
    let http = ScriptedHttp::new(vec![limited()]);
    stopped(
        pollster::block_on(github().head(&http, ("o/r", 5))),
        "a pull request's head",
    );
    let http = ScriptedHttp::new(vec![limited()]);
    stopped(
        pollster::block_on(github().operator((&http, &http), &oauth(), None)),
        "the signed-in operator",
    );
    let http = ScriptedHttp::new(vec![limited()]);
    stopped(
        pollster::block_on(gitlab().revoked(&http, &OperatorToken::new("operator-value"))),
        "the operator's token revoked",
    );
}
