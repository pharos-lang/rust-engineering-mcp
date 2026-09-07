//! Discriminating wire observations for the pinned rmcp 3.2.0 Tasks extension.
//!
//! This is an SDK spike, not a product handler or a product tool.  The client
//! side deliberately speaks newline-delimited JSON over an in-process duplex
//! transport so the assertions cover rmcp's emitted wire representation even
//! though the product enables only rmcp's `server` feature.
//!
//! Keep this as an SDK-regression guard tied to the rmcp 3.2.0 pin and rerun it
//! on every SDK bump. It is never product evidence for ADR-060 D06-T01..T14 or
//! any other product qualification oracle.
//!
//! This does not assert task status subscriptions: rmcp 3.2.0 exposes the
//! `notifications/tasks` model but its public server subscription router rejects
//! that method. The ADR therefore selects polling and makes no notification claim.

use std::{
    borrow::Cow,
    error::Error,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use rmcp::{
    ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, CancelTaskParams, ContentBlock,
        CreateTaskResult, ErrorData, GetTaskParams, GetTaskResult, Implementation, ProtocolVersion,
        ServerCapabilities, ServerInfo, UpdateTaskParams,
    },
    service::{RequestContext, RoleServer},
    task_manager::{TaskExit, TaskManager, TaskOptions},
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

type TestResult = Result<(), Box<dyn Error>>;
const TIMEOUT: Duration = Duration::from_secs(2);
const MODERN_VERSION: &str = "2026-07-28";
const VERSIONS: [&str; 5] = [
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
    "2025-11-25",
    MODERN_VERSION,
];
const TASKS_EXTENSION: &str = "io.modelcontextprotocol/tasks";

#[derive(Clone)]
struct SpikeServer {
    tasks: TaskManager,
    cancel_seen: Arc<AtomicBool>,
    release_cancel: Arc<tokio::sync::Notify>,
}

impl SpikeServer {
    fn new() -> Self {
        Self {
            tasks: TaskManager::new(),
            cancel_seen: Arc::new(AtomicBool::new(false)),
            release_cancel: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

impl ServerHandler for SpikeServer {
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let task = match request.name.as_ref() {
            "complete" => self.tasks.spawn(
                TaskOptions::new()
                    .with_ttl_ms(30_000_u64)
                    .with_poll_interval_ms(1),
                |_context| {
                    Box::pin(async {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        Ok(CallToolResult::success(vec![ContentBlock::text("done")]))
                    })
                },
            ),
            "unlimited" => self.tasks.spawn(
                TaskOptions::new()
                    .with_ttl_ms(None)
                    .with_poll_interval_ms(1),
                |_context| {
                    Box::pin(async {
                        Ok(CallToolResult::success(vec![ContentBlock::text(
                            "unlimited",
                        )]))
                    })
                },
            ),
            "cooperative" => {
                let cancel_seen = Arc::clone(&self.cancel_seen);
                let release_cancel = Arc::clone(&self.release_cancel);
                self.tasks.spawn(
                    TaskOptions::new()
                        .with_ttl_ms(30_000_u64)
                        .with_poll_interval_ms(1),
                    move |context| {
                        Box::pin(async move {
                            context.cancelled().await;
                            cancel_seen.store(true, Ordering::Release);
                            release_cancel.notified().await;
                            Err(TaskExit::Cancelled)
                        })
                    },
                )
            }
            _ => {
                return Err(ErrorData::invalid_params("unknown spike operation", None));
            }
        };
        Ok(CallToolResponse::Task(CreateTaskResult::new(task)))
    }

    async fn get_task(
        &self,
        request: GetTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, ErrorData> {
        Ok(GetTaskResult::new(self.tasks.get_task(&request.task_id)?))
    }

    async fn update_task(
        &self,
        request: UpdateTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.tasks
            .update_task(&request.task_id, request.input_responses)
    }

    async fn cancel_task(
        &self,
        request: CancelTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.tasks.cancel_task(&request.task_id)
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tasks()
                .build(),
        )
        .with_server_info(Implementation::new("rmcp-tasks-spike", "3.2.0"))
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[
            ProtocolVersion::V_2024_11_05,
            ProtocolVersion::V_2025_03_26,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2026_07_28,
        ])
    }
}

struct WireClient {
    reader: BufReader<ReadHalf<DuplexStream>>,
    writer: WriteHalf<DuplexStream>,
}

impl WireClient {
    async fn send(&mut self, value: &Value) -> TestResult {
        let mut bytes = serde_json::to_vec(value)?;
        bytes.push(b'\n');
        tokio::time::timeout(TIMEOUT, self.writer.write_all(&bytes))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "wire write timed out"))??;
        tokio::time::timeout(TIMEOUT, self.writer.flush())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "wire flush timed out"))??;
        Ok(())
    }

    async fn response(&mut self, id: i64) -> Result<Value, Box<dyn Error>> {
        let mut line = String::new();
        let count = tokio::time::timeout(TIMEOUT, self.reader.read_line(&mut line))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "wire read timed out"))??;
        if count == 0 || count > 64 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid wire frame").into());
        }
        let value: Value = serde_json::from_str(&line)?;
        if value["jsonrpc"] != "2.0" || value["id"] != id {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "mismatched response").into());
        }
        Ok(value)
    }

    async fn request(&mut self, value: Value, id: i64) -> Result<Value, Box<dyn Error>> {
        self.send(&value).await?;
        self.response(id).await
    }
}

fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
}

fn capabilities(tasks: bool) -> Value {
    if tasks {
        json!({"extensions": {(TASKS_EXTENSION): {}}})
    } else {
        json!({})
    }
}

fn modern_request(id: i64, method: &str, params: Value, tasks: bool) -> Value {
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": MODERN_VERSION,
        "io.modelcontextprotocol/clientCapabilities": capabilities(tasks)
    });
    json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})
}

async fn start(server: SpikeServer) -> (WireClient, tokio::task::JoinHandle<bool>) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        match server.serve(server_transport).await {
            Ok(service) => service.waiting().await.is_ok(),
            Err(_) => false,
        }
    });
    let (reader, writer) = tokio::io::split(client_transport);
    (
        WireClient {
            reader: BufReader::new(reader),
            writer,
        },
        task,
    )
}

async fn bootstrap(
    client: &mut WireClient,
    version: &str,
    tasks: bool,
) -> Result<Value, Box<dyn Error>> {
    if version == MODERN_VERSION {
        client
            .request(modern_request(1, "server/discover", json!({}), tasks), 1)
            .await
    } else {
        let response = client
            .request(
                json!({
                    "jsonrpc":"2.0",
                    "id":1,
                    "method":"initialize",
                    "params":{
                        "protocolVersion":version,
                        "capabilities":capabilities(tasks),
                        "clientInfo":{"name":"rmcp-tasks-wire-spike","version":"1"}
                    }
                }),
                1,
            )
            .await?;
        client
            .send(&json!({
                "jsonrpc":"2.0",
                "method":"notifications/initialized"
            }))
            .await?;
        Ok(response)
    }
}

fn request_for(version: &str, id: i64, method: &str, params: Value, tasks: bool) -> Value {
    if version == MODERN_VERSION {
        modern_request(id, method, params, tasks)
    } else {
        json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})
    }
}

async fn stop(client: WireClient, server: tokio::task::JoinHandle<bool>) -> TestResult {
    drop(client);
    let clean = tokio::time::timeout(TIMEOUT, server)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "server stop timed out"))??;
    if !clean {
        return Err(io::Error::other("rmcp server did not close cleanly").into());
    }
    Ok(())
}

async fn poll_until(
    client: &mut WireClient,
    task_id: &str,
    first_request_id: i64,
    status: &str,
) -> Result<Value, Box<dyn Error>> {
    tokio::time::timeout(TIMEOUT, async {
        for request_id in first_request_id..first_request_id + 200 {
            tokio::time::sleep(Duration::from_millis(2)).await;
            let response = client
                .request(
                    request_for(
                        MODERN_VERSION,
                        request_id,
                        "tasks/get",
                        json!({"taskId":task_id}),
                        true,
                    ),
                    request_id,
                )
                .await?;
            if response["result"]["status"] == status {
                return Ok(response);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("task did not reach {status} within 200 polls"),
        )
        .into())
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "polling timed out"))?
}

#[test]
fn enable_tasks_advertises_for_every_version_and_client_declaration() -> TestResult {
    runtime()?.block_on(async {
        for version in VERSIONS {
            for tasks_declared in [false, true] {
                let (mut client, server) = start(SpikeServer::new()).await;
                let response = bootstrap(&mut client, version, tasks_declared).await?;
                let result = &response["result"];
                if version == MODERN_VERSION {
                    assert!(
                        result["supportedVersions"]
                            .as_array()
                            .is_some_and(|versions| versions.iter().any(|item| item == version))
                    );
                } else {
                    assert_eq!(result["protocolVersion"], version);
                }
                assert_eq!(
                    result["capabilities"]["extensions"][TASKS_EXTENSION],
                    json!({})
                );
                stop(client, server).await?;
            }
        }
        Ok(())
    })
}

