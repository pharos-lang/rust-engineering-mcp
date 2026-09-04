use crate::ProjectRef;
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, str::FromStr};

/// Syntax alone proves neither existence nor authorization.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ArtifactId(String);
impl ArtifactId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for ArtifactId {
    type Error = ArtifactError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.strip_prefix("art_").is_some_and(|s| {
            s.len() == 32
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) {
            Ok(Self(value))
        } else {
            Err(ArtifactError::InvalidId)
        }
    }
}
impl FromStr for ArtifactId {
    type Err = ArtifactError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.to_owned().try_into()
    }
}
impl From<ArtifactId> for String {
    fn from(value: ArtifactId) -> Self {
        value.0
    }
}
impl fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ArtifactMetadata {
    pub owner: ProjectRef,
    pub id: ArtifactId,
    /// SHA-256 of the stored, redacted bytes, never the original input.
    pub sha256: [u8; 32],
    pub size_bytes: u32,
    /// Capture may have dropped bytes. Exact input/output cap also reports true
    /// conservatively, without another potentially blocking EOF read.
    pub truncated: bool,
    /// Seconds from the injected clock's process-local monotonic origin.
    pub created_seconds: u64,
    pub expires_seconds: u64,
}

/// A view borrows the store, bounding retained content and preventing mutation.
pub struct ArtifactView<'a> {
    pub metadata: &'a ArtifactMetadata,
    pub content: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactError {
    InvalidId,
    InvalidLimits,
    InvalidSecret,
    QuotaExceeded,
    InputFailure,
    InvalidSourceCount,
    EntropyUnavailable,
    IdExhausted,
    NotFound,
    ClockRegression,
    ClockOverflow,
}
impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidId => "invalid artifact identifier",
            Self::InvalidLimits => "invalid artifact limits",
            Self::InvalidSecret => "invalid redaction configuration",
            Self::QuotaExceeded => "artifact quota exceeded",
            Self::InputFailure => "artifact input failed",
            Self::InvalidSourceCount => "invalid artifact input count",
            Self::EntropyUnavailable => "artifact entropy unavailable",
            Self::IdExhausted => "artifact identifier retries exhausted",
            Self::NotFound => "artifact not found",
            Self::ClockRegression => "artifact clock regressed",
            Self::ClockOverflow => "artifact expiry exceeds clock range",
        })
    }
}
impl Error for ArtifactError {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn id_is_canonical_and_serde_validated() {
        let valid = "art_0123456789abcdef0123456789abcdef";
        assert!(valid.parse::<ArtifactId>().is_ok());
        for bad in [
            "",
            "art_0",
            "art_0123456789ABCDEF0123456789abcdef",
            "prj_0123456789abcdef0123456789abcdef",
            "art_0123456789abcdef0123456789abcdef0",
        ] {
            assert_eq!(bad.parse::<ArtifactId>(), Err(ArtifactError::InvalidId));
            assert!(serde_json::from_str::<ArtifactId>(&format!("\"{bad}\"")).is_err());
        }
    }
}
