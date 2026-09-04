use rust_engineering_domain::{
    Applicability, ByteRange, ContractError, Diagnostic, DiagnosticSource, NonEmptyText, Position,
    Replacement, Severity, SourceSpan, Suggestion,
};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn span(start: (u32, u32), end: (u32, u32)) -> Result<SourceSpan, ContractError> {
    SourceSpan::new(
        "src/日本語.rs".parse()?,
        Position::new(start.0, start.1)?,
        Position::new(end.0, end.1)?,
        None,
        true,
        None,
    )
}

#[test]
fn coordinates_and_byte_offsets_reject_invalid_ranges() -> TestResult {
    for (line, column) in [(0, 1), (1, 0), (0, 0)] {
        assert_eq!(Position::new(line, column), Err(ContractError::InvalidSpan));
        assert!(serde_json::from_value::<Position>(json!({"line":line,"column":column})).is_err());
    }
    for (start, end) in [((2, 1), (1, 9)), ((1, 9), (1, 8))] {
        assert_eq!(span(start, end), Err(ContractError::InvalidSpan));
        let mut value = serde_json::to_value(span((1, 1), (1, 1))?)?;
        value["start"] = json!({"line":start.0,"column":start.1});
        value["end"] = json!({"line":end.0,"column":end.1});
        assert!(serde_json::from_value::<SourceSpan>(value).is_err());
    }
    assert_eq!(ByteRange::new(4, 3), Err(ContractError::InvalidSpan));
    assert!(serde_json::from_value::<ByteRange>(json!({"start":4,"end":3})).is_err());
    assert!(serde_json::from_value::<ByteRange>(json!({"start":-1,"end":3})).is_err());
    assert_eq!(ByteRange::new(0, 0)?.start(), 0);
    assert_eq!(ByteRange::new(u64::MAX, u64::MAX)?.end(), u64::MAX);
    Ok(())
}

#[test]
fn multiline_positions_and_optional_independent_bytes_roundtrip() -> TestResult {
    let value = SourceSpan::new(
        "src/日本語.rs".parse()?,
        Position::new(1, 90)?,
        Position::new(2, 1)?,
        Some(ByteRange::new(0, 4)?),
        false,
        Some("préstamo\n変更".parse()?),
    )?;
    assert_eq!(value.file().as_str(), "src/日本語.rs");
    assert_eq!(value.start().column.get(), 90);
    assert_eq!(value.end().line.get(), 2);
    assert_eq!(value.bytes(), Some(ByteRange::new(0, 4)?));
    assert!(!value.is_primary());
    assert_eq!(
        value.label().map(NonEmptyText::as_str),
        Some("préstamo\n変更")
    );
    assert_eq!(
        serde_json::from_str::<SourceSpan>(&serde_json::to_string(&value)?)?,
        value
    );
    Ok(())
}

#[test]
fn locationless_diagnostic_preserves_unicode_and_multiline_text() -> TestResult {
    let value = Diagnostic {
        source: DiagnosticSource::Cargo,
        severity: Severity::Error,
        code: None,
        message: "falló\n依存関係 🦀".parse()?,
        spans: vec![],
        rendered: Some(String::new()),
        suggestions: vec![],
        truncated: true,
    };
    let serialized = serde_json::to_value(&value)?;
    assert_eq!(serialized["source"], "cargo");
    assert_eq!(serialized["severity"], "error");
    assert_eq!(serialized["spans"], json!([]));
    assert_eq!(serde_json::from_value::<Diagnostic>(serialized)?, value);
    Ok(())
}

