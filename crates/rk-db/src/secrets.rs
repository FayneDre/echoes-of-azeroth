use crate::DbError;

const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";

/// Random password of `len` characters (letters and digits only, so it never
/// needs quoting in SQL, option files or `.conf` files). 32 chars ≈ 186 bits.
pub fn generate_password(len: usize) -> Result<String, DbError> {
    // Rejection sampling keeps the distribution uniform over the alphabet.
    let limit = 256 - (256 % ALPHABET.len());
    let mut out = String::with_capacity(len);
    let mut buffer = [0u8; 64];
    while out.len() < len {
        getrandom::fill(&mut buffer).map_err(|e| DbError::Random(e.to_string()))?;
        for &byte in buffer.iter().filter(|&&b| (b as usize) < limit) {
            if out.len() == len {
                break;
            }
            out.push(ALPHABET[byte as usize % ALPHABET.len()] as char);
        }
    }
    Ok(out)
}

/// Database and user names are interpolated into SQL: only `[A-Za-z0-9_]`,
/// 1 to 32 characters (MySQL's user name limit), are accepted.
pub fn validate_identifier(name: &str) -> Result<(), DbError> {
    let ok = (1..=32).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(DbError::InvalidIdentifier(name.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_random_and_safe() {
        let a = generate_password(32).unwrap();
        let b = generate_password(32).unwrap();
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(a.bytes().all(|c| ALPHABET.contains(&c)));
    }

    #[test]
    fn identifiers_are_restricted() {
        assert!(validate_identifier("acore_world").is_ok());
        for bad in ["", "a-b", "a b", "x`; DROP", &"a".repeat(33)] {
            assert!(validate_identifier(bad).is_err(), "{bad}");
        }
    }
}
