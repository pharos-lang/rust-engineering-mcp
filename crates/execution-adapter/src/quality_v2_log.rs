//! Typed Supply chain facts only; source contents and guest diagnostics are excluded.
use rust_engineering_application::security::SecurityError;
use rust_engineering_domain::quality_v2::QualityV2Observation;

pub fn safe_gate_v2_log(
    observation: &QualityV2Observation,
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
        if !safe.report.trim_one() {
            return Err(SecurityError::OutputLimit);
        }
        findings_removed += 1;
    }
}
