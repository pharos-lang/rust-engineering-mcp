use std::{error::Error, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractError {
    InvalidProjectRef,
    InvalidDiagnosticCode,
    InvalidFingerprint,
    EmptyText,
    InvalidSpan,
    EmptySuggestion,
    InvalidFreshnessPolicy,
    InvalidProvenance,
    InconsistentFreshness,
    InconsistentOutcome,
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidDiagnosticCode => {
                "diagnostic code must be E followed by four ASCII digits"
            }
            Self::InvalidProjectRef => "invalid project reference format",
            Self::InvalidFingerprint => "invalid SHA-256 fingerprint format",
            Self::EmptyText => "text must contain non-whitespace characters",
            Self::InvalidSpan => "invalid source span coordinates",
            Self::EmptySuggestion => "suggestion requires at least one replacement",
            Self::InvalidFreshnessPolicy => "freshness thresholds must be strictly increasing",
            Self::InvalidProvenance => "observation cannot precede source creation",
            Self::InconsistentFreshness => "freshness does not match source, clock and policy",
            Self::InconsistentOutcome => "status and operational error fields are inconsistent",
        })
    }
}

impl Error for ContractError {}

fn canonical_hex(value: &str, prefix: &str, digits: usize) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == digits
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

macro_rules! string_value {
    ($name:ident, $validate:expr, $error:expr) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = ContractError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                if ($validate)(&value) {
                    Ok(Self(value))
                } else {
                    Err($error)
                }
            }
        }

        impl FromStr for $name {
            type Err = ContractError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::try_from(value.to_owned())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_value!(
    ProjectRef,
    |s: &str| canonical_hex(s, "prj_", 32),
    ContractError::InvalidProjectRef
);
string_value!(
    ProjectIdentityFingerprint,
    |s: &str| canonical_hex(s, "sha256:", 64),
    ContractError::InvalidFingerprint
);
string_value!(
    ExecutionFingerprint,
    |s: &str| canonical_hex(s, "sha256:", 64),
    ContractError::InvalidFingerprint
);
string_value!(
    NonEmptyText,
    |s: &str| !s.trim().is_empty(),
    ContractError::EmptyText
);

string_value!(
    CatalogFingerprint,
    |s: &str| canonical_hex(s, "sha256:", 64),
    ContractError::InvalidFingerprint
);

string_value!(
    SourceFingerprint,
    |s: &str| canonical_hex(s, "sha256:", 64),
    ContractError::InvalidFingerprint
);

string_value!(
    DiagnosticCode,
    |s: &str| s.len() == 5
        && s.starts_with('E')
        && s.as_bytes()[1..].iter().all(u8::is_ascii_digit),
    ContractError::InvalidDiagnosticCode
);
