//! Docker-backed ADR-060 task lifecycle through the production stdio server.
use super::*;

struct BegunTask {
    id: String,
    response_bytes: usize,
    created: Value,
}

fn begin_named_task(
    server: &mut Server,
    request_id: i64,
    tool: &str,
    arguments: Value,
) -> Result<BegunTask> {
    server.send(task_call(request_id, tool, arguments))?;
    let created = server
        .receive(request_id, DISCOVERY_TIMEOUT)
        .map_err(|error| format!("task creation response: {error}"))?;
    assert!(created.get("error").is_none(), "{created}");
    assert_eq!(created["result"]["status"], "working", "{created}");
    let task_id = created["result"]["taskId"]
        .as_str()
        .ok_or("task id missing")?;
    assert!(
        task_id.starts_with("job_") && task_id.len() == 36,
        "{created}"
    );
    Ok(BegunTask {
        id: task_id.to_owned(),
        response_bytes: serde_json::to_vec(&created)?.len(),
        created,
    })
}

fn begin_task(server: &mut Server, project_ref: &Value, request_id: i64) -> Result<BegunTask> {
    begin_named_task(
        server,
        request_id,
        "rust.test.nextest",
        json!({
            "project_ref":project_ref,
            "execution_mode":"task",
            "timeout_seconds":60
        }),
    )
}

