use super::*;

fn limited() -> Response {
    answer(429, &json!({}), None)
}

fn stopped<T: std::fmt::Debug>(answer: Result<T, Unpolled>, what: &str) {
    match answer {
        Err(Unpolled::RateLimited(reason)) => assert!(reason.contains(what), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_rate_limited_release_or_repository_read_stops_the_poll_as_one() {
    let source = releases(
        Forge::GitHub,
        &[Signal::Release],
        (PrereleaseRule::Either, 0),
    );
    let listing = ScriptedHttp::new(vec![limited(), repository(Forge::GitHub)]);
    stopped(polled(&source, &listing, None), "GitHub's releases");
    let described = ScriptedHttp::new(vec![answer(200, &json!([]), None), limited()]);
    stopped(polled(&source, &described, None), "GitHub's repository");
}

#[test]
fn a_rate_limited_tag_listing_reference_or_object_stops_the_poll_as_one() {
    let source = releases(Forge::GitHub, &[Signal::Tag], (PrereleaseRule::Either, 1));
    let tagged = || answer(200, &json!([{"name": "v1"}]), None);
    let listing = ScriptedHttp::new(vec![limited(), repository(Forge::GitHub)]);
    stopped(
        polled(&source, &listing, Some(&Cursor::default())),
        "GitHub's tags",
    );
    let reference = ScriptedHttp::new(vec![tagged(), repository(Forge::GitHub), limited()]);
    stopped(
        polled(&source, &reference, Some(&Cursor::default())),
        "GitHub's tag",
    );
    let commit = ScriptedHttp::new(vec![
        tagged(),
        repository(Forge::GitHub),
        answer(
            200,
            &json!({"object": {"type": "commit", "sha": "c"}}),
            None,
        ),
        limited(),
    ]);
    stopped(
        polled(&source, &commit, Some(&Cursor::default())),
        "GitHub's tag",
    );
}

#[test]
fn a_rate_limited_read_of_one_tag_stops_as_one() {
    stopped(
        one_tag(Forge::GitLab, &ScriptedHttp::new(vec![limited()])),
        "GitLab's project",
    );
    stopped(
        one_tag(
            Forge::GitLab,
            &ScriptedHttp::new(vec![repository(Forge::GitLab), limited()]),
        ),
        "GitLab's tag",
    );
}
