use super::*;
use rust_engineering_application::{ExecutionCancellation, OperationControl};
use serde_json::{Value, json};
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Continue;
impl OperationControl for Continue {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}
impl ExecutionCancellation for Continue {
    fn is_cancelled(&self) -> bool {
        false
    }
}
fn success() -> Result<Output, Box<dyn std::error::Error>> {
    Ok(output(
        Ok(rust_engineering_application::catalog_context(
            &CatalogProvider::new(None, None),
            &WallClock,
            &Continue,
        )
        .map_err(|e| format!("{e:?}"))?),
        0,
    )?)
}
#[test]
fn status_contract_closes_input_components_and_nullable_reservation() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    contract.decode(None)?;
    for argument in [
        json!({"sync":true}),
        json!({"store":"/tmp"}),
        json!({"model":null}),
    ] {
        assert!(
            contract
                .decode(Some(serde_json::from_value(argument)?))
                .is_err()
        );
    }
    let value = encode_bounded(&contract, success()?)?;
    assert_eq!(value.is_error, Some(false));
    let wire = serde_json::to_value(value)?;
    let content = &wire["structuredContent"];
    assert_eq!(
        serde_json::from_str::<Value>(wire["content"][0]["text"].as_str().ok_or("text")?)?,
        *content
    );
    let validator =
        jsonschema::validator_for(&serde_json::to_value(contract.output_schema.as_ref())?)?;
    for pointer in [
        "",
        "/data",
        "/data/network",
        "/data/context",
        "/data/context/catalog",
        "/data/context/model",
        "/data/context/semantic_index",
        "/data/context/rustsec",
        "/truncation",
        "/evidence",
    ] {
        let mut extra = content.clone();
        extra
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .insert("unexpected".into(), json!(true));
        assert!(!validator.is_valid(&extra), "open object {pointer}");
    }
    let mut missing = content.clone();
    missing["data"]["context"]
        .as_object_mut()
        .ok_or("object")?
        .remove("reservation");
    assert!(!validator.is_valid(&missing));
    Ok(())
}
#[test]
fn availability_errors_cancel_and_complete_encoded_budget() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    for error in [
        ProjectError::Cancelled,
        ProjectError::Rejected(OperationalErrorCode::CommandTimeout),
        ProjectError::Rejected(OperationalErrorCode::SandboxDenied),
        ProjectError::Rejected(OperationalErrorCode::OutputLimitExceeded),
    ] {
        let encoded = contract.encode(output(Err(error), 1)?)?;
        assert_eq!(encoded.is_error, Some(true));
        assert!(encoded.structured_content.ok_or("content")?["data"].is_null());
    }
    assert!(output(Err(ProjectError::Internal), 0).is_err());
    let mut large = success()?;
    static TEXT: [u8; MAX_RESULT / 2] = [b'x'; MAX_RESULT / 2];
    large.summary = std::str::from_utf8(&TEXT)?;
    assert!(serde_json::to_vec(&large)?.len() < MAX_RESULT);
    let encoded = encode_bounded(&contract, large)?;
    assert!(serde_json::to_vec(&encoded)?.len() <= MAX_RESULT);
    assert_eq!(
        encoded.structured_content.ok_or("content")?["error_code"],
        "OUTPUT_LIMIT_EXCEEDED"
    );
    for signal in [WorkerError::Cancelled, WorkerError::TimedOut] {
        assert!(
            joined_result(Joined {
                result: Ok(()),
                interrupted: Some(signal)
            })
            .is_err()
        );
    }
    assert_eq!(
        joined_result::<()>(Joined {
            result: Err(ProjectError::Internal),
            interrupted: Some(WorkerError::Cancelled)
        }),
        Err(ProjectError::Internal)
    );
    Ok(())
}
