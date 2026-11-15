//! SRP6 registration data, as `Acore::Crypto::SRP6::CalculateVerifier`
//! (fork `src/common/Cryptography/Authentication/SRP6.cpp`):
//!
//! ```text
//! v = g ^ H(s || H(UPPER(user) || ':' || UPPER(pass))) mod N
//! ```
//!
//! with SHA-1, g = 7, the 256-bit N below, and every number stored
//! little-endian (salt, hashes read as numbers, 32-byte verifier).

use num_bigint::BigUint;
use sha1::{Digest, Sha1};

pub const SALT_LENGTH: usize = 32;
pub const VERIFIER_LENGTH: usize = 32;

const G: u32 = 7;
/// Big-endian hex of N, as written in `SRP6.cpp`.
const N_HEX: &[u8] = b"894B645E89E1535BBDAD5B8B290650530801B18EBFBF5E8FAB3C82872A3E9BB7";

/// Verifier for an already upper-cased username and password.
pub fn verifier(username: &str, password: &str, salt: &[u8; SALT_LENGTH]) -> [u8; VERIFIER_LENGTH] {
    let inner = Sha1::new()
        .chain_update(username.as_bytes())
        .chain_update(b":")
        .chain_update(password.as_bytes())
        .finalize();
    let x = Sha1::new()
        .chain_update(salt)
        .chain_update(inner)
        .finalize();
    let n = BigUint::parse_bytes(N_HEX, 16).expect("valid constant");
    let v = BigUint::from(G).modpow(&BigUint::from_bytes_le(&x), &n);
    let mut out = [0u8; VERIFIER_LENGTH];
    let bytes = v.to_bytes_le(); // < N, so at most 32 bytes
    out[..bytes.len()].copy_from_slice(&bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made by the fork's worldserver (`account create SRPTEST testpass` in
    /// its console, 2026-11-15), read back from `acore_auth.account`.
    #[test]
    fn matches_an_account_created_by_worldserver() {
        let salt: [u8; SALT_LENGTH] =
            hex::decode("7BEC1B5CBB5C03EA421E3AC81E3788D094AEB78D8FB774CBF661A175E809B65F")
                .unwrap()
                .try_into()
                .unwrap();
        let expected =
            hex::decode("B21FBEDD867E8D2E3A78346D66C622DB766769EC6D13DEC4A7EA323D3C6B0242")
                .unwrap();
        assert_eq!(verifier("SRPTEST", "TESTPASS", &salt).to_vec(), expected);
    }

    #[test]
    fn verifier_is_below_n_and_depends_on_every_input() {
        let salt = [1u8; SALT_LENGTH];
        let v = verifier("PLAYER", "SECRET", &salt);
        let n = BigUint::parse_bytes(N_HEX, 16).unwrap();
        assert!(BigUint::from_bytes_le(&v) < n);
        assert_ne!(v, verifier("PLAYER", "SECRET2", &salt));
        assert_ne!(v, verifier("PLAYER2", "SECRET", &salt));
        assert_ne!(v, verifier("PLAYER", "SECRET", &[2u8; SALT_LENGTH]));
    }
}
