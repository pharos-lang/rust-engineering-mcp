//! Typed scanner facts only; source contents and guest diagnostics are excluded.
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::unsafe_scan::UnsafeObservation;

pub fn safe_unsafe_log(
    observation: &UnsafeObservation,
) -> Result<crate::SafeSecurityLog, SecurityError> {
    if !observation.report.validate() {
        return Err(SecurityError::InvalidMetadata);
    }
    let mut safe = observation.clone();
    let mut findings_removed = 0;
    loop {
        let bytes = serde_json::to_vec(&safe).map_err(|_| SecurityError::InvalidMetadata)?;
        if bytes.len() <= 256 * 1024 {
            return Ok(crate::SafeSecurityLog {
                bytes,
                findings_removed,
            });
        }
        if safe.report.findings.pop().is_none() {
            return Err(SecurityError::OutputLimit);
        }
        findings_removed += 1;
        safe.report.findings_omitted += 1;
        safe.report.syntax_complete = false;
    }
}