#[test]
fn tasks_get_capability_gate_covers_every_version_and_declaration() -> TestResult {
    runtime()?.block_on(async {
        for version in VERSIONS {
            for tasks_declared in [false, true] {
                let (mut client, server) = start(SpikeServer::new()).await;
                bootstrap(&mut client, version, tasks_declared).await?;
                let response = client
                    .request(
                        request_for(
                            version,
                            2,
                            "tasks/get",
                            json!({"taskId":"not-created"}),
                            tasks_declared,
                        ),
                        2,
                    )
                    .await?;
                if tasks_declared {
                    assert_eq!(response["error"]["code"], -32602);
                } else {
                    assert_eq!(response["error"]["code"], -32021);
                    assert_eq!(
                        response["error"]["data"]["requiredCapabilities"]["extensions"]
                            [TASKS_EXTENSION],
                        json!({})
                    );
                }
                stop(client, server).await?;
            }
        }
        Ok(())
    })
}

#[test]
fn create_task_is_polled_to_a_terminal_result_with_the_rmcp_shape() -> TestResult {
    runtime()?.block_on(async {
        let (mut client, server) = start(SpikeServer::new()).await;
        bootstrap(&mut client, MODERN_VERSION, true).await?;
        let created = client
            .request(
                request_for(
                    MODERN_VERSION,
                    2,
                    "tools/call",
                    json!({"name":"complete","arguments":{}}),
                    true,
                ),
                2,
            )
            .await?;
        assert_eq!(created["result"]["resultType"], "task");
        assert_eq!(created["result"]["status"], "working");
        assert_eq!(created["result"]["ttlMs"], 30_000);
        let task_id = created["result"]["taskId"]
            .as_str()
            .ok_or_else(|| io::Error::other("missing task id"))?
            .to_owned();

        let terminal = poll_until(&mut client, &task_id, 3, "completed").await?;
        assert_eq!(terminal["result"]["resultType"], "complete");
        assert_eq!(terminal["result"]["result"]["isError"], false);
        assert_eq!(
            terminal["result"]["result"]["content"][0],
            json!({"type":"text","text":"done"})
        );
        assert!(terminal["result"].get("error").is_none());
        stop(client, server).await
    })
}

#[test]
fn task_cancel_acknowledges_intent_before_cooperative_completion() -> TestResult {
    runtime()?.block_on(async {
        let spike = SpikeServer::new();
        let cancel_seen = Arc::clone(&spike.cancel_seen);
        let release_cancel = Arc::clone(&spike.release_cancel);
        let (mut client, server) = start(spike).await;
        bootstrap(&mut client, MODERN_VERSION, true).await?;
        let created = client
            .request(
                request_for(
                    MODERN_VERSION,
                    2,
                    "tools/call",
                    json!({"name":"cooperative","arguments":{}}),
                    true,
                ),
                2,
            )
            .await?;
        let task_id = created["result"]["taskId"]
            .as_str()
            .ok_or_else(|| io::Error::other("missing task id"))?
            .to_owned();
        let acknowledgement = client
            .request(
                request_for(
                    MODERN_VERSION,
                    3,
                    "tasks/cancel",
                    json!({"taskId":task_id}),
                    true,
                ),
                3,
            )
            .await?;
        assert_eq!(acknowledgement["result"], json!({"resultType":"complete"}));

        tokio::time::timeout(TIMEOUT, async {
            while !cancel_seen.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "cancel was not observed"))?;
        let still_working = client
            .request(
                request_for(
                    MODERN_VERSION,
                    4,
                    "tasks/get",
                    json!({"taskId":task_id}),
                    true,
                ),
                4,
            )
            .await?;
        assert_eq!(still_working["result"]["status"], "working");

        release_cancel.notify_one();
        let terminal = poll_until(&mut client, &task_id, 5, "cancelled").await?;
        assert!(terminal["result"].get("result").is_none());
        stop(client, server).await
    })
}

#[test]
fn none_ttl_is_null_and_legacy_2024_peer_can_receive_a_task() -> TestResult {
    runtime()?.block_on(async {
        let (mut client, server) = start(SpikeServer::new()).await;
        bootstrap(&mut client, MODERN_VERSION, true).await?;
        let unlimited = client
            .request(
                request_for(
                    MODERN_VERSION,
                    2,
                    "tools/call",
                    json!({"name":"unlimited","arguments":{}}),
                    true,
                ),
                2,
            )
            .await?;
        assert_eq!(unlimited["result"]["ttlMs"], Value::Null);
        stop(client, server).await?;

        let version = "2024-11-05";
        let (mut legacy, legacy_server) = start(SpikeServer::new()).await;
        bootstrap(&mut legacy, version, true).await?;
        let created = legacy
            .request(
                request_for(
                    version,
                    2,
                    "tools/call",
                    json!({"name":"complete","arguments":{}}),
                    true,
                ),
                2,
            )
            .await?;
        assert_eq!(created["result"]["resultType"], "task");
        assert_eq!(created["result"]["status"], "working");
        assert!(created["result"]["taskId"].as_str().is_some());
        stop(legacy, legacy_server).await
    })
}
