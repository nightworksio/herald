use herald_core::merging::{Message, Method, Update};
use herald_kinds::{Done, Merge, MergeWriter};
use herald_ports::Method as Verb;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::super::tests::{answer, bearer, entered, write_value, writer};
use crate::Forge;

fn updated(
    forge: Forge,
    update: Update,
    answers: Vec<herald_ports::Response>,
) -> (ScriptedHttp, Done) {
    let http = ScriptedHttp::new(answers);
    let writer = writer(forge, None);
    let done = pollster::block_on(writer.update(&http, &entered(None), update)).unwrap();
    (http, done)
}

fn merged(
    forge: Forge,
    (method, message): (Method, Message),
    answers: Vec<herald_ports::Response>,
) -> (ScriptedHttp, Result<Done, herald_kinds::Unpolled>) {
    let http = ScriptedHttp::new(answers);
    let writer = writer(forge, None);
    let mut pull = entered(None);
    pull.title = "Add it".to_owned();
    pull.body = "Because".to_owned();
    let merge = Merge {
        method,
        message,
        entered: &pull,
    };
    let done = pollster::block_on(writer.merge(&http, merge));
    (http, done)
}

fn sent(http: &ScriptedHttp) -> (Verb, String, Value) {
    let request = &http.requests()[0];
    assert!(
        request
            .headers
            .iter()
            .any(|(_, value)| value.expose().ends_with(&write_value())),
        "{:?}",
        bearer(request)
    );
    (
        request.method,
        request.url.expose().clone(),
        http.bodies()[0].clone(),
    )
}

#[test]
fn each_forge_is_asked_to_update_the_branch_and_github_alone_with_the_head_it_must_find() {
    let (http, done) = updated(Forge::GitHub, Update::Merge, vec![answer(202, &json!({}))]);
    assert_eq!(done, Done::Took);
    assert_eq!(
        sent(&http),
        (
            Verb::Put,
            "https://api.github.com/repos/o/r/pulls/7/update-branch".to_owned(),
            json!({ "expected_head_sha": "h7" })
        )
    );
    let (http, done) = updated(Forge::GitLab, Update::Rebase, vec![answer(202, &json!({}))]);
    assert_eq!(done, Done::Took);
    let (verb, url, _) = sent(&http);
    assert_eq!(verb, Verb::Put);
    assert_eq!(
        url,
        "https://gitlab.com/api/v4/projects/o%2Fr/merge_requests/7/rebase"
    );
    for (update, style) in [(Update::Merge, "merge"), (Update::Rebase, "rebase")] {
        let (http, done) = updated(Forge::Forgejo, update, vec![answer(200, &json!({}))]);
        assert_eq!(done, Done::Took);
        let (verb, url, _) = sent(&http);
        assert_eq!(verb, Verb::Post);
        assert!(
            url.ends_with(&format!("/repos/o/r/pulls/7/update?style={style}")),
            "{url}"
        );
    }
    assert!(writer(Forge::GitHub, None).pins_update());
    assert!(!writer(Forge::GitLab, None).pins_update());
    assert!(!writer(Forge::Forgejo, None).pins_update());
}

#[test]
fn an_update_that_conflicts_is_told_from_one_refused_and_neither_shows_the_credential() {
    let conflict = json!({ "message": format!("merge conflict for {}", write_value()) });
    let (_, done) = updated(Forge::GitHub, Update::Merge, vec![answer(422, &conflict)]);
    let Done::Conflict(said) = done else {
        panic!("{done:?}")
    };
    assert!(
        !said.contains(&write_value()) && said.contains("[the write credential]"),
        "{said}"
    );
    let moved = json!({ "message": "expected head sha didn't match current head ref" });
    let (_, done) = updated(Forge::GitHub, Update::Merge, vec![answer(422, &moved)]);
    assert!(matches!(done, Done::Refused(_)), "{done:?}");
    let (_, done) = updated(Forge::Forgejo, Update::Merge, vec![answer(409, &json!({}))]);
    assert!(matches!(done, Done::Conflict(_)), "{done:?}");
    let (_, done) = updated(Forge::Forgejo, Update::Merge, vec![answer(403, &json!({}))]);
    assert_eq!(done, Done::Refused("the forge answered 403".to_owned()));
    let (_, done) = updated(Forge::GitLab, Update::Rebase, vec![answer(409, &json!({}))]);
    assert!(matches!(done, Done::Refused(_)), "{done:?}");
    let (_, merge) = merged(
        Forge::Forgejo,
        (Method::Merge, Message::Forge),
        vec![answer(409, &json!({}))],
    );
    assert!(matches!(merge, Ok(Done::Refused(_))), "{merge:?}");
    let (_, merge) = merged(
        Forge::GitHub,
        (Method::Merge, Message::Forge),
        vec![answer(422, &json!({ "message": "merge conflict" }))],
    );
    assert!(matches!(merge, Ok(Done::Refused(_))), "{merge:?}");
    for lost in [
        vec![],
        vec![answer(500, &json!({}))],
        vec![answer(429, &json!({}))],
    ] {
        let (_, done) = updated(Forge::GitHub, Update::Merge, lost);
        assert_eq!(done, Done::Lost);
    }
}

