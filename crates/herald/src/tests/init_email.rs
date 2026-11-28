use super::init::{git, init, key, written};

#[test]
fn a_mailgun_first_run_writes_its_transport_unasked_and_asks_its_region_domain_and_key() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), "git@github.com:owner/app.git");
    let asked = [
        "--chat",
        "email",
        "--variant",
        "mailgun",
        "--with",
        "from=releases@mg.example.org",
        "--with",
        "region=eu",
        "--with",
        "domain=mg.example.org",
        "--with",
        "to=team@example.org",
        "--yes",
    ];
    let (done, out) = init(directory.path(), &asked, "", Some(key()));
    assert_eq!(done, Ok(crate::DONE), "{out}");
    let text = written(directory.path()).unwrap();
    for written in [
        "kind = \"email\"\ntransport = \"mailgun\"\nfrom = \"releases@mg.example.org\"\nregion = \"eu\"\ndomain = \"mg.example.org\"\napi_key = { secret = \"MAILGUN_API_KEY\" }\n",
        "to = \"team@example.org\"",
    ] {
        assert!(text.contains(written), "{written}\n{text}");
    }
    assert!(
        out.contains("first-run.md#email-mailgun says how to make it."),
        "{out}"
    );
}
