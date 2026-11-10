use std::collections::BTreeMap;
use std::future::ready;

use herald_ports::{DynSql, Erased, Kv as _, Secrets as _, Sql as _};
use herald_testing::{FakeClock, FakeKv, FakeSecrets, ScriptedSql};
use jiff::Timestamp;

use super::*;

/// A host with the variables and secrets it is given, and nothing else.
pub(crate) struct Bare {
    vars: BTreeMap<String, String>,
    secrets: FakeSecrets,
}

impl Opening for Bare {
    fn connect(&self, base: &str) -> Result<Owned<dyn DynHttp + Send>, String> {
        Err(format!("{base} is out of reach"))
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    fn secrets(&self) -> &dyn DynSecrets {
        &self.secrets
    }

    fn clock(&self) -> Owned<dyn Clock> {
        Owned::new(Box::new(FakeClock::at(Timestamp::UNIX_EPOCH)))
    }

    fn nonce(&self) -> String {
        "nonce".to_owned()
    }

    fn sql<'a>(&'a self, _: SqlAt<'a>) -> Local<'a, Result<Owned<dyn DynSql>, String>> {
        Box::pin(ready(Ok(Owned::new(
            Box::new(ScriptedSql::default()) as Box<dyn DynSql>
        ))))
    }

    fn kv<'a>(&'a self, _: KvAt<'a>) -> Local<'a, Result<Owned<dyn DynKv>, String>> {
        Box::pin(ready(Ok(Owned::new(
            Box::new(FakeKv::default()) as Box<dyn DynKv>
        ))))
    }
}

pub(crate) fn bare(vars: &[(&str, &str)]) -> Bare {
    Bare {
        vars: vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        secrets: FakeSecrets::default().with("NAMED", "t"),
    }
}

#[test]
fn a_secret_the_platform_does_not_set_says_what_needs_it() {
    let host = bare(&[]);
    assert_eq!(
        pollster::block_on(host.secret("NAMED", "the store pushes with it")),
        Ok(Secret::new("NAMED", "t"))
    );
    assert_eq!(
        pollster::block_on(host.secret("MISSING", "the store pushes with it")),
        Err("MISSING is not set; the store pushes with it".to_owned())
    );
}

#[test]
fn a_store_s_repository_is_its_own_else_the_run_s() {
    let run = bare(&[(GITHUB_REPOSITORY, "cli/cli")]);
    assert_eq!(run.repository(Some("own/repo")), Ok("own/repo"));
    assert_eq!(run.repository(None), Ok("cli/cli"));
    assert_eq!(
        bare(&[]).repository(None),
        Err("GITHUB_REPOSITORY is not set; name the store's repository, or run herald in the repository's CI".to_owned())
    );
}

#[test]
fn what_a_host_opens_answers_as_the_ports_it_erases() {
    let host = bare(&[]);
    assert_eq!(
        host.connect("https://api.example").err(),
        Some("https://api.example is out of reach".to_owned())
    );
    assert_eq!(host.clock().now(), Timestamp::UNIX_EPOCH);
    assert_eq!(host.nonce(), "nonce");
    let sql = pollster::block_on(host.sql(SqlAt::D1("DB"))).unwrap();
    let unscripted = pollster::block_on(sql.transaction(Vec::new())).unwrap_err();
    assert!(
        unscripted.to_string().ends_with("no answer was scripted"),
        "{unscripted}"
    );
    let kv = pollster::block_on(host.kv(KvAt::DenoKv(None))).unwrap();
    assert_eq!(pollster::block_on(kv.get(&["a".to_owned()])), Ok(None));
    assert_eq!(
        pollster::block_on(kv.list(&["a".to_owned()])),
        Ok(Vec::new())
    );
    let set = pollster::block_on(kv.set_if(&["a".to_owned()], b"v".to_vec(), None))
        .unwrap()
        .unwrap();
    assert_eq!(
        pollster::block_on(kv.delete_if(&["a".to_owned()], set)),
        Ok(true)
    );
    assert_eq!(
        pollster::block_on(Erased(host.secrets()).secret("NAMED")),
        Ok(Secret::new("NAMED", "t"))
    );
}

#[test]
fn a_store_that_cannot_be_opened_says_which() {
    assert_eq!(
        unopened("d1", "refused"),
        "the d1 store could not be opened: refused"
    );
}
