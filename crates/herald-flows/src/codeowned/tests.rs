use std::sync::Mutex;

use herald_core::{
    Advising, Advisories, Capacity, Codeowners, Dialect, Overviews, Pull, Validators,
};
use herald_kinds::{Asked, Codeowning, Plan, Relisted};
use herald_ports::{Boxed, Store, StoreError, Unkept};
use herald_sources_forge::{Forge, ForgeSource, SourceOptions};
use herald_testing::FakeStore;
use herald_testing::ScriptedHttp;

use super::*;

/// A queue serving one CODEOWNERS file and telling each name by a list,
/// counting what it is asked.
#[derive(Debug)]
struct Served {
    file: Option<String>,
    people: &'static [&'static str],
    etag: Option<&'static str>,
    read: Mutex<Vec<Option<String>>>,
    asked: Mutex<Vec<Option<String>>>,
    looked: Mutex<Vec<String>>,
}

fn served(file: Option<String>, people: &'static [&'static str]) -> Served {
    Served {
        file,
        people,
        etag: None,
        read: Mutex::new(Vec::new()),
        asked: Mutex::new(Vec::new()),
        looked: Mutex::new(Vec::new()),
    }
}

/// No store: each round reads each file in full.
const UNKEPT: Option<&Unkept> = None;

fn keeping<P>(pulls: Option<&P>) -> Keeping<'_, P> {
    (pulls, Timestamp::UNIX_EPOCH)
}

fn reading<'a>(source: &'a Source, reach: Option<Reach<'a>>) -> Reading<'a> {
    (&FORGE, source, reach)
}

static FORGE: std::sync::LazyLock<SourceName> =
    std::sync::LazyLock::new(|| SourceName::new("forge"));

fn unread<'a, T: Send + 'a>() -> Boxed<'a, Result<T, Unpolled>> {
    Box::pin(async { Err(Unpolled::Unreadable(String::new())) })
}

impl Queue for Served {
    fn scoped(&self) -> bool {
        false
    }
    fn opened<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Asked<'a>,
    ) -> Boxed<'a, Result<Vec<Pull>, Unpolled>> {
        unread()
    }
    fn listed<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Asked<'a>,
        _: &'a [Validators],
    ) -> Boxed<'a, Result<Relisted, Unpolled>> {
        unread()
    }
    fn holds(&self, _: &str) -> bool {
        true
    }
    fn planned<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: (&'a str, &'a str, &'a str),
        _: usize,
    ) -> Boxed<'a, Result<Plan, Unpolled>> {
        unread()
    }
    fn changed<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: (&'a str, u64),
    ) -> Boxed<'a, Result<Changed, Unpolled>> {
        unread()
    }
    fn capacity<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: (Asked<'a>, &'a [&'a str]),
        _: Capacity,
    ) -> Boxed<'a, Result<Overviews, Unpolled>> {
        unread()
    }
    fn advisories<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: (Asked<'a>, &'a [&'a str]),
        _: (Advising, jiff::Timestamp),
    ) -> Boxed<'a, Result<Advisories, Unpolled>> {
        unread()
    }
    fn completed<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Asked<'a>,
        _: Vec<Pull>,
    ) -> Boxed<'a, Result<Vec<Pull>, Unpolled>> {
        unread()
    }
    fn repository(&self) -> Option<&str> {
        None
    }
    fn codeowners<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        (_, reference): (&'a str, Option<&'a str>),
        (codeowning, kept): (&'a Codeowning, Option<&'a KeptCodeowners>),
    ) -> Boxed<'a, Result<Option<Found>, Unpolled>> {
        self.read.lock().unwrap().push(reference.map(str::to_owned));
        self.asked
            .lock()
            .unwrap()
            .push(kept.map(|kept| kept.etag.clone()));
        let unchanged = kept.filter(|kept| Some(kept.etag.as_str()) == self.etag);
        let found = match unchanged.and_then(KeptCodeowners::file) {
            Some(file) => Some(Found {
                path: "CODEOWNERS".to_owned(),
                file,
                etag: None,
            }),
            None => self.file.as_ref().map(|file| Found {
                path: "CODEOWNERS".to_owned(),
                file: Codeowners::read(Dialect::GitLab, file, codeowning.most_areas),
                etag: self.etag.map(ToOwned::to_owned),
            }),
        };
        Box::pin(async move { Ok(found) })
    }
    fn named<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        name: &'a str,
    ) -> Boxed<'a, Result<AreaKind, Unpolled>> {
        self.looked.lock().unwrap().push(name.to_owned());
        let told = if self.people.contains(&name) {
            Ok(AreaKind::Person)
        } else if name == "@gone" {
            Err(Unpolled::Unreachable(String::new()))
        } else {
            Ok(AreaKind::Group)
        };
        Box::pin(async move { told })
    }
}