/// ADR-060:182,327 fixes the whole `CreateTaskResult` envelope, not only the
/// identifier: a non-null retention that the client cannot choose and a poll
/// interval. Asserting them separates a real task materialization from a tool
/// result that merely happens to carry a `taskId`.
fn assert_created_task_envelope(begun: &BegunTask) -> Result {
    let created = &begun.created["result"];
    assert_eq!(created["taskId"], begun.id, "{created}");
    assert_eq!(created["ttlMs"], 7_200_000, "{created}");
    assert_eq!(created["pollIntervalMs"], 1_000, "{created}");
    assert_eq!(created["createdAt"], created["lastUpdatedAt"], "{created}");
    assert!(
        created["createdAt"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z')),
        "{created}"
    );
    assert!(
        created["statusMessage"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "{created}"
    );
    // A task envelope, never a synchronous tool result smuggled through the
    // same response slot.
    assert!(created.get("content").is_none(), "{created}");
    assert!(created.get("structuredContent").is_none(), "{created}");
    Ok(())
}

fn task_get(server: &mut Server, request_id: i64, task_id: &str) -> Result<Value> {
    server.send(task_request(
        request_id,
        "tasks/get",
        json!({"taskId":task_id}),
    ))?;
    match server.receive(request_id, CONTROL_TIMEOUT) {
        Ok(response) => Ok(response),
        Err(error) => {
            let diagnostic = server
                .stderr
                .recv_timeout(Duration::from_millis(100))
                .ok()
                .and_then(std::result::Result::ok)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default();
            Err(format!("tasks/get response: {error}; server stderr: {diagnostic}").into())
        }
    }
}

fn task_cancel(server: &mut Server, request_id: i64, task_id: &str) -> Result<Value> {
    server.send(task_request(
        request_id,
        "tasks/cancel",
        json!({"taskId":task_id}),
    ))?;
    server
        .receive(request_id, CONTROL_TIMEOUT)
        .map_err(|error| format!("tasks/cancel response: {error}").into())
}

fn wait_terminal(
    server: &mut Server,
    task_id: &str,
    first_request_id: i64,
    expected: &str,
) -> Result<Value> {
    let deadline = Instant::now() + JOIN_TIMEOUT;
    let mut request_id = first_request_id;
    loop {
        let response = task_get(server, request_id, task_id)?;
        request_id += 1;
        assert!(response.get("error").is_none(), "{response}");
        let status = response["result"]["status"]
            .as_str()
            .ok_or("task status missing")?;
        if status == expected {
            return Ok(response);
        }
        if status != "working" {
            return Err(format!("unexpected task state {status}: {response}").into());
        }
        if Instant::now() >= deadline {
            return Err(format!("task did not reach {expected}: {response}").into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_clean(fixture: &Fixture) -> Result {
    let deadline = Instant::now() + JOIN_TIMEOUT;
    loop {
        if fixture.assert_clean(None).is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("task gateway objects were not joined before the deadline".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn prepare_named(fixture: &Fixture, name: &str) -> Result {
    super::nextest_runtime::prepare(fixture, &super::nextest_runtime::fixture_source(name)?)
}

#[test]
#[ignore = "requires approved Docker Tasks path and test-hooks advertisement"]
fn tasks_revocation_during_active_child_masks_cancels_and_prevents_publication() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    prepare_named(&fixture, "slow")?;
    let mut server = Server::start_tasks(&fixture, None)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let task_id = begin_task(&mut server, &opened["project_ref"], 10)?.id;
    let job = super::nextest_runtime::observe_active_nextest(&mut server, &fixture, 10)?;

    let revoked = fixture.root.join("revoked-workspace");
    fs::rename(&fixture.project, &revoked)?;
    let started = Instant::now();
    let masked = task_get(&mut server, 11, &task_id)?;
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(masked["error"]["code"], -32602, "{masked}");
    assert_eq!(masked["error"]["message"], "task unavailable", "{masked}");
    assert!(masked["error"].get("data").is_none(), "{masked}");
    wait_clean(&fixture)?;
    let still_masked = task_get(&mut server, 12, &task_id)?;
    assert_eq!(still_masked["error"]["code"], -32602, "{still_masked}");
    server
        .finish()
        .map_err(|error| format!("revocation server finish: {error}"))?;
    fixture.assert_clean(Some(&job))?;
    fs::rename(&revoked, &fixture.project)?;
    println!(
        "M3_TASK_REVOCATION_RECEIPT {}",
        json!({"active_child":true,"masked_ms":started.elapsed().as_millis(),"publication_visible":false,"joined_cleanup":true})
    );
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "requires approved Docker Tasks path and test-hooks advertisement"]
fn tasks_cancel_before_start_during_execution_publication_and_cleanup_waits_for_join() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;

    // Before start: the feature-gated delay leaves the committed seed in
    // Admission long enough for a real tasks/cancel round trip.
    {
        let mut fixture = Fixture::new()?;
        prepare_named(&fixture, "passing")?;
        let mut server = Server::start_tasks(&fixture, Some("admission"))?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        let task_id = begin_task(&mut server, &opened["project_ref"], 10)?.id;
        assert!(
            task_cancel(&mut server, 11, &task_id)?
                .get("error")
                .is_none()
        );
        let terminal = wait_terminal(&mut server, &task_id, 12, "cancelled")?;
        assert_eq!(terminal["result"]["status"], "cancelled");
        server.finish()?;
        fixture.assert_clean(None)?;
        fixture.successful = true;
    }

    // During execution and cleanup: a second tool and an M1 Resource read are
    // refused without delaying task control. Cancellation remains working while
    // the observed child or joined cleanup is still active.
    let (poll_latency_ms, cancel_to_cleanup_ms, create_response_bytes) = {
        let mut fixture = Fixture::new()?;
        prepare_named(&fixture, "slow")?;
        let mut server = Server::start_tasks(&fixture, Some("cleanup"))?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        let begun = begin_task(&mut server, &opened["project_ref"], 20)?;
        let task_id = begun.id;
        let create_response_bytes = begun.response_bytes;
        super::nextest_runtime::observe_active_nextest(&mut server, &fixture, 20)?;

        server.send(task_call(
            21,
            "rust.test.nextest",
            json!({"project_ref":opened["project_ref"],"execution_mode":"task","timeout_seconds":60}),
        ))?;
        let busy = server.receive(21, CONTROL_TIMEOUT)?;
        assert_eq!(busy["error"]["code"], -32603, "{busy}");
        assert_eq!(busy["error"]["message"], "Task worker busy", "{busy}");

        let forged = format!(
            "rust-artifact://{}/art_00000000000000000000000000000001",
            opened["project_ref"].as_str().ok_or("project ref")?
        );
        server.send(resource_read_request(22, &forged))?;
        let resource_busy = server.receive(22, CONTROL_TIMEOUT)?;
        assert_eq!(resource_busy["error"]["code"], -32000, "{resource_busy}");

        let cancel_started = Instant::now();
        assert!(
            task_cancel(&mut server, 23, &task_id)?
                .get("error")
                .is_none()
        );
        let poll_started = Instant::now();
        let immediate = task_get(&mut server, 24, &task_id)?;
        let poll_latency_ms = poll_started.elapsed().as_millis();
        assert_eq!(immediate["result"]["status"], "working", "{immediate}");

        let cleanup_deadline = Instant::now() + JOIN_TIMEOUT;
        loop {
            let state = task_get(&mut server, 25, &task_id)?;
            if state["result"]["statusMessage"] == "cleaning up" {
                assert!(
                    task_cancel(&mut server, 26, &task_id)?
                        .get("error")
                        .is_none()
                );
                let still_working = task_get(&mut server, 27, &task_id)?;
                assert_eq!(still_working["result"]["status"], "working");
                break;
            }
            if Instant::now() >= cleanup_deadline {
                return Err("task cleanup phase was not observable".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        let terminal = wait_terminal(&mut server, &task_id, 30, "cancelled")?;
        let cancel_to_cleanup_ms = cancel_started.elapsed().as_millis();
        assert_eq!(terminal["result"]["status"], "cancelled");
        wait_clean(&fixture)?;
        server.finish()?;
        fixture.assert_clean(None)?;
        fixture.successful = true;
        (poll_latency_ms, cancel_to_cleanup_ms, create_response_bytes)
    };

    // During publication: the response is computed, but cancellation before
    // JobExecutor::finish is the commit-race rollback branch and no result is
    // exposed through tasks/get.
    {
        let mut fixture = Fixture::new()?;
        prepare_named(&fixture, "passing")?;
        let mut server = Server::start_tasks(&fixture, Some("publish"))?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        let task_id = begin_task(&mut server, &opened["project_ref"], 40)?.id;
        let deadline = Instant::now() + JOIN_TIMEOUT;
        let mut id = 41;
        loop {
            let state = task_get(&mut server, id, &task_id)?;
            id += 1;
            if state["result"]["statusMessage"] == "publishing result" {
                break;
            }
            if state["result"]["status"] != "working" || Instant::now() >= deadline {
                return Err(format!("publication phase was not observable: {state}").into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            task_cancel(&mut server, id, &task_id)?
                .get("error")
                .is_none()
        );
        let terminal = wait_terminal(&mut server, &task_id, id + 1, "cancelled")?;
        assert!(terminal["result"].get("result").is_none(), "{terminal}");
        server.finish()?;
        fixture.assert_clean(None)?;
        fixture.successful = true;
    }

    println!(
        "M3_TASK_CANCEL_RECEIPT {}",
        json!({"before_start":true,"during_execution":true,"during_publication":true,"during_cleanup":true,"poll_latency_ms":poll_latency_ms,"cancel_to_cleanup_ms":cancel_to_cleanup_ms,"create_task_response_bytes":create_response_bytes,"job_record_resident_bytes":rust_engineering_application::job::JobExecutor::resident_record_bytes(),"reserved_result_bytes":rust_engineering_domain::job::TASK_RESPONSE_MAX_BYTES,"joined_cleanup":true})
    );
    Ok(())
}

#[test]
#[ignore = "requires approved Docker Tasks path and test-hooks advertisement"]
fn tasks_eof_joins_hostile_child_and_uncertain_cleanup_fails_session() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let eof_to_join_ms = {
        let mut fixture = Fixture::new()?;
        prepare_named(&fixture, "slow")?;
        let mut server = Server::start_tasks(&fixture, None)?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        begin_task(&mut server, &opened["project_ref"], 10)?;
        super::nextest_runtime::observe_active_nextest(&mut server, &fixture, 10)?;
        let started = Instant::now();
        server.finish()?;
        let eof_to_join_ms = started.elapsed().as_millis();
        fixture.assert_clean(None)?;
        fixture.successful = true;
        eof_to_join_ms
    };

    {
        let mut fixture = Fixture::new()?;
        prepare_named(&fixture, "slow")?;
        let mut server = Server::start_tasks_uncertain(&fixture)?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        begin_task(&mut server, &opened["project_ref"], 20)?;
        super::nextest_runtime::observe_active_nextest(&mut server, &fixture, 20)?;
        server.finish_expect(false)?;
        fixture.assert_clean(None)?;
        fixture.successful = true;
    }

    println!(
        "M3_TASK_EOF_RECEIPT {}",
        json!({"active_child":true,"eof_to_join_ms":eof_to_join_ms,"uncertain_cleanup_failed_session":true,"joined_cleanup":true})
    );
    Ok(())
}

#[test]
#[ignore = "requires approved Docker Tasks path and test-hooks advertisement"]
fn tasks_restart_masks_old_ids_reconciles_objects_and_admits_fresh_work() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    prepare_named(&fixture, "slow")?;
    let old_id = {
        let mut server = Server::start_tasks(&fixture, None)?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        let task_id = begin_task(&mut server, &opened["project_ref"], 10)?.id;
        super::nextest_runtime::observe_active_nextest(&mut server, &fixture, 10)?;
        server.finish()?;
        task_id
    };
    fixture.assert_clean(None)?;

    prepare_named(&fixture, "passing")?;
    let mut restarted = Server::start_tasks(&fixture, None)?;
    let (opened, _) = restarted.bootstrap_open(&fixture)?;
    let masked = task_get(&mut restarted, 10, &old_id)?;
    assert_eq!(masked["error"]["code"], -32602, "{masked}");
    assert_eq!(masked["error"]["message"], "task unavailable", "{masked}");
    fixture.assert_clean(None)?;
    let fresh = begin_task(&mut restarted, &opened["project_ref"], 11)?.id;
    let completed = wait_terminal(&mut restarted, &fresh, 12, "completed")?;
    assert_eq!(
        completed["result"]["result"]["structuredContent"]["status"], "passed",
        "{completed}"
    );
    restarted.finish()?;
    fixture.assert_clean(None)?;
    println!(
        "M3_TASK_RESTART_RECEIPT {}",
        json!({"old_id_masked":true,"residual_objects":0,"fresh_task_completed":true})
    );
    fixture.successful = true;
    Ok(())
}

/// `task_materialization_requested` matches four distinct tool messages. Only
/// nextest was proved end to end, so the other three tools could have drifted
/// out of that match — or never reached admission — without any test failing.
/// Each case below drives the production binary from a peer that declares the
/// Tasks extension, asserts the `CreateTaskResult` envelope, then cancels
/// inside the admission window so no container work is started.
fn materialize_and_cancel(
    server: &mut Server,
    fixture: &Fixture,
    tool: &str,
    arguments: Value,
) -> Result<String> {
    let begun = begin_named_task(server, 10, tool, arguments)?;
    assert_created_task_envelope(&begun)?;
    assert!(task_cancel(server, 11, &begun.id)?.get("error").is_none());
    let terminal = wait_terminal(server, &begun.id, 12, "cancelled")?;
    assert!(terminal["result"].get("result").is_none(), "{terminal}");
    wait_clean(fixture)?;
    Ok(begun.id)
}

#[test]
#[ignore = "requires approved Docker Tasks path and test-hooks advertisement"]
fn tasks_coverage_materializes_a_create_task_result_for_a_declaring_peer() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    prepare_named(&fixture, "passing")?;
    let mut server = Server::start_tasks(&fixture, Some("admission"))?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let task_id = materialize_and_cancel(
        &mut server,
        &fixture,
        "rust.coverage",
        json!({"project_ref":opened["project_ref"],"execution_mode":"task"}),
    )?;
    server.finish()?;
    fixture.assert_clean(None)?;
    println!(
        "M3_TASK_COVERAGE_RECEIPT {}",
        json!({"tool":"rust.coverage","execution_mode":"task","created_task":true,"task_id":task_id,"cancelled":true})
    );
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "requires approved Docker Tasks path and test-hooks advertisement"]
fn tasks_semver_materializes_a_create_task_result_bound_to_the_candidate() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    super::semver_runtime::prepare_side(&fixture.project, "pub fn kept() {}\n")?;
    let candidate = fixture.root.join("candidate-task");
    super::semver_runtime::prepare_side(&candidate, "pub fn kept() {}\n")?;
    let mut server = Server::start_tasks_with_arguments(
        &fixture,
        Some("admission"),
        vec!["--root".into(), candidate.as_os_str().to_owned()],
    )?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    server.send(call(3, "rust.project.open", json!({"path": candidate})))?;
    let candidate_ref =
        server.receive(3, DISCOVERY_TIMEOUT)?["result"]["structuredContent"]["data"]["project_ref"]
            .clone();
    let task_id = materialize_and_cancel(
        &mut server,
        &fixture,
        "rust.semver.check",
        json!({
            "baseline_project_ref":opened["project_ref"],
            "candidate_project_ref":candidate_ref,
            "execution_mode":"task"
        }),
    )?;
    server.finish()?;
    fixture.assert_clean(None)?;
    println!(
        "M3_TASK_SEMVER_RECEIPT {}",
        json!({"tool":"rust.semver.check","execution_mode":"task","created_task":true,"task_id":task_id,"cancelled":true})
    );
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "requires approved Docker Tasks path and test-hooks advertisement"]
fn tasks_mutation_materializes_a_create_task_result_on_its_only_reachable_path() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;

    // Mutation's budget floor is clamped to at least 300 s, so no selection is
    // ever synchronously qualified: `auto` is the production path a declaring
    // peer actually takes, and explicit `task` is the documented alternative.
    for (case, mode) in [("auto", json!({})), ("task", json!("task"))] {
        let mut fixture = Fixture::new()?;
        prepare_named(&fixture, "passing")?;
        let mut server = Server::start_tasks(&fixture, Some("admission"))?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        let mut arguments = json!({
            "project_ref":opened["project_ref"],
            "max_mutants":1,
            "mutant_timeout_seconds":1
        });
        if case == "task" {
            arguments["execution_mode"] = mode;
        }
        let task_id =
            materialize_and_cancel(&mut server, &fixture, "rust.mutation.test", arguments)?;
        server.finish()?;
        fixture.assert_clean(None)?;
        println!(
            "M3_TASK_MUTATION_RECEIPT {}",
            json!({"tool":"rust.mutation.test","execution_mode":case,"created_task":true,"task_id":task_id,"cancelled":true})
        );
        fixture.successful = true;
    }
    Ok(())
}
