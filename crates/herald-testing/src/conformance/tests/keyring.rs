use herald_ports::{Keyring, KnownKey, StoreError};
use jiff::Timestamp;

use super::broken;
use crate::conformance::{Failure, keyring_conformance};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Forgetting {
    Correctly,
    Nothing,
    Everything,
    TheFirstKnown,
    OneKeyForAll,
    Broken,
}

struct FaultyKeyring {
    fault: Forgetting,
    keys: std::sync::Mutex<std::collections::BTreeMap<String, KnownKey>>,
}

impl FaultyKeyring {
    fn new(fault: Forgetting) -> Self {
        Self {
            fault,
            keys: std::sync::Mutex::default(),
        }
    }

    fn id(&self, id: &str) -> String {
        match self.fault {
            Forgetting::OneKeyForAll => "one".to_owned(),
            _ => id.to_owned(),
        }
    }

    fn broken(&self) -> Result<(), StoreError> {
        match self.fault {
            Forgetting::Broken => Err(broken()),
            _ => Ok(()),
        }
    }
}

impl Keyring for FaultyKeyring {
    async fn known(&self, id: &str) -> Result<Option<KnownKey>, StoreError> {
        self.broken()?;
        Ok(crate::lock(&self.keys).get(&self.id(id)).cloned())
    }

    async fn know(&self, id: &str, key: &KnownKey) -> Result<(), StoreError> {
        let mut keys = crate::lock(&self.keys);
        let id = self.id(id);
        if self.fault == Forgetting::TheFirstKnown && keys.contains_key(&id) {
            return Ok(());
        }
        keys.insert(id, key.clone());
        Ok(())
    }

    async fn forget_before(&self, before: Timestamp) -> Result<(), StoreError> {
        let before = match self.fault {
            Forgetting::Nothing => Timestamp::MIN,
            Forgetting::Everything => Timestamp::MAX,
            _ => before,
        };
        crate::lock(&self.keys).retain(|_, key| key.fetched >= before);
        Ok(())
    }
}

fn keyring(fault: Forgetting) -> Result<(), Failure> {
    pollster::block_on(keyring_conformance(&FaultyKeyring::new(fault)))
}

#[test]
fn a_keyring_that_keeps_its_promises_passes() {
    assert_eq!(keyring(Forgetting::Correctly), Ok(()));
}

#[test]
fn a_keyring_that_breaks_its_promises_is_refused() {
    let cases = [
        (
            Forgetting::Nothing,
            "a key read before the expiry was still known",
        ),
        (
            Forgetting::Everything,
            "a key read at the expiry was forgotten",
        ),
        (
            Forgetting::TheFirstKnown,
            "a key's id did not name the key known last",
        ),
        (
            Forgetting::OneKeyForAll,
            "a key's id did not name the key known last",
        ),
        (
            Forgetting::Broken,
            "the store failed: the store could not be reached: broken",
        ),
    ];
    for (fault, failure) in cases {
        assert_eq!(keyring(fault), Err(Failure(failure.to_owned())));
    }
}
