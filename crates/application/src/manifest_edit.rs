use rust_engineering_domain::{ManifestEdit, ManifestEditError};

/// Pure transformation; does not confer filesystem authority or Cargo validity.
pub trait ManifestEditor {
    fn apply(&self, before: &[u8], edit: &ManifestEdit) -> Result<Vec<u8>, ManifestEditError>;
}
