//! How Web Crypto takes herald's keys, and the keys it publishes.

use std::fmt;

use herald_keys::{PRIVATE_KEY, Public, jwk_bytes, pem, rsa_public, subject};
use herald_ports::{Algorithm, Secret};
use zeroize::Zeroizing;

/// How Web Crypto takes one key: the format its bytes are in, the
/// algorithm's name, and the hash it signs over, where it names one. Its
/// `Debug` shows the key's length alone.
#[derive(Clone, PartialEq, Eq)]
pub struct Imported {
    /// The format: `raw`, `pkcs8` or `spki`.
    pub format: &'static str,
    /// The algorithm's name.
    pub name: &'static str,
    /// The hash, where the algorithm takes one.
    pub hash: Option<&'static str>,
    /// The key's bytes, overwritten with zeroes when dropped.
    pub data: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for Imported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Imported")
            .field("format", &self.format)
            .field("name", &self.name)
            .field("hash", &self.hash)
            .field("data", &format_args!("{} bytes", self.data.len()))
            .finish()
    }
}

const HMAC: &str = "HMAC";
const ED25519: &str = "Ed25519";
const RSA: &str = "RSASSA-PKCS1-v1_5";
const SHA_256: &str = "SHA-256";

fn named(algorithm: Algorithm) -> (&'static str, Option<&'static str>) {
    match algorithm {
        Algorithm::HmacSha256 => (HMAC, Some(SHA_256)),
        Algorithm::Ed25519 => (ED25519, None),
        Algorithm::RsaSha256 => (RSA, Some(SHA_256)),
    }
}

/// The key `key` signs with under `algorithm`: an HMAC's secret as it is,
/// and any other key as the PKCS#8 its PEM holds.
#[must_use]
pub fn signing(algorithm: Algorithm, key: &Secret) -> Option<Imported> {
    let (name, hash) = named(algorithm);
    let (format, data) = match algorithm {
        Algorithm::HmacSha256 => ("raw", key.expose().as_bytes().to_vec()),
        Algorithm::Ed25519 | Algorithm::RsaSha256 => ("pkcs8", pem(key.expose(), PRIVATE_KEY)?),
    };
    let data = Zeroizing::new(data);
    Some(Imported {
        format,
        name,
        hash,
        data,
    })
}

/// The key a signature under `algorithm` is checked with: an HMAC's shared
/// secret and an Ed25519 key as they are, and an RSA key's PKCS#1 inside the
/// `SubjectPublicKeyInfo` Web Crypto takes.
#[must_use]
pub fn verifying(algorithm: Algorithm, key: &[u8]) -> Imported {
    let (name, hash) = named(algorithm);
    let (format, data) = match algorithm {
        Algorithm::HmacSha256 | Algorithm::Ed25519 => ("raw", key.to_vec()),
        Algorithm::RsaSha256 => ("spki", subject(Public::Rsa, key)),
    };
    Imported {
        format,
        name,
        hash,
        data: Zeroizing::new(data),
    }
}

/// The public members of a private key's JSON Web Key.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Jwk {
    /// An Ed25519 key's public point.
    pub x: Option<String>,
    /// An RSA key's modulus.
    pub n: Option<String>,
    /// An RSA key's public exponent.
    pub e: Option<String>,
}

/// The `SubjectPublicKeyInfo` of the key whose JSON Web Key is `jwk`, under
/// `algorithm`; an HMAC's secret has none.
#[must_use]
pub fn published(algorithm: Algorithm, jwk: &Jwk) -> Option<Vec<u8>> {
    match algorithm {
        Algorithm::HmacSha256 => None,
        Algorithm::Ed25519 => Some(subject(Public::Ed25519, &jwk_bytes(jwk.x.as_deref()?)?)),
        Algorithm::RsaSha256 => {
            let modulus = jwk_bytes(jwk.n.as_deref()?)?;
            let exponent = jwk_bytes(jwk.e.as_deref()?)?;
            Some(subject(Public::Rsa, &rsa_public(&modulus, &exponent)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEM: &str = "-----BEGIN PRIVATE KEY-----\nAAEC\n-----END PRIVATE KEY-----\n";

    #[test]
    fn a_signing_key_is_its_secret_for_hmac_and_its_pkcs8_otherwise() {
        let hmac = signing(Algorithm::HmacSha256, &Secret::new("K", "Jefe")).unwrap();
        assert_eq!(
            hmac,
            Imported {
                format: "raw",
                name: "HMAC",
                hash: Some("SHA-256"),
                data: Zeroizing::new(b"Jefe".to_vec()),
            }
        );
        assert_eq!(
            format!("{hmac:?}"),
            "Imported { format: \"raw\", name: \"HMAC\", hash: Some(\"SHA-256\"), data: 4 bytes }"
        );
        let key = Secret::new("K", PEM);
        let ed = signing(Algorithm::Ed25519, &key).unwrap();
        assert_eq!((ed.format, ed.name, ed.hash), ("pkcs8", "Ed25519", None));
        assert_eq!(*ed.data, [0, 1, 2]);
        let rsa = signing(Algorithm::RsaSha256, &key).unwrap();
        assert_eq!(
            (rsa.format, rsa.name, rsa.hash),
            ("pkcs8", "RSASSA-PKCS1-v1_5", Some("SHA-256"))
        );
        assert_eq!(
            signing(Algorithm::Ed25519, &Secret::new("K", "not a key!")),
            None
        );
    }

    #[test]
    fn a_verifying_key_is_raw_but_for_rsa_whose_pkcs1_is_wrapped() {
        assert_eq!(verifying(Algorithm::HmacSha256, b"s").format, "raw");
        let ed = verifying(Algorithm::Ed25519, &[9; 32]);
        assert_eq!((ed.format, ed.data.to_vec()), ("raw", vec![9; 32]));
        let rsa = verifying(Algorithm::RsaSha256, &[1, 2]);
        assert_eq!(rsa.format, "spki");
        assert_eq!(*rsa.data, subject(Public::Rsa, &[1, 2]));
    }

    #[test]
    fn a_published_key_is_built_from_its_jwk_s_public_members() {
        let ed = Jwk {
            x: Some("BwcH".to_owned()),
            ..Jwk::default()
        };
        assert_eq!(
            published(Algorithm::Ed25519, &ed),
            Some(subject(Public::Ed25519, &[7, 7, 7]))
        );
        let rsa = Jwk {
            n: Some("AQI".to_owned()),
            e: Some("AQAB".to_owned()),
            ..Jwk::default()
        };
        assert_eq!(
            published(Algorithm::RsaSha256, &rsa),
            Some(subject(Public::Rsa, &rsa_public(&[1, 2], &[1, 0, 1])))
        );
        assert_eq!(published(Algorithm::HmacSha256, &rsa), None);
        assert_eq!(published(Algorithm::Ed25519, &Jwk::default()), None);
        assert_eq!(published(Algorithm::RsaSha256, &ed), None);
    }
}
