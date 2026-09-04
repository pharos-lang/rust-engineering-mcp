//! Closed mirrors for domain-owned quality discriminants.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QualityProfile {
    Fast,
    Standard,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QualityStage {
    Format,
    Check,
    Clippy,
    Test,
    Audit,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Passed,
    Failed,
    Blocked,
    Unavailable,
    Cancelled,
}
#[derive(Serialize, JsonSchema)]
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
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditDataError {
    Unavailable,
    InvalidSnapshot,
    Integrity,
    MissingLockfile,
    InvalidLockfile,
    Budget,
    Cancelled,
    Timeout,
    SandboxDenied,
    UnsupportedPlatform,
    Internal,
}
#[derive(Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "code",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum QualityIssue {
    Operational(OperationalErrorCode),
    Audit(AuditDataError),
    Incomplete,
}
