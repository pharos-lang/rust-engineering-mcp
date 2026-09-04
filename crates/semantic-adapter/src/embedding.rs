use crate::{E5_REVISION, VerifiedE5Bundle};
use fastembed::{
    InitOptionsUserDefined, Pooling, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel,
};
use rust_engineering_application::EmbeddingProvider;
use rust_engineering_domain::*;
use std::sync::{Arc, OnceLock};

/// Explicit host startup ownership of the process-wide ORT environment. Failure
/// to own its configuration fails closed; later calls reuse our own configuration.
pub struct OfflineRuntime(());
impl OfflineRuntime {
    pub fn initialize() -> Result<Self, SemanticError> {
        static OWNED: OnceLock<bool> = OnceLock::new();
        let configured = *OWNED.get_or_init(|| {
            ort::init()
                .with_telemetry(false)
                .with_logger(Arc::new(|_, _, _, _, _| {}))
                .commit()
        });
        if configured {
            Ok(Self(()))
        } else {
            Err(SemanticError::Inference)
        }
    }
}
pub struct LocalEmbeddingProvider {
    identity: EmbeddingIdentity,
    engine: TextEmbedding,
}
impl LocalEmbeddingProvider {
    pub fn load(_: &OfflineRuntime, bundle: VerifiedE5Bundle) -> Result<Self, SemanticError> {
        let runtime = format!(
            "fastembed6.0.2/ort2.0.0-rc.13/{}/{}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            ort::info()
        );
        if runtime.len() > 4096 {
            return Err(SemanticError::InvalidArtifact);
        }
        let identity = EmbeddingIdentity {
            model: "intfloat/multilingual-e5-small".to_owned(),
            revision: E5_REVISION.to_owned(),
            artifact_fingerprint: bundle.fingerprint,
            runtime,
            // This describes local verification, not the earlier publisher download.
            // The development provisioning receipt records that separate network use.
            provenance: Provenance::new(
                SourceKind::EmbeddingModel,
                format!("local-verified-e5:{E5_REVISION}")
                    .parse()
                    .map_err(|_| SemanticError::InvalidArtifact)?,
                None,
                None,
                IntegrityStatus::Verified,
                false,
            )
            .map_err(|_| SemanticError::InvalidArtifact)?,
            dimension: 384,
            max_tokens: 512,
            intra_threads: 2,
            pooling: PoolingKind::Mean,
            normalization: Normalization::L2,
        };
        let [
            onnx,
            tokenizer_file,
            config_file,
            special_tokens_map_file,
            tokenizer_config_file,
        ] = bundle.files;
        let files = TokenizerFiles {
            tokenizer_file,
            config_file,
            special_tokens_map_file,
            tokenizer_config_file,
        };
        let model = UserDefinedEmbeddingModel::new(onnx, files).with_pooling(Pooling::Mean);
        let options = InitOptionsUserDefined::new()
            .with_max_length(512)
            .with_intra_threads(2);
        let engine = TextEmbedding::try_new_from_user_defined(model, options)
            .map_err(|_| SemanticError::Inference)?;
        Ok(Self { identity, engine })
    }
    fn embed(
        &mut self,
        text: &str,
        prefix: &str,
        maximum: usize,
    ) -> Result<Vec<f32>, SemanticError> {
        if text.trim().is_empty() || text.len() > maximum || text.chars().any(char::is_control) {
            return Err(SemanticError::InvalidInput);
        }
        let mut vectors = self
            .engine
            .embed([format!("{prefix}: {text}")], Some(1))
            .map_err(|_| SemanticError::Inference)?;
        if vectors.len() != 1 {
            return Err(SemanticError::Inference);
        }
        let vector = vectors.pop().ok_or(SemanticError::Inference)?;
        validate_embedding(&vector, self.identity.dimension)
            .map_err(|_| SemanticError::Inference)?;
        Ok(vector)
    }
}
impl EmbeddingProvider for LocalEmbeddingProvider {
    fn identity(&self) -> &EmbeddingIdentity {
        &self.identity
    }
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, SemanticError> {
        self.embed(text, "query", 256)
    }
    fn embed_passage(&mut self, text: &str) -> Result<Vec<f32>, SemanticError> {
        if text.len() > 8192 || text.chars().any(|c| c.is_control() && !c.is_whitespace()) {
            return Err(SemanticError::InvalidInput);
        }
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        self.embed(&normalized, "passage", 8192)
    }
}
