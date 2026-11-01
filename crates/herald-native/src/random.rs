use herald_core::hex;
use ring::rand::{SecureRandom, SystemRandom};

/// How many random bytes a new secret holds: 256 bits.
const SECRET_BYTES: usize = 32;

/// A new secret of 256 random bits from the system's generator, in
/// lowercase hex, as `herald init` prints a marker secret (ADR-0011); none
/// where the generator fails.
#[must_use]
pub fn random_secret() -> Option<String> {
    let mut bytes = [0_u8; SECRET_BYTES];
    SystemRandom::new().fill(&mut bytes).ok()?;
    Some(hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_256_random_bits_in_hex_and_new_each_time() {
        let first = random_secret().unwrap();
        assert_eq!(first.len(), 64);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        assert_ne!(random_secret().unwrap(), first);
    }
}
