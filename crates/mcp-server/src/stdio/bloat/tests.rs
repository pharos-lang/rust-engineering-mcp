use super::super::security_tool::assert_common_error_contract;
use super::*;
use rust_engineering_application::bloat::BloatObservation;
use rust_engineering_domain::bloat::{
    APPROVED_CARGO_BLOAT_VERSION, BloatAttribution, BloatCrate, BloatFunction, BloatProfile,
    MeasuredBinary,
};

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) mod fixtures {
    use super::*;

    pub(in crate::stdio::bloat) fn options(profile: BloatProfile) -> TestResult<BloatOptions> {
        BloatOptions::new("workload".into(), Some("member".into()), profile)
            .map_err(|error| format!("bloat options: {error:?}").into())
    }

    pub(in crate::stdio::bloat) fn measured(size_bytes: u64) -> MeasuredBinary {
        MeasuredBinary {
            size_bytes,
            sha256: format!("sha256:{}", "b".repeat(64)),
            format: BinaryFormat::Elf64Aarch64,
            analysis_build_symbols_forced: true,
        }
    }

    pub(in crate::stdio::bloat) fn attribution(reported: Option<u64>) -> BloatAttribution {
        BloatAttribution {
            estimated: true,
            reported_file_size_bytes: reported,
            text_section_size_bytes: Some(2_048),
            functions: vec![BloatFunction {
                crate_name: "member".into(),
                name: "work".into(),
                size_bytes: 512,
            }],
            crates: vec![BloatCrate {
                name: "member".into(),
                size_bytes: 1_024,
            }],
            functions_omitted_by_row_cap: 0,
            crates_omitted_by_row_cap: 0,
        }
    }

    /// The shape of every binary that links `std`: the analyzer ranked more
    /// functions than the product's own cap admits, so the cap dropped the
    /// smallest ones and says how many. The real native positive dropped 378
    /// of 634 function rows.
    pub(in crate::stdio::bloat) fn capped_attribution(reported: Option<u64>) -> BloatAttribution {
        let mut capped = attribution(reported);
        capped.functions_omitted_by_row_cap = 378;
        capped.crates_omitted_by_row_cap = 3;
        capped
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::stdio::bloat) fn observation(
        options: BloatOptions,
        exit: BloatExit,
        exit_code: Option<i32>,
        completeness: BloatCompleteness,
        measured_binary: Option<MeasuredBinary>,
        attributed: Option<BloatAttribution>,
    ) -> TestResult<BloatObservation> {
        use super::super::super::security_tool::test_fixtures as fixture;
        Ok(BloatObservation {
            options,
            analyzer_version: APPROVED_CARGO_BLOAT_VERSION.into(),
            exit,
            exit_code,
            termination: ExecutionTermination::Exited,
            measured: measured_binary,
            attribution: attributed,
            completeness,
            report: b"{\"file-size\":4096}".to_vec(),
            runtime: fixture::runtime()?,
            execution_fingerprint: fixture::execution_fingerprint('3')?,
            vendor_fingerprint: fixture::source_fingerprint('5')?,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }

    pub(in crate::stdio::bloat) fn descriptor(
        completeness: ArtifactCompleteness,
    ) -> TestResult<QualityArtifactDescriptor> {
        use rust_engineering_domain::{
            ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
            ArtifactSource, GuestArtifactName, PayloadFormatVersion, PluginIdentity,
            QualityArtifactDraft, QualityArtifactId, QualityJobId, QualityMimeType, UtcInstant,
        };
        let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
        Ok(QualityArtifactDraft {
            artifact_id: QualityArtifactId::from_random_bytes([1; 16]),
            member_index: 0,
            kind: QualityArtifactKind::BloatJson,
            mime_type: QualityMimeType::ApplicationJson,
            payload_format_version: PayloadFormatVersion::BloatJsonV1,
            completeness,
            sensitivity: ArtifactSensitivity::SymbolDerived,
            created_at_utc: created.clone(),
            expires_at_utc: created.checked_add_seconds(60)?,
            source: ArtifactSource {
                captured_source_sha256: [2; 32],
                guest_name: GuestArtifactName::BloatJson,
                selection: ArtifactSelection::Workspace,
            },
            runtime: ArtifactRuntime {
                image_digest: [3; 32],
                toolchain_identity: [4; 32],
                plugin: ArtifactPlugin {
                    identity: PluginIdentity::Bloat,
                    version: 1,
                    digest: [5; 32],
                },
                implementation_digest: [6; 32],
            },
        }
        .into_descriptor(
            QualityJobId::from_random_bytes([7; 16]),
            [8; 32],
            [9; 32],
            512,
        )?)
    }

    pub(in crate::stdio::bloat) fn published(
        observation: BloatObservation,
        artifact_completeness: ArtifactCompleteness,
        with_artifact: bool,
    ) -> TestResult<PublishedBloat> {
        let artifacts = if with_artifact {
            vec![descriptor(artifact_completeness)?]
        } else {
            Vec::new()
        };
        Ok(PublishedBloat {
            observation,
            artifacts,
        })
    }
}

fn arguments(extra: &[(&str, serde_json::Value)]) -> TestResult<rmcp::model::JsonObject> {
    let mut value = serde_json::json!({
        "project_ref": "prj_00000000000000000000000000000001",
        "binary_target": "workload",
    })
    .as_object()
    .cloned()
    .ok_or("arguments")?;
    for (key, item) in extra {
        value.insert((*key).into(), item.clone());
    }
    Ok(value)
}

/// A dummy publisher; the dependency-gating tests that use it never reach a
/// call to `publish_bloat` because the executor gate is checked, and returns,
/// before any port method could run.
struct NeverPublisher;
impl BloatPublisher for NeverPublisher {
    fn publish_bloat(
        &mut self,
        _capture: &rust_engineering_application::security::SecurityCapture,
        _observation: &BloatObservation,
        _revalidate: &mut dyn FnMut() -> Result<
            rust_engineering_application::QualityOwnerFacts,
            InspectionError,
        >,
    ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
        unreachable!("dependency-gating tests never dispatch to the worker")
    }
}

fn vendor_config() -> TestResult<HostCargoVendorConfig> {
    Ok(HostCargoVendorConfig {
        directory: std::path::PathBuf::from("/vendor"),
        fingerprint: format!("sha256:{}", "9".repeat(64)).parse()?,
    })
}

fn registry() -> TestResult<Arc<Mutex<Registry>>> {
    Ok(Arc::new(Mutex::new(
        Registry::new(
            rust_engineering_project::SecureProjects::new(&[]).map_err(|_| "backend")?,
            rust_engineering_project::OsReferences,
            rust_engineering_project::MonotonicClock::default(),
            10,
            1,
        )
        .map_err(|_| "registry")?,
    )))
}

#[test]
fn schema_is_closed_and_stable() -> TestResult {
    let tool = BloatTool::new()?;
    let definition = serde_json::to_value(&tool.definition)?;
    assert_eq!(definition["name"], NAME);
    assert_eq!(definition["inputSchema"]["additionalProperties"], false);
    assert!(
        definition["outputSchema"]["additionalProperties"] == false
            || definition["outputSchema"]["unevaluatedProperties"] == false
    );
    assert_eq!(
        definition["inputSchema"]["properties"]["timeout_seconds"]["default"],
        DEFAULT_TIMEOUT_SECONDS
    );
    assert_eq!(
        definition["inputSchema"]["properties"]["timeout_seconds"]["maximum"],
        300
    );
    Ok(())
}

#[test]
fn closed_input_rejects_paths_unknown_fields_and_out_of_range_numbers() -> TestResult {
    let tool = BloatTool::new()?;
    assert!(tool.contract.decode(Some(arguments(&[])?)).is_ok());
    assert!(
        tool.contract
            .decode(Some(arguments(&[
                ("package", serde_json::json!("member")),
                ("profile", serde_json::json!("release_lto")),
                ("timeout_seconds", serde_json::json!(300)),
                ("execution_mode", serde_json::json!("synchronous")),
            ])?))
            .is_ok()
    );
    for (key, value) in [
        ("binary_target", serde_json::json!("/usr/bin/workload")),
        ("binary_target", serde_json::json!("../workload")),
        ("binary_target", serde_json::json!("work load")),
        ("binary_target", serde_json::json!("work;load")),
        ("binary_target", serde_json::json!("")),
        ("binary_target", serde_json::json!("a".repeat(65))),
        ("package", serde_json::json!("bad name")),
        ("package", serde_json::json!("/etc/passwd")),
        ("package", serde_json::json!("")),
        ("profile", serde_json::json!("debug")),
        ("profile", serde_json::json!("release-lto")),
        ("timeout_seconds", serde_json::json!(0)),
        ("timeout_seconds", serde_json::json!(301)),
        ("flags", serde_json::json!(["--symbols-section"])),
        ("path", serde_json::json!("/proc/self/exe")),
    ] {
        assert!(
            tool.contract
                .decode(Some(arguments(&[(key, value.clone())])?))
                .is_err(),
            "accepted {key}={value}"
        );
    }
    let mut bad_project_ref = arguments(&[])?;
    bad_project_ref.insert("project_ref".into(), serde_json::json!("not-a-project-ref"));
    assert!(tool.contract.decode(Some(bad_project_ref)).is_err());

    // The required target has no default.
    let mut without_target = arguments(&[])?;
    without_target.remove("binary_target");
    assert!(tool.contract.decode(Some(without_target)).is_err());
    Ok(())
}

#[test]
fn a_task_execution_mode_is_refused_before_any_dependency_is_consulted() -> TestResult {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        let tool = BloatTool::new()?;
        let value = tool
            .call_with_token(
                CallToolRequestParams::new(NAME)
                    .with_arguments(arguments(&[("execution_mode", serde_json::json!("task"))])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?
            .structured_content
            .ok_or("content")?;
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["error_code"], "TASKS_REQUIRED");

        let error = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .err()
            .ok_or("runtime must be required")?;
        assert_eq!(error.message, "Bloat runtime is not configured");
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

/// Every runtime dependency this tool needs is declared, in order, before any
/// container is created: discovery readiness, the offline vendor tree, the
/// durable publisher and finally the approved analyzer executor.
#[test]
fn every_absent_runtime_dependency_is_declared_in_order() -> TestResult {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        let ready = Arc::new(AtomicBool::new(false));
        let tool = BloatTool::new()?.with_runtime(Runtime {
            registry: registry()?,
            workers: Workers::new(),
            ready: Arc::clone(&ready),
            vendor: None,
            executor: None,
            publisher: None,
        });
        let blocked = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?
            .structured_content
            .ok_or("content")?;
        assert_eq!(blocked["status"], "blocked");
        assert_eq!(blocked["error_code"], "SANDBOX_DENIED");

        ready.store(true, Ordering::Release);
        let missing_vendor = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?
            .structured_content
            .ok_or("content")?;
        assert_eq!(missing_vendor["status"], "unavailable");
        assert_eq!(missing_vendor["error_code"], "MISSING_OFFLINE_DATA");

        let tool = BloatTool::new()?.with_runtime(Runtime {
            registry: registry()?,
            workers: Workers::new(),
            ready: Arc::new(AtomicBool::new(true)),
            vendor: Some(vendor_config()?),
            executor: None,
            publisher: None,
        });
        let missing_publisher = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?
            .structured_content
            .ok_or("content")?;
        assert_eq!(missing_publisher["status"], "unavailable");
        assert_eq!(missing_publisher["error_code"], "ARTIFACT_UNAVAILABLE");

        let tool = BloatTool::new()?.with_runtime(Runtime {
            registry: registry()?,
            workers: Workers::new(),
            ready: Arc::new(AtomicBool::new(true)),
            vendor: Some(vendor_config()?),
            executor: None,
            publisher: Some(Arc::new(Mutex::new(NeverPublisher))),
        });
        let missing_executor = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?
            .structured_content
            .ok_or("content")?;
        assert_eq!(missing_executor["status"], "unavailable");
        assert_eq!(missing_executor["error_code"], "TOOL_NOT_INSTALLED");
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[test]
fn operational_errors_have_closed_status_and_codes() -> TestResult {
    assert_common_error_contract!(BloatTool::new()?, error);
    Ok(())
}

#[test]
fn result_encoding_separates_build_and_analyzer_failures_from_declared_completeness_states()
-> TestResult {
    let tool = BloatTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let options = fixtures::options(BloatProfile::Release)?;
    let cases: Vec<(PublishedBloat, &str, serde_json::Value)> = vec![
        (
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::Passed,
                    Some(0),
                    BloatCompleteness::Complete,
                    Some(fixtures::measured(4_096)),
                    Some(fixtures::attribution(Some(4_096))),
                )?,
                ArtifactCompleteness::Complete,
                true,
            )?,
            "passed",
            serde_json::Value::Null,
        ),
        (
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::CompilationFailed,
                    Some(101),
                    BloatCompleteness::Unavailable,
                    None,
                    None,
                )?,
                ArtifactCompleteness::Complete,
                false,
            )?,
            "failed",
            serde_json::json!("OBSERVED_FAILURE"),
        ),
        (
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::AnalysisFailed,
                    Some(1),
                    BloatCompleteness::Unavailable,
                    None,
                    None,
                )?,
                ArtifactCompleteness::Complete,
                false,
            )?,
            "failed",
            serde_json::json!("OBSERVED_FAILURE"),
        ),
        (
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::Uncalibrated,
                    Some(7),
                    BloatCompleteness::Unavailable,
                    None,
                    None,
                )?,
                ArtifactCompleteness::Complete,
                false,
            )?,
            "blocked",
            serde_json::json!("ANALYZER_UNAVAILABLE"),
        ),
        (
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::Passed,
                    Some(0),
                    BloatCompleteness::UnsupportedFormat,
                    Some(MeasuredBinary {
                        size_bytes: 2_048,
                        sha256: format!("sha256:{}", "c".repeat(64)),
                        format: BinaryFormat::Wasm,
                        analysis_build_symbols_forced: true,
                    }),
                    None,
                )?,
                ArtifactCompleteness::Complete,
                false,
            )?,
            "blocked",
            serde_json::json!("UNSUPPORTED_FORMAT"),
        ),
        (
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::Passed,
                    Some(0),
                    BloatCompleteness::SizeMismatch,
                    Some(fixtures::measured(4_096)),
                    Some(fixtures::attribution(Some(4_095))),
                )?,
                ArtifactCompleteness::Complete,
                true,
            )?,
            "blocked",
            serde_json::json!("SIZE_MISMATCH"),
        ),
        // ADR-079 §2: a ranking the product's own cap bounded is a validated
        // analysis. This is the case that could not be `passed` before.
        (
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::Passed,
                    Some(0),
                    BloatCompleteness::Complete,
                    Some(fixtures::measured(4_096)),
                    Some(fixtures::capped_attribution(Some(4_096))),
                )?,
                ArtifactCompleteness::Complete,
                true,
            )?,
            "passed",
            serde_json::Value::Null,
        ),
        // ADR-079 §3, last bullet: a valid measurement whose backing artifact
        // was never published has no success to declare.
        (
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::Passed,
                    Some(0),
                    BloatCompleteness::Complete,
                    Some(fixtures::measured(4_096)),
                    Some(fixtures::attribution(Some(4_096))),
                )?,
                ArtifactCompleteness::Complete,
                false,
            )?,
            "blocked",
            serde_json::json!("EVIDENCE_INCOMPLETE"),
        ),
        // Nor one whose artifact was published as anything but complete.
        (
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::Passed,
                    Some(0),
                    BloatCompleteness::Complete,
                    Some(fixtures::measured(4_096)),
                    Some(fixtures::attribution(Some(4_096))),
                )?,
                ArtifactCompleteness::Partial,
                true,
            )?,
            "blocked",
            serde_json::json!("EVIDENCE_INCOMPLETE"),
        ),
    ];
    for (published, status, code) in cases {
        let encoded = tool.encode_result(&reference, published, 7)?;
        let value = encoded.structured_content.ok_or("content")?;
        assert_eq!(value["status"], status, "{value}");
        assert_eq!(value["error_code"], code);
        assert_eq!(value["duration_ms"], 7);
        // `passed` and "the analysis validated" are the same statement
        // (ADR-079 §2), and nothing else in the payload may decide it.
        assert_eq!(
            value["data"]["observation"]["analysis_validated"],
            serde_json::json!(status == "passed"),
            "{value}"
        );
    }
    Ok(())
}

