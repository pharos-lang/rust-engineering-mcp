//! Typed contract boundary. JSON values exist only while crossing the SDK edge.
use std::{marker::PhantomData, sync::Arc};

use rmcp::model::{CallToolResult, ErrorData, JsonObject};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};

pub(super) trait ToolOutput: Serialize + JsonSchema {
    /// Project failures are valid results; only operational failures set isError.
    fn status(&self) -> rust_engineering_domain::ToolStatus;
}

pub(super) struct Contract<I, O> {
    pub input_schema: Arc<JsonObject>,
    pub output_schema: Arc<JsonObject>,
    input: jsonschema::Validator,
    output: jsonschema::Validator,
    types: PhantomData<fn(I) -> O>,
}

impl<I: DeserializeOwned + JsonSchema, O: ToolOutput> Contract<I, O> {
    pub fn new() -> Result<Self, ErrorData> {
        let input = serde_json::to_value(schemars::schema_for!(I)).map_err(|_| internal())?;
        let output = serde_json::to_value(
            schemars::generate::SchemaSettings::default()
                .for_serialize()
                .into_generator()
                .into_root_schema_for::<O>(),
        )
        .map_err(|_| internal())?;
        // Root object contracts are required by the product, even where MCP is broader.
        if !closed_object(&input) || !closed_object(&output) {
            return Err(internal());
        }
        Ok(Self {
            input_schema: Arc::new(input.as_object().ok_or_else(internal)?.clone()),
            output_schema: Arc::new(output.as_object().ok_or_else(internal)?.clone()),
            input: jsonschema::validator_for(&input).map_err(|_| internal())?,
            output: jsonschema::validator_for(&output).map_err(|_| internal())?,
            types: PhantomData,
        })
    }

    pub fn decode(&self, arguments: Option<JsonObject>) -> Result<I, ErrorData> {
        let value = serde_json::Value::Object(arguments.unwrap_or_default());
        if !self.input.is_valid(&value) {
            return Err(invalid_input());
        }
        // Schema constraints and Rust semantic invariants are both enforced.
        serde_json::from_value(value).map_err(|_| invalid_input())
    }

    pub fn encode(&self, output: O) -> Result<CallToolResult, ErrorData> {
        let is_error = matches!(
            output.status(),
            rust_engineering_domain::ToolStatus::Blocked
                | rust_engineering_domain::ToolStatus::Unavailable
                | rust_engineering_domain::ToolStatus::Cancelled
        );
        let value = serde_json::to_value(output).map_err(|_| internal())?;
        if !self.output.is_valid(&value) {
            return Err(internal());
        }
        // SDK constructors generate structured content and the identical JSON text.
        Ok(if is_error {
            CallToolResult::structured_error(value)
        } else {
            CallToolResult::structured(value)
        })
    }
}

// Nested object closure belongs to each DTO and its schema snapshot.
// Union fragments are not standalone objects and cannot all be closed recursively.
fn closed_object(schema: &serde_json::Value) -> bool {
    schema.get("type").and_then(|v| v.as_str()) == Some("object")
        && (schema.get("additionalProperties").and_then(|v| v.as_bool()) == Some(false)
            || schema
                .get("unevaluatedProperties")
                .and_then(|v| v.as_bool())
                == Some(false))
}

fn invalid_input() -> ErrorData {
    ErrorData::invalid_params("Invalid tool arguments", None)
}

fn internal() -> ErrorData {
    // Never reflect failed payloads or schema engine diagnostics to peers/logs.
    ErrorData::internal_error("Tool contract validation failed", None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::{Evidence, OutputEnvelope, Report, Truncation};
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct Input {
        #[schemars(length(min = 1, max = 16))]
        #[schemars(with = "String")]
        value: rust_engineering_domain::NonEmptyText,
    }

    // The fixture exercises the shared boundary without publishing an unimplemented tool.
    #[derive(Serialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct ResultDto {
        failed: bool,
        #[schemars(length(min = 1, max = 16))]
        summary: String,
    }
    impl ToolOutput for ResultDto {
        fn status(&self) -> rust_engineering_domain::ToolStatus {
            if self.failed {
                rust_engineering_domain::ToolStatus::Failed
            } else {
                rust_engineering_domain::ToolStatus::Passed
            }
        }
    }

    #[test]
    fn validates_schema_and_rust_input_without_reflection() -> Result<(), Box<dyn std::error::Error>>
    {
        let contract = Contract::<Input, ResultDto>::new()?;
        assert_eq!(
            contract
                .decode(Some(serde_json::from_value(json!({"value":"valid"}))?))?
                .value
                .as_str(),
            "valid"
        );
        for value in [
            json!({}),
            json!({"value":null}),
            json!({"value":"   "}),
            json!({"value":""}),
            json!({"value":17}),
            json!({"value":"synthetic_secret_very_long"}),
            json!({"value":"valid","extra":true}),
        ] {
            let error = contract
                .decode(Some(serde_json::from_value(value)?))
                .err()
                .ok_or("accepted invalid input")?;
            assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
            assert_eq!(error.message, "Invalid tool arguments");
            assert!(error.data.is_none());
        }
        assert!(contract.decode(None).is_err());
        Ok(())
    }

    #[test]
    fn project_failure_is_valid_structured_result_with_identical_text()
    -> Result<(), Box<dyn std::error::Error>> {
        let domain = OutputEnvelope::failed(Report {
            summary: "failed".parse()?,
            duration_ms: 1,
            data: (),
            diagnostics: vec![],
            truncation: Truncation::default(),
            evidence: Evidence::Local,
        });
        assert!(!domain.is_operational_error());
        let contract = Contract::<Input, ResultDto>::new()?;
        let result = contract.encode(ResultDto {
            failed: !domain.status().is_success(),
            summary: domain.summary().to_string(),
        })?;
        let wire = serde_json::to_value(result)?;
        assert_eq!(wire["isError"], false);
        assert_eq!(wire["structuredContent"]["failed"], true);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                wire["content"][0]["text"].as_str().ok_or("text missing")?
            )?,
            wire["structuredContent"]
        );
        let error = contract
            .encode(ResultDto {
                failed: false,
                summary: "synthetic_secret_very_long".into(),
            })
            .err()
            .ok_or("invalid output accepted")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert_eq!(error.message, "Tool contract validation failed");
        assert!(error.data.is_none());
        Ok(())
    }

    #[test]
    fn rejects_non_object_contracts_at_startup() {
        assert!(Contract::<String, ResultDto>::new().is_err());
    }
}