fn source() -> Source {
    source_at(None)
}

fn source_at(reference: Option<herald_core::CodeownersRef>) -> Source {
    let options = SourceOptions {
        repository: Some("g/p".to_owned()),
        areas: Some(Zoning::Codeowners),
        codeowners_ref: reference,
        ..SourceOptions::default()
    };
    Source::new(ForgeSource {
        forge: Forge::GitLab,
        options,
        names: None,
    })
}

fn changed(paths: &[&str]) -> Changed {
    Changed {
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        whole: true,
    }
}

#[test]
fn each_gitlab_name_a_pull_request_names_is_looked_up_once_a_round_and_the_file_read_once() {
    let served = served(
        Some("*.md @ana @web @gone\n*.rs @ana @unmet\n".to_owned()),
        &["@ana"],
    );
    let http = ScriptedHttp::new(Vec::new());
    let source = source();
    let reach: Reach<'_> = (&served, &http);
    let mut codeowned = Codeowned::new(reading(&source, Some(reach)), keeping(UNKEPT));
    let docs = changed(&["a.md"]);
    for _ in 0..2 {
        let areas =
            pollster::block_on(codeowned.areas(("g/p", Some("main"), Some(&docs)))).unwrap();
        let shown: Vec<(&str, AreaKind)> = areas
            .iter()
            .map(|area| (area.name.as_str(), area.kind))
            .collect();
        assert_eq!(
            shown,
            [
                ("@ana", AreaKind::Person),
                ("@gone", AreaKind::Name),
                ("@web", AreaKind::Group)
            ]
        );
    }
    assert_eq!(*served.read.lock().unwrap(), [Some("main".to_owned())]);
    assert_eq!(*served.looked.lock().unwrap(), ["@ana", "@gone", "@web"]);
    let mut unqueued = Codeowned::new(reading(&source, None), keeping(UNKEPT));
    let none = pollster::block_on(unqueued.areas(("g/p", Some("dev"), Some(&docs))));
    assert_eq!(none, Err(Unpolled::Unreadable(UNQUEUED.to_owned())));
}

#[test]
fn only_the_first_names_of_a_file_are_looked_up() {
    let written: Vec<String> = (0..=MOST_CODEOWNERS_LOOKUPS)
        .map(|at| format!("@n{at}"))
        .collect();
    let served = served(Some(format!("*.md {}\n", written.join(" "))), &[]);
    let http = ScriptedHttp::new(Vec::new());
    let source = source();
    let reach: Reach<'_> = (&served, &http);
    let mut codeowned = Codeowned::new(reading(&source, Some(reach)), keeping(UNKEPT));
    let areas =
        pollster::block_on(codeowned.areas(("g/p", None, Some(&changed(&["a.md"]))))).unwrap();
    assert_eq!(served.looked.lock().unwrap().len(), MOST_CODEOWNERS_LOOKUPS);
    let last = format!("@n{MOST_CODEOWNERS_LOOKUPS}");
    assert_eq!(
        areas
            .iter()
            .find(|area| area.name == last)
            .map(|area| area.kind),
        Some(AreaKind::Name)
    );
    assert_eq!(
        areas
            .iter()
            .filter(|area| area.kind == AreaKind::Group)
            .count(),
        MOST_CODEOWNERS_LOOKUPS
    );
}

