use herald_core::{CodeOwner, CodeownersRef};
use herald_ports::{Response, Secret};
use herald_testing::ScriptedHttp;

use super::*;
use crate::SourceOptions;

fn text(status: u16, body: &str) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    }
}

fn origin(forge: Forge) -> Origin {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    Origin::new(forge, &options, "o/r")
}

fn codeowning(paths: &[&str], most: usize) -> Codeowning {
    Codeowning {
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        reference: CodeownersRef::Base,
        most,
        most_areas: 200,
    }
}

fn urls(http: &ScriptedHttp) -> Vec<String> {
    http.asked().into_iter().map(|(url, _)| url).collect()
}

fn read(
    http: &ScriptedHttp,
    forge: Forge,
    reference: Option<&str>,
    codeowning: &Codeowning,
) -> Result<Option<Found>, Unpolled> {
    read_kept(http, (forge, reference), codeowning, None)
}

fn read_kept(
    http: &ScriptedHttp,
    (forge, reference): (Forge, Option<&str>),
    codeowning: &Codeowning,
    kept: Option<&KeptCodeowners>,
) -> Result<Option<Found>, Unpolled> {
    let token = Token::Named(Secret::new("TOKEN", "t"));
    pollster::block_on(found(
        &Erased(http),
        (&origin(forge), Some(&token)),
        reference,
        (codeowning, kept),
    ))
}

fn tagged(status: u16, body: &str, etag: &str) -> Response {
    Response {
        headers: vec![("etag".to_owned(), etag.to_owned())],
        ..text(status, body)
    }
}

fn kept_at(path: &str, text: &str, codeowning: &Codeowning) -> KeptCodeowners {
    let key = herald_core::CodeownersKey {
        source: herald_core::SourceName::new("forge"),
        repository: "o/r".to_owned(),
        reference: None,
    };
    let file = Codeowners::read(Dialect::GitHub, text, codeowning.most_areas);
    let read = (codeowning.bounds(), jiff::Timestamp::UNIX_EPOCH);
    KeptCodeowners::of(key, (path, "\"v1\""), &file, read).expect("kept")
}

fn owners(found: &Found) -> Vec<String> {
    let changed = herald_core::Changed {
        paths: ["src/a.rs".to_owned()].into_iter().collect(),
        whole: true,
    };
    found
        .file
        .owning(Some(&changed))
        .owners
        .iter()
        .map(|owner| owner.written().to_owned())
        .collect()
}

#[test]
fn the_first_place_a_file_is_found_at_is_read_at_the_branch_asked() {
    let http = ScriptedHttp::new(vec![text(404, "{}"), text(200, "src/ @org/core @ana\n")]);
    let places = codeowning(
        &[".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"],
        1024,
    );
    let read = read(&http, Forge::GitHub, Some("main"), &places)
        .unwrap()
        .unwrap();
    assert_eq!(read.path, "CODEOWNERS");
    assert_eq!(read.file.refused(), None);
    let changed = herald_core::Changed {
        paths: ["src/lib.rs".to_owned()].into(),
        whole: true,
    };
    assert_eq!(
        read.file
            .owning(Some(&changed))
            .owners
            .into_iter()
            .collect::<Vec<_>>(),
        [
            CodeOwner::Team("@org/core".to_owned()),
            CodeOwner::Person("@ana".to_owned())
        ]
    );
    assert_eq!(
        urls(&http),
        [
            "https://api.github.com/repos/o/r/contents/.github/CODEOWNERS?ref=main",
            "https://api.github.com/repos/o/r/contents/CODEOWNERS?ref=main"
        ]
    );
}

