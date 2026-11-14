//! Signed artifact index (ADR 0010).
//!
//! ```text
//! index.json = {"format": 1, "keyId": "<16 hex>", "signature": "<128 hex>", "payload": "<JSON text>"}
//! signed message = DOMAIN ++ payload (UTF-8 bytes, exactly as published)
//! ```
//!
//! Pure: parsing, verification, anti-rollback and selection. Fetching the
//! file and keeping the last accepted copy belong to the caller.

use std::collections::BTreeMap;

use ed25519_dalek::{Signature, VerifyingKey};
use eoa_core::Catalog;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Prefix of every signed message: a signature made for anything else
/// (another file, another version of this format) never verifies here.
pub const DOMAIN: &[u8] = b"echoes-of-azeroth/index/v1\n";

/// Where CI publishes the index: asset of the fixed `index` release.
pub const DEFAULT_URL: &str =
    "https://github.com/FayneDre/echoes-of-azeroth/releases/download/index/index.json";

/// Public keys trusted to sign the index, compiled in (`trusted-keys.json`,
/// also read by `tools/ci/index.mjs`).
const TRUSTED_KEYS: &str = include_str!("../trusted-keys.json");

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("the index is not valid JSON: {0}")]
    Json(String),
    #[error("unsupported index format {0}: update the launcher")]
    Format(u32),
    #[error("the index is signed with an unknown key {0}")]
    UnknownKey(String),
    #[error("the index signature is invalid")]
    BadSignature,
    #[error(
        "the index is older (serial {offered}) than the one already accepted (serial {accepted})"
    )]
    Rollback { offered: u64, accepted: u64 },
    #[error("the index has the serial {0} of the accepted one but another content")]
    SerialReused(u64),
    #[error("no published server build matches {emulator}/{flavor} on {platform} with this catalog's pins")]
    NoBuild {
        emulator: String,
        flavor: String,
        platform: String,
    },
    #[error("unknown {0}")]
    Unknown(String),
}

/// A public key the launcher trusts.
#[derive(Debug, Clone)]
pub struct TrustedKey {
    pub id: String,
    pub key: VerifyingKey,
}

impl TrustedKey {
    /// From the raw 32-byte public key; the id is derived from it.
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self, IndexError> {
        let key = VerifyingKey::from_bytes(bytes).map_err(|_| IndexError::BadSignature)?;
        Ok(Self {
            id: key_id(bytes),
            key,
        })
    }
}

/// First 16 hex digits of the SHA-256 of the raw public key.
pub fn key_id(public_key: &[u8; 32]) -> String {
    hex::encode(Sha256::digest(public_key))[..16].to_owned()
}

/// The keys compiled into this build.
pub fn trusted_keys() -> Vec<TrustedKey> {
    #[derive(Deserialize)]
    struct Entry {
        #[serde(rename = "publicKey")]
        public_key: String,
    }
    #[derive(Deserialize)]
    struct File {
        keys: Vec<Entry>,
    }
    let file: File = serde_json::from_str(TRUSTED_KEYS).expect("trusted-keys.json is valid");
    file.keys
        .iter()
        .map(|entry| {
            let bytes: [u8; 32] = hex::decode(&entry.public_key)
                .ok()
                .and_then(|b| b.try_into().ok())
                .expect("trusted key is 32 bytes of hex");
            TrustedKey::from_bytes(&bytes).expect("trusted key is a valid ed25519 point")
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    format: u32,
    key_id: String,
    signature: String,
    payload: String,
}

/// Verified content of the index. Unknown fields are ignored: CI may add
/// some without breaking older launchers (`format` changes when they matter).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    pub format: u32,
    pub serial: u64,
    pub published: String,
    pub builds: Vec<Build>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Build {
    pub emulator: String,
    pub flavor: String,
    pub platform: String,
    pub build_key: String,
    pub core: Pin,
    /// Every module compiled in: id -> commit.
    pub modules: BTreeMap<String, String>,
    pub server: Artifact,
    pub db_snapshot: Option<SnapshotArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub repo: String,
    pub commit: String,
}

/// A downloadable file: mirrors in order, checked by size and sha256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub urls: Vec<String>,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotArtifact {
    #[serde(flatten)]
    pub file: Artifact,
    pub mysql: SnapshotMysql,
    /// Modules whose SQL the snapshot holds (dbimport names).
    pub modules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotMysql {
    pub id: String,
    pub version: String,
}

/// Verifies the envelope with `keys` and returns its payload.
pub fn open(envelope: &[u8], keys: &[TrustedKey]) -> Result<Index, IndexError> {
    let envelope: Envelope =
        serde_json::from_slice(envelope).map_err(|e| IndexError::Json(e.to_string()))?;
    if envelope.format != 1 {
        return Err(IndexError::Format(envelope.format));
    }
    let key = keys
        .iter()
        .find(|k| k.id == envelope.key_id)
        .ok_or_else(|| IndexError::UnknownKey(envelope.key_id.clone()))?;
    let signature: [u8; 64] = hex::decode(&envelope.signature)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or(IndexError::BadSignature)?;
    let mut message = DOMAIN.to_vec();
    message.extend_from_slice(envelope.payload.as_bytes());
    key.key
        .verify_strict(&message, &Signature::from_bytes(&signature))
        .map_err(|_| IndexError::BadSignature)?;
    let index: Index =
        serde_json::from_str(&envelope.payload).map_err(|e| IndexError::Json(e.to_string()))?;
    if index.format != 1 {
        return Err(IndexError::Format(index.format));
    }
    Ok(index)
}

/// Anti-rollback: `offered` may replace `accepted` only with a higher
/// serial, or the same serial and the same content.
pub fn check_newer(offered: &Index, accepted: Option<&Index>) -> Result<(), IndexError> {
    let Some(accepted) = accepted else {
        return Ok(());
    };
    if offered.serial < accepted.serial {
        return Err(IndexError::Rollback {
            offered: offered.serial,
            accepted: accepted.serial,
        });
    }
    if offered.serial == accepted.serial && offered != accepted {
        return Err(IndexError::SerialReused(offered.serial));
    }
    Ok(())
}

/// The build to install for `emulator`/`flavor` on `platform`: same pins as
/// the catalog (flavor source, every compatible module's commit). The last
/// listed of several wins (newest recipe).
pub fn select<'a>(
    index: &'a Index,
    catalog: &Catalog,
    emulator: &str,
    flavor: &str,
    platform: &str,
) -> Result<&'a Build, IndexError> {
    let entry = catalog
        .emulator(emulator)
        .ok_or_else(|| IndexError::Unknown(format!("emulator '{emulator}'")))?;
    let manifest = entry
        .flavors
        .get(flavor)
        .ok_or_else(|| IndexError::Unknown(format!("flavor '{flavor}'")))?;
    let core = Pin {
        repo: manifest.source.repo.clone(),
        commit: manifest.source.commit.clone(),
    };
    let modules: BTreeMap<String, String> = entry
        .modules
        .values()
        .filter(|m| m.compatible_flavors.iter().any(|f| f == flavor))
        .map(|m| (m.id.clone(), m.source.commit.clone()))
        .collect();
    index
        .builds
        .iter()
        .rev()
        .find(|b| {
            b.emulator == emulator
                && b.flavor == flavor
                && b.platform == platform
                && b.core == core
                && b.modules == modules
        })
        .ok_or_else(|| IndexError::NoBuild {
            emulator: emulator.to_owned(),
            flavor: flavor.to_owned(),
            platform: platform.to_owned(),
        })
}