#[test]
fn a_file_at_no_place_names_no_area_and_the_default_branch_is_read_where_the_source_says() {
    let served = served(None, &[]);
    let http = ScriptedHttp::new(Vec::new());
    let reach: Reach<'_> = (&served, &http);
    let docs = changed(&["a.md"]);
    let source = source_at(Some(herald_core::CodeownersRef::Default));
    let mut codeowned = Codeowned::new(reading(&source, Some(reach)), keeping(UNKEPT));
    for base in ["main", "dev"] {
        let areas = pollster::block_on(codeowned.areas(("g/p", Some(base), Some(&docs))));
        assert_eq!(areas, Ok(Vec::new()));
    }
    assert_eq!(*served.read.lock().unwrap(), [None]);
    let plain = Source::new(ForgeSource {
        forge: Forge::GitLab,
        options: SourceOptions {
            repository: Some("g/p".to_owned()),
            ..SourceOptions::default()
        },
        names: None,
    });
    let mut unzoned = Codeowned::new(reading(&plain, None), keeping(UNKEPT));
    let none = pollster::block_on(unzoned.areas(("g/p", None, Some(&docs))));
    assert_eq!(none, Ok(Vec::new()));
}

#[test]
fn a_table_names_areas_by_path_and_an_owner_of_known_kind_is_looked_up_never() {
    let table: Zoning = serde_json::from_value(serde_json::json!({ "docs": ["*.md"] })).unwrap();
    let tabled = Source::new(ForgeSource {
        forge: Forge::GitLab,
        options: SourceOptions {
            repository: Some("g/p".to_owned()),
            areas: Some(table),
            ..SourceOptions::default()
        },
        names: None,
    });
    let docs = changed(&["a.md"]);
    let mut by_table = Codeowned::new(reading(&tabled, None), keeping(UNKEPT));
    let areas = pollster::block_on(by_table.areas(("g/p", None, Some(&docs))));
    assert_eq!(areas.map(names), Ok(vec!["docs".to_owned()]));
    let served = served(Some("*.md @@maintainer @g/sub\n".to_owned()), &[]);
    let http = ScriptedHttp::new(Vec::new());
    let source = source();
    let mut codeowned = Codeowned::new(reading(&source, Some((&served, &http))), keeping(UNKEPT));
    let areas = pollster::block_on(codeowned.areas(("g/p", None, Some(&docs)))).unwrap();
    let shown: Vec<(&str, AreaKind)> = areas
        .iter()
        .map(|area| (area.name.as_str(), area.kind))
        .collect();
    assert_eq!(
        shown,
        [
            ("@@maintainer", AreaKind::Role),
            ("@g/sub", AreaKind::Group)
        ]
    );
    assert!(served.looked.lock().unwrap().is_empty());
}

#[test]
fn a_file_read_in_full_is_kept_and_asked_for_again_by_its_validator_the_next_round() {
    let mut served = served(Some("*.md @ana\n".to_owned()), &["@ana"]);
    let http = ScriptedHttp::new(Vec::new());
    let source = source();
    let store = FakeStore::default();
    let docs = changed(&["a.md"]);
    let pulls = store.pulls();
    for (repository, etag) in [
        ("G/P", "\"v1\""),
        ("g/p", "\"v1\""),
        ("g/p", "\"v2\""),
        ("g/p", "\"v2\""),
    ] {
        served.etag = Some(etag);
        let mut codeowned =
            Codeowned::new(reading(&source, Some((&served, &http))), keeping(pulls));
        let areas =
            pollster::block_on(codeowned.areas((repository, Some("main"), Some(&docs)))).unwrap();
        assert_eq!(names(areas), ["@ana"]);
    }
    let v1 = Some("\"v1\"".to_owned());
    assert_eq!(
        *served.asked.lock().unwrap(),
        [None, v1.clone(), v1, Some("\"v2\"".to_owned())]
    );
    let key = CodeownersKey {
        source: SourceName::new("forge"),
        repository: "g/p".to_owned(),
        reference: Some("main".to_owned()),
    };
    let kept = pollster::block_on(store.codeowners(&key)).unwrap().unwrap();
    assert_eq!(kept.value.etag, "\"v2\"");
    assert_eq!(kept.value.path, "CODEOWNERS");
    assert_eq!(kept.value.most, (128 * 1024, 200));
    assert_eq!(kept.version, herald_ports::Version(2));
}

