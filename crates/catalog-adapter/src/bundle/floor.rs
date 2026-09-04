//! Trusted local reservation, independent from the active payload and signing key.
use super::{PublisherTrust, VerifiedBundle, sha256};
use serde::{Deserialize, Serialize};

const MAX_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloorError {
    InvalidState,
    TrustMismatch,
}
impl std::fmt::Display for FloorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "catalog sequence floor: {self:?}")
    }
}
impl std::error::Error for FloorError {}

// Preserve the original CLI record's field order and serialization exactly.
// Deserialize is private: callers cannot bypass SequenceFloor::parse validation.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    format_version: u32,
    publisher: String,
    channel: String,
    sequence: u64,
    bundle_sha256: String,
    checksum: String,
}

pub struct SequenceFloor {
    record: Record,
}
impl SequenceFloor {
    pub fn new(bundle: &VerifiedBundle) -> Self {
        let mut value = Self {
            record: Record {
                format_version: 1,
                publisher: bundle.manifest().publisher.clone(),
                channel: bundle.manifest().channel.clone(),
                sequence: bundle.manifest().sequence,
                bundle_sha256: bundle.fingerprint().to_owned(),
                checksum: String::new(),
            },
        };
        value.record.checksum = value.digest();
        value
    }
    fn digest(&self) -> String {
        sha256(
            format!(
                "catalog-floor-v1\0{}\0{}\0{}\0{}",
                self.record.publisher,
                self.record.channel,
                self.record.sequence,
                self.record.bundle_sha256
            )
            .as_bytes(),
        )
    }
    pub fn parse(bytes: &[u8], trust: &PublisherTrust) -> Result<Self, FloorError> {
        if bytes.len() > MAX_BYTES {
            return Err(FloorError::InvalidState);
        }
        let record: Record = serde_json::from_slice(bytes).map_err(|_| FloorError::InvalidState)?;
        let value = Self { record };
        if value.record.format_version != 1
            || value.record.sequence == 0
            || value.record.sequence > i64::MAX as u64
            || value.record.bundle_sha256.len() != 64
            || !value
                .record
                .bundle_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || value.record.checksum != value.digest()
            || value.bytes()? != bytes
        {
            return Err(FloorError::InvalidState);
        }
        if value.record.publisher != trust.publisher || value.record.channel != trust.channel {
            return Err(FloorError::TrustMismatch);
        }
        Ok(value)
    }
    pub fn bytes(&self) -> Result<Vec<u8>, FloorError> {
        serde_json::to_vec(&self.record).map_err(|_| FloorError::InvalidState)
    }
    pub fn sequence(&self) -> u64 {
        self.record.sequence
    }
    pub fn bundle_sha256(&self) -> &str {
        &self.record.bundle_sha256
    }
    pub fn publisher(&self) -> &str {
        &self.record.publisher
    }
    pub fn channel(&self) -> &str {
        &self.record.channel
    }
    pub fn matches(&self, bundle: &VerifiedBundle) -> bool {
        self.sequence() == bundle.manifest().sequence
            && self.bundle_sha256() == bundle.fingerprint()
    }
    pub fn permits(&self, bundle: &VerifiedBundle) -> bool {
        bundle.manifest().sequence > self.sequence() || self.matches(bundle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TRUST: &[u8] = include_bytes!("../../../../fixtures/catalog/fixture-trust.json");
    const ONE: &[u8] = include_bytes!("../../../../fixtures/catalog/fixture-1.tar.zst");
    const TWO: &[u8] = include_bytes!("../../../../fixtures/catalog/fixture-2.tar.zst");
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn original_cli_wire_format_and_rotation_identity_are_preserved() -> TestResult {
        let trust = PublisherTrust::parse(TRUST)?;
        let bundle = super::super::verify(ONE, &trust)?;
        let floor = SequenceFloor::new(&bundle);
        let hash = sha256(ONE);
        let checksum =
            sha256(format!("catalog-floor-v1\0fixture-only\0test\0{}\0{hash}", 1).as_bytes());
        let expected = format!(
            "{{\"format_version\":1,\"publisher\":\"fixture-only\",\"channel\":\"test\",\"sequence\":1,\"bundle_sha256\":\"{hash}\",\"checksum\":\"{checksum}\"}}"
        );
        assert_eq!(floor.bytes()?, expected.as_bytes());
        let mut rotated = PublisherTrust::parse(TRUST)?;
        rotated.public_key = "01".repeat(32);
        let reopened = SequenceFloor::parse(&floor.bytes()?, &rotated)?;
        assert_eq!(
            (
                reopened.publisher(),
                reopened.channel(),
                reopened.sequence(),
                reopened.bundle_sha256()
            ),
            ("fixture-only", "test", 1, hash.as_str())
        );
        assert!(reopened.matches(&bundle));
        assert!(reopened.permits(&super::super::verify(TWO, &trust)?));
        let newer = SequenceFloor::new(&super::super::verify(TWO, &trust)?);
        assert!(!newer.permits(&bundle));
        Ok(())
    }

    #[test]
    fn mismatch_corruption_noncanonical_and_oversized_records_fail_closed() -> TestResult {
        let trust = PublisherTrust::parse(TRUST)?;
        let floor = SequenceFloor::new(&super::super::verify(ONE, &trust)?);
        let bytes = floor.bytes()?;
        for field in ["publisher", "channel"] {
            let mut other = PublisherTrust::parse(TRUST)?;
            if field == "publisher" {
                other.publisher = "other".into();
            } else {
                other.channel = "other".into();
            }
            assert!(matches!(
                SequenceFloor::parse(&bytes, &other),
                Err(FloorError::TrustMismatch)
            ));
        }
        let text = String::from_utf8(bytes.clone())?;
        for bad in [
            text.replace("\"sequence\":1", "\"sequence\":2"),
            text.replace("\"checksum\":\"", "\"checksum\":\"0"),
            format!("{text}\n"),
            text.replace("\"format_version\":1", "\"format_version\":2"),
            text.replace("\"sequence\":1", "\"sequence\":0"),
            text.replace("\"sequence\":1", "\"sequence\":18446744073709551615"),
            text.replace("{", "{\"extra\":1,"),
        ] {
            assert!(matches!(
                SequenceFloor::parse(bad.as_bytes(), &trust),
                Err(FloorError::InvalidState)
            ));
        }
        assert!(matches!(
            SequenceFloor::parse(&vec![b' '; 4097], &trust),
            Err(FloorError::InvalidState)
        ));
        Ok(())
    }
}