#[test]
fn a_file_at_no_place_is_none_and_another_answer_fails_the_read() {
    let places = codeowning(&["CODEOWNERS", "docs/CODEOWNERS"], 1024);
    let http = ScriptedHttp::new(vec![text(404, "{}"), text(404, "{}")]);
    assert!(
        read(&http, Forge::Forgejo, None, &places)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        urls(&http),
        [
            "https://codeberg.org/api/v1/repos/o/r/raw/CODEOWNERS",
            "https://codeberg.org/api/v1/repos/o/r/raw/docs/CODEOWNERS"
        ]
    );
    let http = ScriptedHttp::new(vec![text(403, "{}")]);
    assert!(matches!(
        read(&http, Forge::GitHub, None, &places),
        Err(Unpolled::Unreadable(_))
    ));
    assert_eq!(urls(&http).len(), 1);
    let http = ScriptedHttp::new(vec![text(403, r#"{"message":"API rate limit exceeded"}"#)]);
    assert!(matches!(
        read(&http, Forge::GitHub, None, &places),
        Err(Unpolled::RateLimited(_))
    ));
    let http = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        read(&http, Forge::GitHub, None, &places),
        Err(Unpolled::Unreachable(_))
    ));
}

#[test]
fn a_file_longer_than_the_source_reads_is_found_and_refused_in_its_forge_s_dialect() {
    let body = "[Docs]\n*.md @docs\n";
    let http = ScriptedHttp::new(vec![text(200, body)]);
    let refused = read(
        &http,
        Forge::GitLab,
        None,
        &codeowning(&["CODEOWNERS"], body.len() - 1),
    )
    .unwrap()
    .unwrap();
    assert!(refused.file.refused().is_some());
    let http = ScriptedHttp::new(vec![text(200, body)]);
    let sectioned = read(
        &http,
        Forge::GitLab,
        None,
        &codeowning(&["CODEOWNERS"], body.len()),
    )
    .unwrap()
    .unwrap();
    let changed = herald_core::Changed {
        paths: ["a.md".to_owned()].into(),
        whole: true,
    };
    assert_eq!(
        sectioned
            .file
            .owning(Some(&changed))
            .sections
            .into_iter()
            .collect::<Vec<_>>(),
        ["Docs"]
    );
    assert_eq!(dialect(Forge::GitHub), Dialect::GitHub);
    assert_eq!(dialect(Forge::GitLab), Dialect::GitLab);
    assert_eq!(dialect(Forge::Forgejo), Dialect::Forgejo);
}

fn looked_up(http: &ScriptedHttp, forge: Forge, name: &str) -> Result<AreaKind, Unpolled> {
    pollster::block_on(named(&Erased(http), (&origin(forge), None), name))
}

#[test]
fn a_gitlab_name_is_a_person_a_group_or_a_name_by_at_most_two_lookups() {
    let http = ScriptedHttp::new(vec![text(200, "[{\"id\": 1}]")]);
    assert_eq!(
        looked_up(&http, Forge::GitLab, "@ana"),
        Ok(AreaKind::Person)
    );
    assert_eq!(
        urls(&http),
        ["https://gitlab.com/api/v4/users?username=ana"]
    );
    let http = ScriptedHttp::new(vec![text(200, "[]"), text(200, "{\"id\": 2}")]);
    assert_eq!(
        looked_up(&http, Forge::GitLab, "@web.team"),
        Ok(AreaKind::Group)
    );
    assert_eq!(
        urls(&http),
        [
            "https://gitlab.com/api/v4/users?username=web.team",
            "https://gitlab.com/api/v4/groups/web.team"
        ]
    );
    let http = ScriptedHttp::new(vec![text(403, "[{\"id\": 1}]"), text(404, "{}")]);
    assert_eq!(
        looked_up(&http, Forge::GitLab, "@hidden"),
        Ok(AreaKind::Name)
    );
    let http = ScriptedHttp::new(vec![text(200, "not json"), text(401, "{}")]);
    assert_eq!(looked_up(&http, Forge::GitLab, "@odd"), Ok(AreaKind::Name));
    for answers in [
        vec![text(429, "Retry later")],
        vec![text(200, "[]"), text(429, "Retry later")],
    ] {
        let http = ScriptedHttp::new(answers);
        assert!(matches!(
            looked_up(&http, Forge::GitLab, "@busy"),
            Err(Unpolled::RateLimited(_))
        ));
    }
    let http = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        looked_up(&http, Forge::GitLab, "@gone"),
        Err(Unpolled::Unreachable(_))
    ));
    let http = ScriptedHttp::new(vec![text(200, "[]")]);
    assert!(matches!(
        looked_up(&http, Forge::GitLab, "@gone"),
        Err(Unpolled::Unreachable(_))
    ));
    let http = ScriptedHttp::new(Vec::new());
    for (forge, name) in [
        (Forge::GitHub, "@ana"),
        (Forge::Forgejo, "@ana"),
        (Forge::GitLab, "@"),
        (Forge::GitLab, "@a b"),
        (Forge::GitLab, "@a%2Fb"),
    ] {
        assert_eq!(looked_up(&http, forge, name), Ok(AreaKind::Name), "{name}");
    }
    assert_eq!(urls(&http), Vec::<String>::new());
    let http = ScriptedHttp::new(vec![text(200, "[{}]")]);
    assert_eq!(
        looked_up(&http, Forge::GitLab, "under_score-1"),
        Ok(AreaKind::Person)
    );
}