/// ADR-079 §2, on the wire. The case that motivated the decision: every binary
/// that links `std` ranks more than `BLOAT_MAX_ROWS` functions, so the cap acts
/// on every real analysis. The result is `passed`, the cap is named with the
/// limit it applied and the rows it dropped, and the response-budget counters
/// stay at zero so a reader can tell the two limits apart.
///
/// This fails the moment ranking coverage is wired back into the status.
#[test]
fn a_ranking_bounded_by_the_products_own_cap_is_passed_and_says_what_the_cap_dropped() -> TestResult
{
    let tool = BloatTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let observation = fixtures::observation(
        fixtures::options(BloatProfile::Release)?,
        BloatExit::Passed,
        Some(0),
        BloatCompleteness::Complete,
        Some(fixtures::measured(4_096)),
        Some(fixtures::capped_attribution(Some(4_096))),
    )?;
    let value = tool
        .encode_result(
            &reference,
            fixtures::published(observation, ArtifactCompleteness::Complete, true)?,
            7,
        )?
        .structured_content
        .ok_or("content")?;
    assert_eq!(value["status"], "passed", "{value}");
    assert!(value["error_code"].is_null());
    let observation = &value["data"]["observation"];
    assert_eq!(observation["analysis_validated"], true);
    // Validity is untouched by the cap, and says so in its own vocabulary.
    assert_eq!(observation["completeness"], "complete");
    let cap = &observation["attribution"]["ranking_cap"];
    assert_eq!(cap["max_rows"], BLOAT_MAX_ROWS as u64);
    assert_eq!(cap["functions_omitted"], 378);
    assert_eq!(cap["crates_omitted"], 3);
    // The other limit did not act, and says so separately.
    let trim = &observation["attribution"]["response_trim"];
    assert_eq!(trim["functions_omitted"], 0);
    assert_eq!(trim["crates_omitted"], 0);
    assert_eq!(trim["budget_bytes"], RESPONSE_BUDGET_BYTES as u64);
    // A capped ranking is still an exactly measured file.
    assert_eq!(observation["measured"]["size_bytes"], 4_096);
    Ok(())
}

