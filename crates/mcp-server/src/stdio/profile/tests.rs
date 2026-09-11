use super::super::security_tool::assert_common_error_contract;
use super::*;
use rust_engineering_domain::profile::{
    PROFILE_BACKEND, ProfileChild, ProfileCounters, ProfileFrameWeight,
};

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) mod fixtures {
    use super::*;

    pub(in crate::stdio::profile) fn counters(samples: u64, lost: u64) -> ProfileCounters {
        ProfileCounters {
            observed_duration_ms: 9_800,
            samples_collected: samples,
            samples_lost: lost,
            stacks_written: samples / 2,
            frames_total: samples * 4,
            frames_unresolved: 3,
            stacks_truncated: 0,
            modules_seen: 2,
            max_depth_applied: 127,
        }
    }

    pub(in crate::stdio::profile) fn observation(
        build: ProfileBuildOutcome,
        status: ProfileStatus,
        completeness: ProfileCompleteness,
        counters: ProfileCounters,
        top_frames: Vec<ProfileFrameWeight>,
    ) -> TestResult<ProfileObservation> {
        use super::super::super::security_tool::test_fixtures as fixture;
        Ok(ProfileObservation {
            options: ProfileOptions::new("workload".into(), 99, 10)
                .map_err(|error| format!("profile options: {error:?}"))?,
            backend: PROFILE_BACKEND,
            build,
            build_exit_code: Some(0),
            status,
            counters,
            child: ProfileChild {
                exit_code: Some(0),
                signal: None,
            },
            perf_errno: if status == ProfileStatus::ProfilerUnavailable {
                Some(1)
            } else {
                None
            },
            completeness,
            top_frames,
            stacks: b"main;work 990\n".to_vec(),
            svg: if status == ProfileStatus::ProfilerUnavailable {
                Vec::new()
            } else {
                b"<svg></svg>".to_vec()
            },
            termination: ExecutionTermination::Exited,
            runtime: fixture::runtime()?,
            execution_fingerprint: fixture::execution_fingerprint('3')?,
            vendor_fingerprint: fixture::source_fingerprint('5')?,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }

    pub(in crate::stdio::profile) fn descriptor(
        kind: QualityArtifactKind,
        completeness: ArtifactCompleteness,
    ) -> TestResult<QualityArtifactDescriptor> {
        use rust_engineering_domain::{
            ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
            ArtifactSource, GuestArtifactName, PayloadFormatVersion, PluginIdentity,
            QualityArtifactDraft, QualityArtifactId, QualityJobId, QualityMimeType, UtcInstant,
        };
        let (payload_format_version, mime_type, guest_name) = match kind {
            QualityArtifactKind::FlamegraphSvg => (
                PayloadFormatVersion::FlamegraphSvgV1,
                QualityMimeType::ImageSvgXml,
                GuestArtifactName::FlamegraphSvg,
            ),
            _ => (
                PayloadFormatVersion::CollapsedStacksV1,
                QualityMimeType::TextPlain,
                GuestArtifactName::CollapsedStacks,
            ),
        };
        let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
        Ok(QualityArtifactDraft {
            artifact_id: QualityArtifactId::from_random_bytes([1; 16]),
            member_index: 0,
            kind,
            mime_type,
            payload_format_version,
            completeness,
            sensitivity: ArtifactSensitivity::SymbolDerived,
            created_at_utc: created.clone(),
            expires_at_utc: created.checked_add_seconds(60)?,
            source: ArtifactSource {
                captured_source_sha256: [2; 32],
                guest_name,
                selection: ArtifactSelection::Workspace,
            },
            runtime: ArtifactRuntime {
                image_digest: [3; 32],
                toolchain_identity: [4; 32],
                plugin: ArtifactPlugin {
                    identity: PluginIdentity::ProfileHelper,
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

    pub(in crate::stdio::profile) fn published(
        observation: ProfileObservation,
        completeness: ArtifactCompleteness,
        with_artifacts: bool,
    ) -> TestResult<PublishedProfile> {
        let artifacts = if with_artifacts {
            vec![
                descriptor(QualityArtifactKind::FlamegraphSvg, completeness)?,
                descriptor(QualityArtifactKind::CollapsedStacks, completeness)?,
            ]
        } else {
            Vec::new()
        };
        Ok(PublishedProfile {
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

#[test]
fn closed_input_rejects_paths_arguments_and_out_of_range_numbers() -> TestResult {
    let tool = ProfileTool::new()?;
    assert!(tool.contract.decode(Some(arguments(&[])?)).is_ok());
    assert!(
        tool.contract
            .decode(Some(arguments(&[
                ("frequency_hz", serde_json::json!(999)),
                ("duration_seconds", serde_json::json!(60)),
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
        ("args", serde_json::json!(["--fast"])),
        ("path", serde_json::json!("/proc/self/exe")),
        ("sysctl", serde_json::json!("kernel.perf_event_paranoid")),
        ("frequency_hz", serde_json::json!(0)),
        ("frequency_hz", serde_json::json!(1000)),
        ("duration_seconds", serde_json::json!(0)),
        ("duration_seconds", serde_json::json!(61)),
        ("timeout_seconds", serde_json::json!(0)),
        ("timeout_seconds", serde_json::json!(301)),
    ] {
        assert!(
            tool.contract
                .decode(Some(arguments(&[(key, value.clone())])?))
                .is_err(),
            "accepted {key}={value}"
        );
    }
    // The required target has no default.
    let mut without_target = arguments(&[])?;
    without_target.remove("binary_target");
    assert!(tool.contract.decode(Some(without_target)).is_err());
    Ok(())
}

#[test]
fn an_ungranted_host_blocks_before_anything_is_dispatched() -> TestResult {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        let tool = ProfileTool::new()?;
        let error = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .err()
            .ok_or("runtime must be required")?;
        assert_eq!(error.message, "Profiling runtime is not configured");

        // No grant: blocked before discovery, before the vendor tree and before
        // any worker is admitted. `ready` is deliberately still false here.
        let tool = ProfileTool::new()?.with_runtime(Runtime {
            registry: Arc::new(Mutex::new(
                Registry::new(
                    rust_engineering_project::SecureProjects::new(&[]).map_err(|_| "backend")?,
                    rust_engineering_project::OsReferences,
                    rust_engineering_project::MonotonicClock::default(),
                    10,
                    1,
                )
                .map_err(|_| "registry")?,
            )),
            workers: Workers::new(),
            ready: Arc::new(AtomicBool::new(false)),
            vendor: None,
            profiling: None,
            executor: None,
            publisher: None,
        });
        let value = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?
            .structured_content
            .ok_or("content")?;
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["error_code"], "PROFILING_NOT_AUTHORIZED");

        // The task gate is refused before the grant is even consulted.
        let task = tool
            .call_with_token(
                CallToolRequestParams::new(NAME)
                    .with_arguments(arguments(&[("execution_mode", serde_json::json!("task"))])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?
            .structured_content
            .ok_or("content")?;
        assert_eq!(task["error_code"], "TASKS_REQUIRED");
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[test]
fn a_granted_host_still_declares_every_absent_dependency() -> TestResult {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        let ready = Arc::new(AtomicBool::new(false));
        let tool = ProfileTool::new()?.with_runtime(Runtime {
            registry: Arc::new(Mutex::new(
                Registry::new(
                    rust_engineering_project::SecureProjects::new(&[]).map_err(|_| "backend")?,
                    rust_engineering_project::OsReferences,
                    rust_engineering_project::MonotonicClock::default(),
                    10,
                    1,
                )
                .map_err(|_| "registry")?,
            )),
            workers: Workers::new(),
            ready: Arc::clone(&ready),
            vendor: None,
            profiling: Some(HostProfilingConfig {
                grant: ProfilingGrant::UserSpaceSampling,
            }),
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
        assert_eq!(blocked["error_code"], "SANDBOX_DENIED");
        ready.store(true, Ordering::Release);
        let unavailable = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?
            .structured_content
            .ok_or("content")?;
        assert_eq!(unavailable["status"], "unavailable");
        assert_eq!(unavailable["error_code"], "MISSING_OFFLINE_DATA");
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[test]
fn operational_errors_have_closed_status_and_codes() -> TestResult {
    assert_common_error_contract!(
        ProfileTool::new()?,
        error,
        PROFILING_NOT_AUTHORIZED => ("blocked", "PROFILING_NOT_AUTHORIZED"),
    );
    Ok(())
}

#[test]
fn result_encoding_separates_a_build_failure_a_refused_sampler_and_partial_evidence() -> TestResult
{
    let tool = ProfileTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let cases: Vec<(PublishedProfile, &str, serde_json::Value)> = vec![
        (
            fixtures::published(
                fixtures::observation(
                    ProfileBuildOutcome::Built,
                    ProfileStatus::Complete,
                    ProfileCompleteness::Complete,
                    fixtures::counters(990, 0),
                    vec![ProfileFrameWeight {
                        frame: "main".into(),
                        self_samples: 900,
                        total_samples: 990,
                    }],
                )?,
                ArtifactCompleteness::Complete,
                true,
            )?,
            "passed",
            serde_json::Value::Null,
        ),
        // Zero samples is a valid, declared result and never an error.
        (
            fixtures::published(
                fixtures::observation(
                    ProfileBuildOutcome::Built,
                    ProfileStatus::ChildExited,
                    ProfileCompleteness::NoSamples,
                    fixtures::counters(0, 0),
                    Vec::new(),
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
                    ProfileBuildOutcome::CompilationFailed,
                    ProfileStatus::ChildExited,
                    ProfileCompleteness::NoSamples,
                    fixtures::counters(0, 0),
                    Vec::new(),
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
                    ProfileBuildOutcome::TargetNotFound,
                    ProfileStatus::ChildExited,
                    ProfileCompleteness::NoSamples,
                    fixtures::counters(0, 0),
                    Vec::new(),
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
                    ProfileBuildOutcome::Built,
                    ProfileStatus::ProfilerUnavailable,
                    ProfileCompleteness::Unavailable,
                    fixtures::counters(0, 0),
                    Vec::new(),
                )?,
                ArtifactCompleteness::Complete,
                false,
            )?,
            "blocked",
            serde_json::json!("PROFILER_UNAVAILABLE"),
        ),
        (
            fixtures::published(
                fixtures::observation(
                    ProfileBuildOutcome::Built,
                    ProfileStatus::Complete,
                    ProfileCompleteness::LostSamples,
                    fixtures::counters(990, 12),
                    Vec::new(),
                )?,
                ArtifactCompleteness::Complete,
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
        // The sampled evidence itself never crosses this boundary.
        let text = serde_json::to_string(&value)?;
        assert!(!text.contains("<svg"), "svg bytes leaked");
        assert!(!text.contains("main;work"), "collapsed stacks leaked");
    }
    Ok(())
}

#[test]
fn a_refused_sampler_still_reports_its_errno_and_every_counter() -> TestResult {
    let tool = ProfileTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let value = tool
        .encode_result(
            &reference,
            fixtures::published(
                fixtures::observation(
                    ProfileBuildOutcome::Built,
                    ProfileStatus::ProfilerUnavailable,
                    ProfileCompleteness::Unavailable,
                    fixtures::counters(0, 0),
                    Vec::new(),
                )?,
                ArtifactCompleteness::Complete,
                false,
            )?,
            7,
        )?
        .structured_content
        .ok_or("content")?;
    let observation = &value["data"]["observation"];
    assert_eq!(observation["perf_errno"], 1);
    assert_eq!(observation["status"], "profiler_unavailable");
    assert_eq!(observation["completeness"], "unavailable");
    assert_eq!(observation["samples_collected"], 0);
    assert_eq!(observation["samples_lost"], 0);
    assert_eq!(observation["frames_unresolved"], 3);
    assert_eq!(observation["stacks_truncated"], 0);
    assert_eq!(observation["frequency_hz"], 99);
    assert_eq!(observation["requested_duration_seconds"], 10);
    assert_eq!(observation["observed_duration_ms"], 9_800);
    assert_eq!(observation["backend"], PROFILE_BACKEND);
    Ok(())
}

#[test]
fn frames_are_ranked_by_weight_before_they_are_published() -> TestResult {
    let tool = ProfileTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let frames = vec![
        ProfileFrameWeight {
            frame: "cold".into(),
            self_samples: 1,
            total_samples: 4,
        },
        ProfileFrameWeight {
            frame: "hot".into(),
            self_samples: 800,
            total_samples: 900,
        },
        ProfileFrameWeight {
            frame: "[unknown]".into(),
            self_samples: 40,
            total_samples: 40,
        },
    ];
    let value = tool
        .encode_result(
            &reference,
            fixtures::published(
                fixtures::observation(
                    ProfileBuildOutcome::Built,
                    ProfileStatus::Complete,
                    ProfileCompleteness::Complete,
                    fixtures::counters(990, 0),
                    frames,
                )?,
                ArtifactCompleteness::Complete,
                true,
            )?,
            7,
        )?
        .structured_content
        .ok_or("content")?;
    let names: Vec<&str> = value["data"]["observation"]["top_frames"]
        .as_array()
        .ok_or("top_frames")?
        .iter()
        .filter_map(|frame| frame["frame"].as_str())
        .collect();
    assert_eq!(names, ["hot", "[unknown]", "cold"]);
    assert_eq!(value["data"]["artifacts"][0]["kind"], "flamegraph_svg");
    assert_eq!(value["data"]["artifacts"][1]["kind"], "collapsed_stacks");
    Ok(())
}