/// A transport refusing, as every runtime's does, an answer longer than
/// its request reads.
struct Capped(ScriptedHttp);

impl herald_ports::Http for Capped {
    async fn send(&self, request: herald_ports::Request) -> Result<Response, HttpError> {
        let most = request.most;
        let answer = self.0.send(request).await?;
        if answer.body.len() > most {
            return Err(HttpError::Larger { most });
        }
        Ok(answer)
    }
}

#[test]
fn no_more_of_a_file_than_the_source_reads_is_downloaded_and_a_longer_one_is_refused_unread() {
    let most = herald_core::MOST_CODEOWNERS_READ + 16;
    let body = format!("*.md @docs\n#{}\n", "x".repeat(most - 12));
    let token = Token::Named(Secret::new("TOKEN", "t"));
    let asked = |places: &Codeowning, answers: Vec<Response>| {
        let http = Capped(ScriptedHttp::new(answers));
        let read = pollster::block_on(found(
            &Erased(&http),
            (&origin(Forge::GitHub), Some(&token)),
            None,
            (places, None),
        ));
        let most: Vec<usize> = http
            .0
            .requests()
            .iter()
            .map(|request| request.most)
            .collect();
        (read.unwrap().unwrap(), most)
    };
    let (refused, read) = asked(
        &codeowning(&["CODEOWNERS"], most),
        vec![tagged(200, &body, "\"v1\"")],
    );
    assert_eq!(read, [most]);
    assert_eq!(refused.path, "CODEOWNERS");
    assert_eq!(refused.etag, None);
    let longer = Codeowners::longer(Dialect::GitHub, most);
    assert_eq!(refused.file.refused(), longer.refused());
    let (whole, _) = asked(
        &codeowning(&["CODEOWNERS"], most + 1),
        vec![tagged(200, &body, "\"v1\"")],
    );
    assert_eq!(whole.file.refused(), None);
    assert_eq!(whole.etag.as_deref(), Some("\"v1\""));
    let missing = format!("{{\"message\":\"{}\"}}", "n".repeat(64));
    let (second, read) = asked(
        &codeowning(&[".github/CODEOWNERS", "CODEOWNERS"], 8),
        vec![text(404, &missing), text(200, "* @a\n")],
    );
    let default = herald_core::MOST_CODEOWNERS_READ;
    assert_eq!(read, [default, default]);
    assert_eq!(second.path, "CODEOWNERS");
}

#[test]
fn the_kept_place_is_asked_by_its_validator_and_an_unchanged_file_is_the_kept_one() {
    let places = codeowning(&[".github/CODEOWNERS", "CODEOWNERS"], 4096);
    let kept = kept_at("CODEOWNERS", "src/ @org/kept\n", &places);
    let http = ScriptedHttp::new(vec![text(404, "{}"), text(304, "")]);
    let found = read_kept(&http, (Forge::GitHub, Some("main")), &places, Some(&kept))
        .unwrap()
        .unwrap();
    assert_eq!(found.path, "CODEOWNERS");
    assert_eq!(found.etag, None);
    assert_eq!(owners(&found), ["@org/kept"]);
    let asked: Vec<Option<String>> = http.asked().into_iter().map(|(_, etag)| etag).collect();
    assert_eq!(asked, [None, Some("\"v1\"".to_owned())]);
}