#[test]
fn github_merges_with_the_head_it_must_find_the_method_and_the_pull_request_s_message() {
    let (http, done) = merged(
        Forge::GitHub,
        (Method::Squash, Message::Pull),
        vec![answer(200, &json!({ "merged": true }))],
    );
    assert_eq!(done.unwrap(), Done::Took);
    assert_eq!(
        sent(&http),
        (
            Verb::Put,
            "https://api.github.com/repos/o/r/pulls/7/merge".to_owned(),
            json!({ "sha": "h7", "merge_method": "squash", "commit_title": "Add it", "commit_message": "Because" })
        )
    );
    let (http, _) = merged(
        Forge::GitHub,
        (Method::Rebase, Message::Pull),
        vec![answer(200, &json!({}))],
    );
    assert_eq!(
        sent(&http).2,
        json!({ "sha": "h7", "merge_method": "rebase" })
    );
    let (http, done) = merged(Forge::GitHub, (Method::FastForward, Message::Forge), vec![]);
    assert!(done.is_err());
    assert_eq!(http.requests().len(), 0);
    for status in [405, 409, 422] {
        let (_, done) = merged(
            Forge::GitHub,
            (Method::Merge, Message::Forge),
            vec![answer(status, &json!({ "message": "no" }))],
        );
        assert_eq!(
            done.unwrap(),
            Done::Refused(format!("the forge answered {status}: no"))
        );
    }
}

#[test]
fn gitlab_and_forgejo_merge_with_the_head_they_must_find() {
    let (http, done) = merged(
        Forge::GitLab,
        (Method::Squash, Message::Pull),
        vec![answer(200, &json!({ "state": "merged" }))],
    );
    assert_eq!(done.unwrap(), Done::Took);
    let (verb, url, body) = sent(&http);
    assert_eq!(verb, Verb::Put);
    assert_eq!(
        url,
        "https://gitlab.com/api/v4/projects/o%2Fr/merge_requests/7/merge"
    );
    assert_eq!(
        body,
        json!({ "sha": "h7", "squash": true, "squash_commit_message": "Add it\n\nBecause" })
    );
    let (http, _) = merged(
        Forge::GitLab,
        (Method::Merge, Message::Pull),
        vec![answer(200, &json!({}))],
    );
    assert_eq!(
        sent(&http).2,
        json!({ "sha": "h7", "squash": false, "merge_commit_message": "Add it\n\nBecause" })
    );
    let (http, _) = merged(
        Forge::GitLab,
        (Method::FastForward, Message::Forge),
        vec![answer(200, &json!({}))],
    );
    assert_eq!(sent(&http).2, json!({ "sha": "h7", "squash": false }));
    for (method, named) in [
        (Method::Merge, "merge"),
        (Method::Squash, "squash"),
        (Method::Rebase, "rebase"),
        (Method::FastForward, "fast-forward-only"),
    ] {
        let (http, done) = merged(
            Forge::Forgejo,
            (method, Message::Pull),
            vec![answer(200, &json!({}))],
        );
        assert_eq!(done.unwrap(), Done::Took);
        let (verb, url, body) = sent(&http);
        assert_eq!(verb, Verb::Post);
        assert!(url.ends_with("/repos/o/r/pulls/7/merge"), "{url}");
        assert_eq!(body["Do"], named);
        assert_eq!(body["head_commit_id"], "h7");
        assert_eq!(
            body.get("MergeTitleField").is_some(),
            method.messaged(),
            "{method:?}"
        );
    }
    let (_, done) = merged(
        Forge::Forgejo,
        (Method::Merge, Message::Forge),
        vec![answer(423, &json!({}))],
    );
    assert_eq!(
        done.unwrap(),
        Done::Refused("the forge answered 423".to_owned())
    );
}

#[test]
fn each_forge_with_jobs_is_asked_to_re_run_the_failed_job_alone_and_a_refusal_is_told() {
    let job = herald_core::Rerun {
        run: "42".to_owned(),
        job: "420".to_owned(),
        page: "https://forge.test/o/r/runs/42".to_owned(),
        call: String::new(),
    };
    for (forge, url, status, said) in [
        (
            Forge::GitHub,
            "https://api.github.com/repos/o/r/actions/jobs/420/rerun",
            201,
            Done::Took,
        ),
        (
            Forge::GitLab,
            "https://gitlab.com/api/v4/projects/o%2Fr/jobs/420/retry",
            403,
            Done::Refused("the forge answered 403".to_owned()),
        ),
    ] {
        let http = ScriptedHttp::new(vec![answer(status, &json!({}))]);
        let done = pollster::block_on(writer(forge, None).rerun(&http, &job)).unwrap();
        assert_eq!(sent(&http), (Verb::Post, url.to_owned(), json!({})));
        assert_eq!(done, said, "{forge:?}");
    }
    let unnumbered = herald_core::Rerun {
        job: "../420".to_owned(),
        ..job
    };
    let http = ScriptedHttp::new(Vec::new());
    assert!(pollster::block_on(writer(Forge::GitHub, None).rerun(&http, &unnumbered)).is_err());
    assert_eq!(http.requests().len(), 0);
}
