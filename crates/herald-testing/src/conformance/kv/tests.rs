use herald_ports::Versionstamp;

use super::*;
use crate::FakeKv;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    FindsWhatWasNeverSet,
    SetsNothing,
    ReadsAnotherValue,
    OverwritesAsAbsent,
    RefusesItsOwnStamp,
    KeepsItsStamp,
    SetsAgainstAStaleStamp,
    DeletesAgainstAStaleStamp,
    ChangesOnARefusal,
    DeletesNothing,
    KeepsWhatItDeletes,
    ListsWhatItDeleted,
    DeletesTwice,
    TakesAStampBack,
    MatchesPartOfAName,
    ListsWithoutTheValue,
    ListsEverythingUnderNothing,
    Down,
}

struct Faulty {
    fault: Fault,
    inner: FakeKv,
}

fn faulty(fault: Fault) -> Faulty {
    Faulty {
        fault,
        inner: FakeKv::default(),
    }
}

impl Faulty {
    fn down(&self) -> Result<(), KvError> {
        match self.fault {
            Fault::Down => Err(KvError::Unavailable {
                reason: "down".to_owned(),
            }),
            _ => Ok(()),
        }
    }
}

impl Kv for Faulty {
    async fn get(&self, key: &[String]) -> Result<Option<Stamped>, KvError> {
        self.down()?;
        let found = self.inner.get(key).await?;
        Ok(match self.fault {
            Fault::FindsWhatWasNeverSet => Some(Stamped {
                value: Vec::new(),
                versionstamp: Versionstamp("0".to_owned()),
            }),
            Fault::ReadsAnotherValue => found.map(|stamped| Stamped {
                value: b"other".to_vec(),
                ..stamped
            }),
            _ => found,
        })
    }

    async fn set_if(
        &self,
        key: &[String],
        value: Vec<u8>,
        expected: Option<Versionstamp>,
    ) -> Result<Option<Versionstamp>, KvError> {
        let current = self.inner.get(key).await?.map(|found| found.versionstamp);
        match self.fault {
            Fault::SetsNothing => Ok(None),
            Fault::RefusesItsOwnStamp if expected.is_some() => Ok(None),
            Fault::OverwritesAsAbsent if expected.is_none() && current.is_some() => Ok(current),
            Fault::SetsAgainstAStaleStamp if expected.is_some() && expected != current => {
                self.inner.set_if(key, value, current).await
            }
            Fault::KeepsItsStamp if expected.is_some() => {
                self.inner.set_if(key, value, expected.clone()).await?;
                Ok(expected)
            }
            Fault::TakesAStampBack if value == b"again" && current.is_none() => {
                self.inner.set_if(key, value, None).await?;
                Ok(Some(Versionstamp(format!("{:020}", 2))))
            }
            _ => self.inner.set_if(key, value, expected).await,
        }
    }

    async fn delete_if(&self, key: &[String], expected: Versionstamp) -> Result<bool, KvError> {
        let current = self.inner.get(key).await?.map(|found| found.versionstamp);
        let held = current.as_ref() == Some(&expected);
        match self.fault {
            Fault::DeletesAgainstAStaleStamp if current.is_some() && !held => Ok(true),
            Fault::ChangesOnARefusal if current.is_some() && !held => {
                self.inner.set_if(key, b"changed".to_vec(), current).await?;
                Ok(false)
            }
            Fault::DeletesNothing => Ok(false),
            Fault::KeepsWhatItDeletes => Ok(held),
            Fault::DeletesTwice if current.is_none() => Ok(true),
            _ => self.inner.delete_if(key, expected).await,
        }
    }

    async fn list(&self, prefix: &[String]) -> Result<Vec<Pair>, KvError> {
        let every = self.inner.list(&[]).await?;
        let under: Vec<Pair> = every
            .iter()
            .filter(|pair| pair.key.starts_with(prefix))
            .cloned()
            .collect();
        Ok(match self.fault {
            Fault::MatchesPartOfAName => every
                .into_iter()
                .filter(|pair| pair.key.join("/").starts_with(&prefix.join("/")))
                .collect(),
            Fault::ListsWithoutTheValue => under
                .into_iter()
                .map(|pair| Pair {
                    value: Vec::new(),
                    ..pair
                })
                .collect(),
            Fault::ListsEverythingUnderNothing if under.is_empty() => every,
            Fault::ListsWhatItDeleted if under.is_empty() => vec![Pair {
                key: prefix.to_vec(),
                value: Vec::new(),
            }],
            _ => under,
        })
    }
}

fn run(fault: Fault) -> Result<(), Failure> {
    pollster::block_on(kv_conformance(&faulty(fault)))
}

#[test]
fn the_fake_key_value_store_passes() {
    assert_eq!(
        pollster::block_on(kv_conformance(&FakeKv::default())),
        Ok(())
    );
}

#[test]
fn a_key_value_store_that_breaks_its_promises_is_refused() {
    let cases = [
        (Fault::FindsWhatWasNeverSet, "a key never set was found"),
        (Fault::SetsNothing, "a key was not set where it was absent"),
        (
            Fault::ReadsAnotherValue,
            "a value set was not read back as set, at its versionstamp",
        ),
        (
            Fault::OverwritesAsAbsent,
            "a key was set as absent where it holds a value",
        ),
        (
            Fault::RefusesItsOwnStamp,
            "a value was not set against its versionstamp",
        ),
        (
            Fault::KeepsItsStamp,
            "a value set again kept its versionstamp",
        ),
        (
            Fault::SetsAgainstAStaleStamp,
            "a value was set against a versionstamp it no longer has",
        ),
        (
            Fault::DeletesAgainstAStaleStamp,
            "a key was deleted against a versionstamp it no longer has",
        ),
        (
            Fault::ChangesOnARefusal,
            "a refused write or delete changed the value",
        ),
        (
            Fault::DeletesNothing,
            "a key was not deleted against its versionstamp",
        ),
        (Fault::KeepsWhatItDeletes, "a deleted key was still found"),
        (Fault::ListsWhatItDeleted, "a deleted key was still listed"),
        (Fault::DeletesTwice, "a deleted key was deleted again"),
        (
            Fault::TakesAStampBack,
            "a deleted key was not set again where it was absent, at a new versionstamp",
        ),
        (
            Fault::MatchesPartOfAName,
            "a prefix did not list every key under it, with its value, and nothing else",
        ),
        (
            Fault::ListsWithoutTheValue,
            "a prefix did not list every key under it, with its value, and nothing else",
        ),
        (
            Fault::ListsEverythingUnderNothing,
            "a prefix nothing lies under listed a key",
        ),
        (
            Fault::Down,
            "the key-value store failed: the key-value store could not be reached: down",
        ),
    ];
    for (fault, failure) in cases {
        assert_eq!(run(fault), Err(Failure(failure.to_owned())), "{failure}");
    }
}