#[test]
fn a_changed_file_at_the_kept_place_is_read_in_full_with_its_new_validator() {
    let places = codeowning(&["CODEOWNERS"], 4096);
    let kept = kept_at("CODEOWNERS", "src/ @org/kept\n", &places);
    let http = ScriptedHttp::new(vec![tagged(200, "src/ @org/new\n", "\"v2\"")]);
    let found = read_kept(&http, (Forge::GitHub, None), &places, Some(&kept))
        .unwrap()
        .unwrap();
    assert_eq!(found.etag.as_deref(), Some("\"v2\""));
    assert_eq!(owners(&found), ["@org/new"]);
    let full = ScriptedHttp::new(vec![tagged(200, "src/ @org/new\n", "\"v3\"")]);
    let read = read(&full, Forge::GitHub, None, &places).unwrap().unwrap();
    assert_eq!(read.etag.as_deref(), Some("\"v3\""));
}

#[test]
fn a_kept_file_of_other_bounds_another_dialect_or_place_is_not_asked_by_its_validator() {
    let places = codeowning(&["CODEOWNERS"], 4096);
    let kept = kept_at("CODEOWNERS", "src/ @org/kept\n", &places);
    let others = [
        (codeowning(&["CODEOWNERS"], 4095), Forge::GitHub),
        (
            Codeowning {
                most_areas: 199,
                ..places.clone()
            },
            Forge::GitHub,
        ),
        (places.clone(), Forge::GitLab),
        (codeowning(&["docs/CODEOWNERS"], 4096), Forge::GitHub),
    ];
    for (other, forge) in others {
        let http = ScriptedHttp::new(vec![text(200, "src/ @org/new\n")]);
        let found = read_kept(&http, (forge, None), &other, Some(&kept))
            .unwrap()
            .unwrap();
        assert_eq!(owners(&found), ["@org/new"], "{forge:?}");
        assert_eq!(http.asked()[0].1, None, "{forge:?}");
    }
}

#[test]
fn an_unchanged_file_whose_kept_form_does_not_read_back_is_read_again_in_full() {
    let places = codeowning(&["CODEOWNERS"], 4096);
    let kept = kept_at("CODEOWNERS", "src/ @org/kept\n", &places);
    let mut written = serde_json::to_value(&kept).unwrap();
    written["file"]["patterns"] = serde_json::json!(["("]);
    let broken: KeptCodeowners = serde_json::from_value(written).unwrap();
    let http = ScriptedHttp::new(vec![
        text(304, ""),
        tagged(200, "src/ @org/new\n", "\"v2\""),
    ]);
    let found = read_kept(&http, (Forge::GitHub, None), &places, Some(&broken))
        .unwrap()
        .unwrap();
    assert_eq!(owners(&found), ["@org/new"]);
    assert_eq!(found.etag.as_deref(), Some("\"v2\""));
    let asked: Vec<Option<String>> = http.asked().into_iter().map(|(_, etag)| etag).collect();
    assert_eq!(asked, [Some("\"v1\"".to_owned()), None]);
}

#[test]
fn a_304_to_a_place_asked_without_a_validator_is_no_file_read() {
    let places = codeowning(&[".github/CODEOWNERS", "CODEOWNERS"], 4096);
    let kept = kept_at("CODEOWNERS", "src/ @org/kept\n", &places);
    let http = ScriptedHttp::new(vec![text(304, "")]);
    let read = read_kept(&http, (Forge::GitHub, None), &places, Some(&kept));
    assert!(matches!(read, Err(Unpolled::Unreadable(_))), "{read:?}");
    assert_eq!(http.asked().len(), 1);
}
