//! The isolated candidate producer is a real process/security boundary.
use crate::{InspectionControl, InspectionError};
use rust_engineering_domain::{RustMutationCommand, RustMutationObservation, SourceBundle};

pub trait ProjectMutationPort {
    fn mutate(
        &self,
        source: &SourceBundle,
        command: RustMutationCommand,
        control: &dyn InspectionControl,
    ) -> Result<RustMutationObservation, InspectionError>;
}