/// ADR-079 §3. A one-byte disagreement between the size this product measured
/// and the size the analyzer reported is not `passed`, and the payload does not
/// present the attribution as a description of the measured file: the
/// completeness names the disagreement, validity is false, and the two sizes
/// stay visibly different instead of being reconciled.
#[test]
fn a_size_mismatch_is_not_passed_and_its_attribution_never_describes_the_measured_file()
-> TestResult {
    let tool = BloatTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let observation = fixtures::observation(
        fixtures::options(BloatProfile::Release)?,
        BloatExit::Passed,
        Some(0),
        BloatCompleteness::SizeMismatch,
        Some(fixtures::measured(4_096)),
        Some(fixtures::attribution(Some(4_095))),
    )?;
    let value = tool
        .encode_result(
            &reference,
            fixtures::published(observation, ArtifactCompleteness::Invalid, true)?,
            7,
        )?
        .structured_content
        .ok_or("content")?;
    assert_eq!(value["status"], "blocked", "{value}");
    assert_eq!(value["error_code"], "SIZE_MISMATCH");
    let observation = &value["data"]["observation"];
    assert_eq!(observation["analysis_validated"], false);
    assert_eq!(observation["completeness"], "size_mismatch");
    assert_eq!(observation["measured"]["size_bytes"], 4_096);
    assert_eq!(
        observation["attribution"]["reported_file_size_bytes"],
        4_095
    );
    assert_ne!(
        observation["measured"]["size_bytes"],
        observation["attribution"]["reported_file_size_bytes"]
    );
    // And the artifact behind it is declared invalid, not complete evidence.
    assert_eq!(value["data"]["artifacts"][0]["completeness"], "invalid");
    Ok(())
}

