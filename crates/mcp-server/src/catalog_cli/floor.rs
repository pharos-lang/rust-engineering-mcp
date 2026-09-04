//! CLI uses the same trusted sequence record as read-only runtime observation.
pub(super) use rust_engineering_catalog::bundle::SequenceFloor as Floor;
impl From<rust_engineering_catalog::bundle::FloorError> for super::Error {
    fn from(error: rust_engineering_catalog::bundle::FloorError) -> Self {
        match error {
            rust_engineering_catalog::bundle::FloorError::InvalidState => Self::State,
            rust_engineering_catalog::bundle::FloorError::TrustMismatch => Self::TrustMismatch,
        }
    }
}