#[test]
fn a_file_without_a_validator_or_with_no_store_is_read_in_full_each_round() {
    let unvalidated = served(Some("*.md @ana\n".to_owned()), &[]);
    let http = ScriptedHttp::new(Vec::new());
    let source = source();
    let store = FakeStore::default();
    let docs = changed(&["a.md"]);
    for _ in 0..2 {
        let reach: Reach<'_> = (&unvalidated, &http);
        let mut codeowned = Codeowned::new(reading(&source, Some(reach)), keeping(store.pulls()));
        pollster::block_on(codeowned.areas(("g/p", Some("main"), Some(&docs)))).unwrap();
    }
    assert_eq!(*unvalidated.asked.lock().unwrap(), [None, None]);
    let mut validated = served(Some("*.md @ana\n".to_owned()), &[]);
    validated.etag = Some("e");
    for _ in 0..2 {
        let reach: Reach<'_> = (&validated, &http);
        let mut codeowned = Codeowned::new(reading(&source, Some(reach)), keeping(UNKEPT));
        pollster::block_on(codeowned.areas(("g/p", Some("main"), Some(&docs)))).unwrap();
    }
    assert_eq!(*validated.asked.lock().unwrap(), [None, None]);
}

#[test]
fn a_store_that_cannot_keep_the_file_leaves_it_read_in_full() {
    let mut served = served(Some("*.md @ana\n".to_owned()), &["@ana"]);
    served.etag = Some("e");
    let http = ScriptedHttp::new(Vec::new());
    let source = source();
    let docs = changed(&["a.md"]);
    let refusing = Unkept;
    assert!(matches!(
        pollster::block_on(refusing.codeowners(&CodeownersKey {
            source: SourceName::new("forge"),
            repository: "g/p".to_owned(),
            reference: None,
        })),
        Err(StoreError::Unsupported { .. })
    ));
    for _ in 0..2 {
        let reach: Reach<'_> = (&served, &http);
        let mut codeowned = Codeowned::new(reading(&source, Some(reach)), keeping(Some(&refusing)));
        let areas = pollster::block_on(codeowned.areas(("g/p", None, Some(&docs))));
        assert_eq!(areas.map(names), Ok(vec!["@ana".to_owned()]));
    }
    assert_eq!(*served.asked.lock().unwrap(), [None, None]);
}

#[test]
fn a_gitlab_name_is_looked_up_on_each_source_s_own_forge_and_kept_apart_by_source() {
    let on_one = served(Some("*.md @web\n".to_owned()), &["@web"]);
    let on_other = served(Some("*.md @web\n".to_owned()), &[]);
    let http = ScriptedHttp::new(Vec::new());
    let (one, other) = (source(), source());
    let other_name = SourceName::new("other");
    let docs = changed(&["a.md"]);
    let store = FakeStore::default();
    let mut first = Codeowned::new(
        reading(&one, Some((&on_one, &http))),
        keeping(store.pulls()),
    );
    let mut second = Codeowned::new(
        (&other_name, &other, Some((&on_other, &http))),
        keeping(store.pulls()),
    );
    for _ in 0..2 {
        for (codeowned, kind) in [
            (&mut first, AreaKind::Person),
            (&mut second, AreaKind::Group),
        ] {
            let areas = pollster::block_on(codeowned.areas(("g/p", None, Some(&docs)))).unwrap();
            let shown: Vec<(&str, AreaKind)> = areas
                .iter()
                .map(|area| (area.name.as_str(), area.kind))
                .collect();
            assert_eq!(shown, [("@web", kind)]);
        }
    }
    assert_eq!(*on_one.looked.lock().unwrap(), ["@web"]);
    assert_eq!(*on_other.looked.lock().unwrap(), ["@web"]);
    assert_eq!(*on_one.read.lock().unwrap(), [None]);
    assert_eq!(*on_other.read.lock().unwrap(), [None]);
}
