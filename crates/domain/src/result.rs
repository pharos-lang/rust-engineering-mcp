use serde::{Deserialize, Serialize};

use crate::{ContractError, Diagnostic, Evidence, NonEmptyText};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Passed,
    Failed,
    Blocked,
    Unavailable,
    Cancelled,
}

impl ToolStatus {
    pub fn is_success(self) -> bool {
        self == Self::Passed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OperationalErrorCode {
    ProjectNotFound,
    InvalidProject,
    ToolNotInstalled,
    LockfileUpdateRequired,
    CommandTimeout,
    SandboxDenied,
    NetworkDenied,
    UnsupportedPlatform,
    OutputLimitExceeded,
}

impl OperationalErrorCode {
    pub fn status(self) -> ToolStatus {
        match self {
            Self::ToolNotInstalled | Self::UnsupportedPlatform => ToolStatus::Unavailable,
            Self::ProjectNotFound
            | Self::InvalidProject
            | Self::LockfileUpdateRequired
            | Self::CommandTimeout
            | Self::SandboxDenied
            | Self::NetworkDenied
            | Self::OutputLimitExceeded => ToolStatus::Blocked,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationalError {
    code: OperationalErrorCode,
    message: NonEmptyText,
}

impl OperationalError {
    pub fn new(code: OperationalErrorCode, message: NonEmptyText) -> Self {
        Self { code, message }
    }

    pub fn code(&self) -> OperationalErrorCode {
        self.code
    }

    pub fn message(&self) -> &NonEmptyText {
        &self.message
    }
}

/// Metadata only. The execution gateway will enforce streaming budgets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Truncation {
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub diagnostics_omitted: u64,
}

impl Truncation {
    pub fn is_truncated(self) -> bool {
        self.stdout_truncated || self.stderr_truncated || self.diagnostics_omitted != 0
    }
}

/// Typed report assembled by a use case before choosing its outcome.
pub struct Report<T> {
    pub summary: NonEmptyText,
    pub duration_ms: u64,
    pub data: T,
    pub diagnostics: Vec<Diagnostic>,
    pub truncation: Truncation,
    pub evidence: Evidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "RawEnvelope<T>",
    bound(deserialize = "T: Deserialize<'de>")
)]
pub struct OutputEnvelope<T> {
    status: ToolStatus,
    summary: NonEmptyText,
    duration_ms: u64,
    error_code: Option<OperationalErrorCode>,
    error_message: Option<NonEmptyText>,
    diagnostics: Vec<Diagnostic>,
    truncation: Truncation,
    data: T,
    evidence: Evidence,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnvelope<T> {
    status: ToolStatus,
    summary: NonEmptyText,
    duration_ms: u64,
    #[serde(deserialize_with = "crate::required_nullable")]
    error_code: Option<OperationalErrorCode>,
    #[serde(deserialize_with = "crate::required_nullable")]
    error_message: Option<NonEmptyText>,
    diagnostics: Vec<Diagnostic>,
    truncation: Truncation,
    data: T,
    evidence: Evidence,
}

impl<T> OutputEnvelope<T> {
    fn from_report(status: ToolStatus, error: Option<OperationalError>, report: Report<T>) -> Self {
        let (error_code, error_message) = match error {
            Some(error) => (Some(error.code), Some(error.message)),
            None => (None, None),
        };
        Self {
            status,
            error_code,
            error_message,
            summary: report.summary,
            duration_ms: report.duration_ms,
            data: report.data,
            diagnostics: report.diagnostics,
            truncation: report.truncation,
            evidence: report.evidence,
        }
    }

    pub fn passed(report: Report<T>) -> Self {
        Self::from_report(ToolStatus::Passed, None, report)
    }

    pub fn failed(report: Report<T>) -> Self {
        Self::from_report(ToolStatus::Failed, None, report)
    }

    pub fn cancelled(report: Report<T>) -> Self {
        Self::from_report(ToolStatus::Cancelled, None, report)
    }

    pub fn operational_error(error: OperationalError, report: Report<T>) -> Self {
        Self::from_report(error.code.status(), Some(error), report)
    }

    pub fn status(&self) -> ToolStatus {
        self.status
    }

    pub fn is_operational_error(&self) -> bool {
        self.error_code.is_some() || self.status == ToolStatus::Cancelled
    }

    pub fn summary(&self) -> &NonEmptyText {
        &self.summary
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    pub fn error_code(&self) -> Option<OperationalErrorCode> {
        self.error_code
    }

    pub fn error_message(&self) -> Option<&NonEmptyText> {
        self.error_message.as_ref()
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn truncation(&self) -> Truncation {
        self.truncation
    }

    pub fn data(&self) -> &T {
        &self.data
    }

    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }
}

impl<T> TryFrom<RawEnvelope<T>> for OutputEnvelope<T> {
    type Error = ContractError;

    fn try_from(raw: RawEnvelope<T>) -> Result<Self, Self::Error> {
        let valid = match (raw.status, raw.error_code, &raw.error_message) {
            (ToolStatus::Passed | ToolStatus::Failed | ToolStatus::Cancelled, None, None) => true,
            (status, Some(code), Some(_)) => status == code.status(),
            _ => false,
        };
        if !valid {
            return Err(ContractError::InconsistentOutcome);
        }
        Ok(Self {
            status: raw.status,
            summary: raw.summary,
            duration_ms: raw.duration_ms,
            error_code: raw.error_code,
            error_message: raw.error_message,
            diagnostics: raw.diagnostics,
            truncation: raw.truncation,
            data: raw.data,
            evidence: raw.evidence,
        })
    }
}
