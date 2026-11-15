//! Build number of a Windows executable, from its `VS_FIXEDFILEINFO`
//! (version resource), without parsing the PE structure: the block starts
//! with a fixed signature and struct version, then the file version
//! (`major.minor` in the high dword, `patch.build` in the low dword).
//! `Wow.exe` 3.3.5a reads 3.3.5.12340.

use std::path::Path;

use crate::ClientError;

const SIGNATURE: [u8; 4] = 0xFEEF_04BDu32.to_le_bytes();
const STRUCT_VERSION: u32 = 0x0001_0000;

pub fn executable_build(path: &Path) -> Result<Option<u32>, ClientError> {
    let bytes = std::fs::read(path).map_err(ClientError::io(path))?;
    Ok(build_in(&bytes))
}

fn build_in(bytes: &[u8]) -> Option<u32> {
    let dword = |at: usize| -> Option<u32> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")))
    };
    let mut from = 0;
    while let Some(pos) = bytes[from..]
        .windows(4)
        .position(|w| w == SIGNATURE)
        .map(|p| p + from)
    {
        if dword(pos + 4) == Some(STRUCT_VERSION) {
            return dword(pos + 12).map(|ls| ls & 0xFFFF);
        }
        from = pos + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_info(ms: u32, ls: u32) -> Vec<u8> {
        let mut b = vec![0u8; 16];
        b.extend_from_slice(&SIGNATURE);
        b.extend_from_slice(&STRUCT_VERSION.to_le_bytes());
        b.extend_from_slice(&ms.to_le_bytes());
        b.extend_from_slice(&ls.to_le_bytes());
        b.extend_from_slice(&[0u8; 32]);
        b
    }

    #[test]
    fn reads_the_build_of_3_3_5a() {
        let bytes = fixed_info(3 << 16 | 3, 5 << 16 | 12340);
        assert_eq!(build_in(&bytes), Some(12340));
    }

    #[test]
    fn skips_a_signature_without_struct_version() {
        let mut bytes = SIGNATURE.to_vec();
        bytes.extend_from_slice(&[9, 9, 9, 9]);
        bytes.extend(fixed_info(1 << 16 | 12, 1 << 16 | 5875));
        assert_eq!(build_in(&bytes), Some(5875));
        assert_eq!(build_in(b"no version here"), None);
    }
}
