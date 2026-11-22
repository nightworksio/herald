use std::sync::Mutex;

use herald_core::{
    Advising, Advisories, Capacity, Codeowners, Dialect, Overviews, Pull, Validators,
};
use herald_kinds::{Asked, Codeowning, Plan, Relisted};
use herald_ports::Boxed;
use herald_sources_forge::{Forge, ForgeSource, SourceOptions};
use herald_testing::ScriptedHttp;

use super::*;

/// A queue serving one CODEOWNERS file and telling each name by a list,
/// counting what it is asked.
#[derive(Debug)]
struct Served {
    file: String,
    people: &'static [&'static str],
    read: Mutex<usize>,
    looked: Mutex<Vec<String>>,
}

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
        _: (&'a str, Option<&'a str>),
        _: &'a Codeowning,
    ) -> Boxed<'a, Result<Option<Found>, Unpolled>> {
        *self.read.lock().unwrap() += 1;
        let found = Found {
            path: "CODEOWNERS".to_owned(),
            file: Codeowners::read(Dialect::GitLab, &self.file),
        };
        Box::pin(async move { Ok(Some(found)) })
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
    let options = SourceOptions {
        repository: Some("g/p".to_owned()),
        areas: Some(Zoning::Codeowners),
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
    let served = Served {
        file: "*.md @ana @web @gone\n*.rs @ana @unmet\n".to_owned(),
        people: &["@ana"],
        read: Mutex::new(0),
        looked: Mutex::new(Vec::new()),
    };
    let http = ScriptedHttp::new(Vec::new());
    let source = source();
    let mut codeowned = Codeowned::default();
    let reach: Reach<'_> = (&served, &http);
    let docs = changed(&["a.md"]);
    for _ in 0..2 {
        let areas = pollster::block_on(
            codeowned.areas((&source, Some(reach)), ("g/p", Some("main"), Some(&docs))),
        )
        .unwrap();
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
    assert_eq!(*served.read.lock().unwrap(), 1);
    assert_eq!(*served.looked.lock().unwrap(), ["@ana", "@gone", "@web"]);
    let none =
        pollster::block_on(codeowned.areas((&source, None), ("g/p", Some("dev"), Some(&docs))));
    assert_eq!(none, Err(Unpolled::Unreadable(UNQUEUED.to_owned())));
}

#[test]
fn only_the_first_names_of_a_file_are_looked_up() {
    let written: Vec<String> = (0..=MOST_CODEOWNERS_LOOKUPS)
        .map(|at| format!("@n{at}"))
        .collect();
    let served = Served {
        file: format!("*.md {}\n", written.join(" ")),
        people: &[],
        read: Mutex::new(0),
        looked: Mutex::new(Vec::new()),
    };
    let http = ScriptedHttp::new(Vec::new());
    let source = source();
    let mut codeowned = Codeowned::default();
    let reach: Reach<'_> = (&served, &http);
    let areas = pollster::block_on(codeowned.areas(
        (&source, Some(reach)),
        ("g/p", None, Some(&changed(&["a.md"]))),
    ))
    .unwrap();
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
