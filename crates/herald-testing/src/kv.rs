use std::collections::BTreeMap;
use std::sync::Mutex;

use herald_ports::{Kv, KvError, Pair, Stamped, Versionstamp};

use crate::lock;

/// A key-value store in memory, with versionstamps counting up from one.
#[derive(Debug, Default)]
pub struct FakeKv {
    state: Mutex<(u64, BTreeMap<Vec<String>, Stamped>)>,
}

impl Kv for FakeKv {
    async fn get(&self, key: &[String]) -> Result<Option<Stamped>, KvError> {
        Ok(lock(&self.state).1.get(key).cloned())
    }

    async fn set_if(
        &self,
        key: &[String],
        value: Vec<u8>,
        expected: Option<Versionstamp>,
    ) -> Result<Option<Versionstamp>, KvError> {
        let mut state = lock(&self.state);
        let current = state.1.get(key).map(|stamped| stamped.versionstamp.clone());
        if current != expected {
            return Ok(None);
        }
        state.0 += 1;
        let versionstamp = Versionstamp(format!("{:020}", state.0));
        state.1.insert(
            key.to_vec(),
            Stamped {
                value,
                versionstamp: versionstamp.clone(),
            },
        );
        Ok(Some(versionstamp))
    }

    async fn delete(&self, key: &[String]) -> Result<(), KvError> {
        lock(&self.state).1.remove(key);
        Ok(())
    }

    async fn list(&self, prefix: &[String]) -> Result<Vec<Pair>, KvError> {
        Ok(lock(&self.state)
            .1
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(key, stamped)| Pair {
                key: key.clone(),
                value: stamped.value.clone(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    #[test]
    fn a_value_is_set_only_against_its_current_versionstamp() {
        let kv = FakeKv::default();
        let first = pollster::block_on(kv.set_if(&key(&["a"]), b"1".to_vec(), None)).unwrap();
        assert!(first.is_some());
        assert_eq!(
            pollster::block_on(kv.set_if(&key(&["a"]), b"2".to_vec(), None)).unwrap(),
            None
        );
        let second =
            pollster::block_on(kv.set_if(&key(&["a"]), b"2".to_vec(), first.clone())).unwrap();
        assert!(second.is_some() && second != first);
        let stamped = pollster::block_on(kv.get(&key(&["a"]))).unwrap().unwrap();
        assert_eq!(
            (stamped.value, Some(stamped.versionstamp)),
            (b"2".to_vec(), second)
        );
    }

    #[test]
    fn keys_are_listed_by_prefix_and_deleted() {
        let kv = FakeKv::default();
        for parts in [&["p", "1"][..], &["p", "2"], &["q", "1"]] {
            pollster::block_on(kv.set_if(&key(parts), parts.concat().into_bytes(), None)).unwrap();
        }
        let listed = pollster::block_on(kv.list(&key(&["p"]))).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|pair| pair.key.clone())
                .collect::<Vec<_>>(),
            [key(&["p", "1"]), key(&["p", "2"])]
        );
        pollster::block_on(kv.delete(&key(&["p", "1"]))).unwrap();
        assert_eq!(pollster::block_on(kv.get(&key(&["p", "1"]))).unwrap(), None);
        assert_eq!(pollster::block_on(kv.list(&key(&["p"]))).unwrap().len(), 1);
    }
}
