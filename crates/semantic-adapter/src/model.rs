use rust_engineering_domain::{CatalogFingerprint, SemanticError};
use sha2::{Digest, Sha256};

pub const E5_REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";
pub const E5_FILES: [(&str, usize, &str); 5] = [
    (
        "model.onnx",
        470268510,
        "ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665",
    ),
    (
        "tokenizer.json",
        17082730,
        "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39",
    ),
    (
        "config.json",
        653,
        "bbb7c1333fc4b3e27fbc9cd5d2070aabcc1d4dfb99917c3633e772f97545a6b6",
    ),
    (
        "special_tokens_map.json",
        167,
        "d05497f1da52c5e09554c0cd874037a083e1dc1b9cfd48034d1c717f1afc07a7",
    ),
    (
        "tokenizer_config.json",
        443,
        "a1d6bc8734a6f635dc158508bef000f8e2e5a759c7d92f984b2c86e5ff53425b",
    ),
];

/// Exact immutable E5 distribution. No path, URL, download or caller-supplied
/// expected hash is accepted. Bytes are checked before native/JSON parsing.
pub struct VerifiedE5Bundle {
    pub(crate) files: [Vec<u8>; 5],
    pub(crate) fingerprint: CatalogFingerprint,
}
impl VerifiedE5Bundle {
    pub fn verify(files: [Vec<u8>; 5]) -> Result<Self, SemanticError> {
        let mut manifest = Sha256::new();
        for (bytes, (name, size, expected)) in files.iter().zip(E5_FILES) {
            if bytes.len() != size || hex(&Sha256::digest(bytes)) != expected {
                return Err(SemanticError::InvalidArtifact);
            }
            manifest.update(name.as_bytes());
            manifest.update([0]);
            manifest.update(expected.as_bytes());
        }
        let fingerprint = format!("sha256:{}", hex(&manifest.finalize()))
            .parse()
            .map_err(|_| SemanticError::InvalidArtifact)?;
        Ok(Self { files, fingerprint })
    }
    pub fn fingerprint(&self) -> &CatalogFingerprint {
        &self.fingerprint
    }
    pub fn byte_length(&self) -> usize {
        self.files.iter().map(Vec::len).sum()
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_absent_and_wrong_bytes_before_parsing() {
        assert!(matches!(
            VerifiedE5Bundle::verify(Default::default()),
            Err(SemanticError::InvalidArtifact)
        ));
        let mut files: [Vec<u8>; 5] = Default::default();
        files[0] = b"not an ONNX model".to_vec();
        assert!(matches!(
            VerifiedE5Bundle::verify(files),
            Err(SemanticError::InvalidArtifact)
        ));
    }
}
