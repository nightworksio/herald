//! New RSA keys for herald's ActivityPub actors, made on AWS-LC (ADR-0004,
//! ADR-0102).

use aws_lc_rs::encoding::AsDer as _;
use aws_lc_rs::rsa::{KeyPair, KeySize};
use herald_keys::PRIVATE_KEY;
use herald_ports::{Algorithm, Secret};
use zeroize::Zeroizing;

use crate::RingSigner;

/// How many bits a new RSA key's modulus holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Bits {
    /// 2048, as Mastodon's own keys are.
    #[default]
    Rsa2048,
    /// 3072.
    Rsa3072,
    /// 4096.
    Rsa4096,
}

impl Bits {
    /// Every size, the smallest first.
    pub const ALL: [Self; 3] = [Self::Rsa2048, Self::Rsa3072, Self::Rsa4096];

    /// How many bits it is.
    #[must_use]
    pub const fn bits(self) -> u16 {
        match self {
            Self::Rsa2048 => 2048,
            Self::Rsa3072 => 3072,
            Self::Rsa4096 => 4096,
        }
    }

    const fn size(self) -> KeySize {
        match self {
            Self::Rsa2048 => KeySize::Rsa2048,
            Self::Rsa3072 => KeySize::Rsa3072,
            Self::Rsa4096 => KeySize::Rsa4096,
        }
    }
}

/// A new RSA key: the private key, as PKCS#8 PEM in a buffer zeroed when
/// dropped, and its `SubjectPublicKeyInfo` in DER, as herald's signer reads
/// it back from the private key.
pub struct Made {
    /// The private key, as PKCS#8 PEM.
    pub private: Zeroizing<String>,
    /// The public key, as its DER `SubjectPublicKeyInfo`.
    pub public: Vec<u8>,
}

impl std::fmt::Debug for Made {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Made")
            .field("public", &self.public)
            .finish_non_exhaustive()
    }
}

/// Why no key was made.
pub const UNMADE: &str = "the system's random generator or AWS-LC could not make an RSA key";

/// A new RSA key of `bits`, which herald's signer reads back before it is
/// handed over, so a key it could not sign with is never written.
///
/// # Errors
///
/// Where AWS-LC makes no key, or herald's signer cannot read the one made.
pub fn rsa_key(bits: Bits) -> Result<Made, &'static str> {
    let pair = KeyPair::generate(bits.size()).map_err(|_| UNMADE)?;
    let der = pair.as_der().map_err(|_| UNMADE)?;
    let der = der.as_ref();
    let mut private = Zeroizing::new(String::with_capacity(der.len() * 2 + 64));
    herald_keys::armour(der, PRIVATE_KEY, &mut private);
    let secret = Secret::new("activitypub keygen", private.as_str());
    let public = RingSigner::default()
        .public_half(Algorithm::RsaSha256, &secret)
        .ok_or("herald's signer could not read the RSA key AWS-LC made")?;
    Ok(Made { private, public })
}

#[cfg(test)]
mod tests {
    use herald_keys::{Public, subject};
    use herald_ports::{Signer as _, Verifier as _};
    use ring::signature::RsaKeyPair;

    use super::*;

    #[test]
    fn a_key_made_signs_as_herald_signs_and_its_public_key_is_the_one_it_signs_for() {
        for bits in Bits::ALL {
            let made = rsa_key(bits).unwrap();
            let mut empty = String::new();
            herald_keys::armour(&[], PRIVATE_KEY, &mut empty);
            let (begin, end) = empty.split_once('\n').unwrap();
            assert!(made.private.starts_with(begin));
            assert!(made.private.ends_with(end));
            let der = herald_keys::private_key(&made.private).unwrap();
            let pair = RsaKeyPair::from_pkcs8(&der).unwrap();
            assert_eq!(pair.public().modulus_len() * 8, usize::from(bits.bits()));
            assert_eq!(made.public, subject(Public::Rsa, pair.public().as_ref()));
            let secret = Secret::new("KEY", made.private.as_str());
            let signature = pollster::block_on(RingSigner::default().sign(
                Algorithm::RsaSha256,
                &secret,
                b"message",
            ))
            .unwrap();
            assert!(pollster::block_on(crate::RingVerifier.verify(
                Algorithm::RsaSha256,
                pair.public().as_ref(),
                b"message",
                &signature
            )));
            let read = format!("{made:?}");
            assert!(read.starts_with("Made { public: ["), "{read}");
            assert!(read.ends_with(", .. }"), "{read}");
            assert!(!read.contains("PRIVATE"), "{read}");
        }
    }

    #[test]
    fn each_size_is_its_bits() {
        assert_eq!(Bits::ALL.map(Bits::bits), [2048, 3072, 4096]);
        assert_eq!(Bits::default(), Bits::Rsa2048);
    }
}