#[test]
fn multipart_suggestion_preserves_insertion_and_deletion() -> TestResult {
    let suggestion = Suggestion::new(
        "mover 🦀".parse()?,
        Applicability::MachineApplicable,
        vec![
            Replacement {
                span: span((1, 1), (1, 2))?,
                replacement: String::new(),
            },
            Replacement {
                span: span((2, 4), (2, 4))?,
                replacement: "🦀\n".to_owned(),
            },
        ],
    )?;
    assert_eq!(suggestion.message().as_str(), "mover 🦀");
    assert_eq!(suggestion.applicability(), Applicability::MachineApplicable);
    assert_eq!(suggestion.edits().len(), 2);
    assert_eq!(suggestion.edits()[0].replacement, "");
    let serialized = serde_json::to_value(&suggestion)?;
    assert_eq!(serialized["applicability"], "machine_applicable");
    assert_eq!(
        serde_json::from_value::<Suggestion>(serialized)?,
        suggestion
    );
    assert_eq!(
        Suggestion::new("x".parse()?, Applicability::Unspecified, vec![]),
        Err(ContractError::EmptySuggestion)
    );
    assert!(
        serde_json::from_value::<Suggestion>(
            json!({"message":"x","applicability":"unspecified","edits":[]})
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn objects_reject_unknown_fields_and_empty_required_text() -> TestResult {
    let valid_span = serde_json::to_value(span((1, 1), (1, 2))?)?;
    let valid_replacement = json!({"span": valid_span, "replacement":""});
    let valid_suggestion =
        json!({"message":"x","applicability":"unspecified","edits":[valid_replacement]});
    let valid_diagnostic = json!({"source":"rustc","severity":"note","code":null,"message":"x","spans":[valid_span],"rendered":null,"suggestions":[valid_suggestion],"truncated":false});
    macro_rules! reject_extra {
        ($ty:ty, $value:expr) => {{
            let mut value = $value;
            value["unexpected"] = json!(true);
            assert!(serde_json::from_value::<$ty>(value).is_err());
        }};
    }
    reject_extra!(Position, json!({"line":1,"column":1}));
    reject_extra!(ByteRange, json!({"start":0,"end":1}));
    reject_extra!(SourceSpan, valid_span.clone());
    reject_extra!(Replacement, valid_replacement);
    reject_extra!(Suggestion, valid_suggestion.clone());
    reject_extra!(Diagnostic, valid_diagnostic.clone());
    for field in ["file", "label"] {
        let mut value = valid_span.clone();
        value[field] = json!("");
        assert!(serde_json::from_value::<SourceSpan>(value).is_err());
    }
    for field in ["message", "code"] {
        let mut value = valid_diagnostic.clone();
        value[field] = json!("");
        assert!(serde_json::from_value::<Diagnostic>(value).is_err());
    }
    let mut empty_suggestion = valid_suggestion;
    empty_suggestion["message"] = json!("");
    assert!(serde_json::from_value::<Suggestion>(empty_suggestion).is_err());
    Ok(())
}

#[test]
fn closed_enums_keep_explicit_wire_names() -> TestResult {
    for (source, name) in [
        (DiagnosticSource::Rustc, "rustc"),
        (DiagnosticSource::Cargo, "cargo"),
        (DiagnosticSource::Clippy, "clippy"),
        (DiagnosticSource::Rustfmt, "rustfmt"),
        (DiagnosticSource::Rustsec, "rustsec"),
    ] {
        assert_eq!(serde_json::to_value(source)?, json!(name));
        assert_eq!(
            serde_json::from_value::<DiagnosticSource>(json!(name))?,
            source
        );
    }
    for (severity, name) in [
        (Severity::Error, "error"),
        (Severity::Warning, "warning"),
        (Severity::Note, "note"),
        (Severity::Help, "help"),
    ] {
        assert_eq!(serde_json::to_value(severity)?, json!(name));
    }
    for (applicability, name) in [
        (Applicability::MachineApplicable, "machine_applicable"),
        (Applicability::MaybeIncorrect, "maybe_incorrect"),
        (Applicability::HasPlaceholders, "has_placeholders"),
        (Applicability::Unspecified, "unspecified"),
    ] {
        assert_eq!(serde_json::to_value(applicability)?, json!(name));
    }
    for invalid in ["unknown", "Rustc", "", "fatal"] {
        assert!(serde_json::from_value::<DiagnosticSource>(json!(invalid)).is_err());
        assert!(serde_json::from_value::<Severity>(json!(invalid)).is_err());
        assert!(serde_json::from_value::<Applicability>(json!(invalid)).is_err());
    }
    Ok(())
}

#[test]
fn diagnostic_roundtrip_validates_nested_evidence() -> TestResult {
    let primary = span((1, 1), (1, 2))?;
    let secondary = span((2, 1), (3, 1))?;
    let value = Diagnostic {
        source: DiagnosticSource::Rustc,
        severity: Severity::Error,
        code: Some("E0502".parse()?),
        message: "cannot borrow".parse()?,
        spans: vec![primary.clone(), secondary.clone()],
        rendered: Some("error[E0502]\n  → src/日本語.rs:1:1".to_owned()),
        suggestions: vec![Suggestion::new(
            "remove".parse()?,
            Applicability::MaybeIncorrect,
            vec![Replacement {
                span: primary,
                replacement: String::new(),
            }],
        )?],
        truncated: false,
    };
    let wire = serde_json::to_value(&value)?;
    assert_eq!(serde_json::from_value::<Diagnostic>(wire.clone())?, value);
    let mut reversed = wire.clone();
    reversed["suggestions"][0]["edits"][0]["span"]["end"]["column"] = json!(0);
    assert!(serde_json::from_value::<Diagnostic>(reversed).is_err());
    let mut invalid_bytes = wire.clone();
    invalid_bytes["spans"][1]["bytes"] = json!({"start":10,"end":9});
    assert!(serde_json::from_value::<Diagnostic>(invalid_bytes).is_err());
    let mut empty_edits = wire;
    empty_edits["suggestions"][0]["edits"] = json!([]);
    assert!(serde_json::from_value::<Diagnostic>(empty_edits).is_err());
    assert!(serde_json::from_str::<Position>(r#"{"line":1,"line":2,"column":1}"#).is_err());
    assert!(serde_json::from_value::<Position>(json!({"line":1})).is_err());
    Ok(())
}