/// The exact measured file size is what `passed` rests on (ADR-079 §2), so it
/// must survive every path that publishes a measurement at all — including the
/// three that block. A path that dropped it would leave the analyzer's estimate
/// unchecked.
#[test]
fn the_exact_measured_size_survives_every_path_that_measured_a_file() -> TestResult {
    let tool = BloatTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let options = fixtures::options(BloatProfile::Release)?;
    let cases: Vec<(&str, PublishedBloat)> = vec![
        (
            "capped ranking",
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::Passed,
                    Some(0),
                    BloatCompleteness::Complete,
                    Some(fixtures::measured(4_096)),
                    Some(fixtures::capped_attribution(Some(4_096))),
                )?,
                ArtifactCompleteness::Complete,
                true,
            )?,
        ),
        (
            "size mismatch",
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::Passed,
                    Some(0),
                    BloatCompleteness::SizeMismatch,
                    Some(fixtures::measured(4_096)),
                    Some(fixtures::attribution(Some(4_095))),
                )?,
                ArtifactCompleteness::Invalid,
                true,
            )?,
        ),
        (
            "unsupported format",
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::Passed,
                    Some(0),
                    BloatCompleteness::UnsupportedFormat,
                    Some(MeasuredBinary {
                        size_bytes: 4_096,
                        sha256: format!("sha256:{}", "c".repeat(64)),
                        format: BinaryFormat::Wasm,
                        analysis_build_symbols_forced: true,
                    }),
                    None,
                )?,
                ArtifactCompleteness::Unavailable,
                false,
            )?,
        ),
        (
            "unpublished evidence",
            fixtures::published(
                fixtures::observation(
                    options.clone(),
                    BloatExit::Passed,
                    Some(0),
                    BloatCompleteness::Complete,
                    Some(fixtures::measured(4_096)),
                    Some(fixtures::attribution(Some(4_096))),
                )?,
                ArtifactCompleteness::Complete,
                false,
            )?,
        ),
    ];
    for (name, published) in cases {
        let value = tool
            .encode_result(&reference, published, 7)?
            .structured_content
            .ok_or("content")?;
        assert_eq!(
            value["data"]["observation"]["measured"]["size_bytes"], 4_096,
            "{name}: {value}"
        );
        assert_eq!(
            value["data"]["observation"]["measured"]["analysis_build_symbols_forced"], true,
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn measured_and_attribution_stay_separate_objects_in_the_wire_response() -> TestResult {
    let tool = BloatTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let options = fixtures::options(BloatProfile::Release)?;
    let observation = fixtures::observation(
        options,
        BloatExit::Passed,
        Some(0),
        BloatCompleteness::Complete,
        Some(fixtures::measured(4_096)),
        Some(fixtures::attribution(Some(4_096))),
    )?;
    let value = tool
        .encode_result(
            &reference,
            fixtures::published(observation, ArtifactCompleteness::Complete, true)?,
            7,
        )?
        .structured_content
        .ok_or("content")?;
    let observation = &value["data"]["observation"];
    let measured = observation.get("measured").ok_or("measured missing")?;
    let attribution = observation
        .get("attribution")
        .ok_or("attribution missing")?;
    assert!(measured.is_object());
    assert!(attribution.is_object());
    assert_ne!(measured, attribution);
    assert_eq!(attribution["estimated"], true);
    assert_eq!(measured["size_bytes"], 4_096);
    assert_eq!(measured["analysis_build_symbols_forced"], true);
    assert_eq!(attribution["reported_file_size_bytes"], 4_096);
    // Neither object borrows the other's fields.
    assert!(measured.get("functions").is_none());
    assert!(measured.get("estimated").is_none());
    assert!(attribution.get("size_bytes").is_none());
    assert!(attribution.get("sha256").is_none());
    Ok(())
}

#[test]
fn function_and_crate_rows_are_ranked_by_size_before_they_are_published() -> TestResult {
    let tool = BloatTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let options = fixtures::options(BloatProfile::Release)?;
    let mut attribution = fixtures::attribution(Some(4_096));
    attribution.functions = vec![
        BloatFunction {
            crate_name: "member".into(),
            name: "cold".into(),
            size_bytes: 4,
        },
        BloatFunction {
            crate_name: "member".into(),
            name: "hot".into(),
            size_bytes: 900,
        },
    ];
    attribution.crates = vec![
        BloatCrate {
            name: "small".into(),
            size_bytes: 10,
        },
        BloatCrate {
            name: "large".into(),
            size_bytes: 2_000,
        },
    ];
    let observation = fixtures::observation(
        options,
        BloatExit::Passed,
        Some(0),
        BloatCompleteness::Complete,
        Some(fixtures::measured(4_096)),
        Some(attribution),
    )?;
    let value = tool
        .encode_result(
            &reference,
            fixtures::published(observation, ArtifactCompleteness::Complete, true)?,
            7,
        )?
        .structured_content
        .ok_or("content")?;
    let functions: Vec<&str> = value["data"]["observation"]["attribution"]["functions"]
        .as_array()
        .ok_or("functions")?
        .iter()
        .filter_map(|row| row["name"].as_str())
        .collect();
    assert_eq!(functions, ["hot", "cold"]);
    let crates: Vec<&str> = value["data"]["observation"]["attribution"]["crates"]
        .as_array()
        .ok_or("crates")?
        .iter()
        .filter_map(|row| row["name"].as_str())
        .collect();
    assert_eq!(crates, ["large", "small"]);
    assert_eq!(value["data"]["artifacts"][0]["kind"], "bloat_json");
    Ok(())
}
