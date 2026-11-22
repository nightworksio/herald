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
    let token = Token::Named(Secret::new("TOKEN", "t"));
    pollster::block_on(found(
        &Erased(http),
        (&origin(forge), Some(&token)),
        reference,
        codeowning,
    ))
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
