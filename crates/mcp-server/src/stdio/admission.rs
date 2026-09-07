//! Admission over SDK messages; framing, routing and cancellation remain in rmcp.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{
    ClientNotification, ClientRequest, ErrorData, Extensions, JsonRpcMessage, ProtocolVersion,
    RequestId, ServerInfo, ServerResult,
};
use rmcp::service::{
    NotificationContext, RequestContext, RoleServer, RxJsonRpcMessage, Service, TxJsonRpcMessage,
};
use rmcp::transport::Transport;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use rust_engineering_application::job::{DeliveryToken, DeliveryTracker, InMemoryDeliveryTracker};

use super::budget::IoFailure;

const CAPACITY: usize = 16;
const SEND_DEADLINE: Duration = Duration::from_secs(10);

/// Retain this through complete SDK service dispatch, including cancellation
/// cleanup. It is independent of request IDs and of response suppression.
#[derive(Clone)]
pub(super) struct AdmissionLease {
    _permit: Arc<OwnedSemaphorePermit>,
}

pub(super) fn lease(extensions: &Extensions) -> Option<AdmissionLease> {
    extensions.get::<AdmissionLease>().cloned()
}

#[allow(dead_code)]
pub(super) fn delivery_token(extensions: &Extensions) -> Option<DeliveryToken> {
    extensions.get::<DeliveryToken>().copied()
}

struct OutstandingRequest {
    lease: AdmissionLease,
    delivery: DeliveryToken,
}

/// Preserve the transport lease even if the inner handler drops its context
/// before awaiting application work. All routing/negotiation stays in the SDK.
pub(super) struct AdmittedService<S>(S);

impl<S> AdmittedService<S> {
    pub(super) fn new(inner: S) -> Self {
        Self(inner)
    }
}

impl<S: Service<RoleServer>> Service<RoleServer> for AdmittedService<S> {
    async fn handle_request(
        &self,
        request: ClientRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<ServerResult, ErrorData> {
        let lease = lease(&context.extensions);
        let result = self.0.handle_request(request, context).await;
        drop(lease);
        result
    }

    async fn handle_notification(
        &self,
        notification: ClientNotification,
        context: NotificationContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let lease = lease(&context.extensions);
        let result = self.0.handle_notification(notification, context).await;
        drop(lease);
        result
    }

    fn get_info(&self) -> ServerInfo {
        self.0.get_info()
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        self.0.supported_protocol_versions()
    }
}

pub(super) struct AdmittedTransport<T> {
    inner: T,
    failed: Arc<IoFailure>,
    requests: Arc<Semaphore>,
    request_ledger: Arc<Mutex<HashMap<RequestId, OutstandingRequest>>>,
    delivery: Arc<dyn DeliveryTracker>,
    next_delivery: Arc<AtomicU64>,
    notifications: Arc<Semaphore>,
    sends: Arc<Semaphore>,
    send_deadline: Duration,
}

impl<T> AdmittedTransport<T> {
    pub(super) fn new(inner: T, failed: Arc<IoFailure>) -> Self {
        Self {
            inner,
            failed,
            requests: Arc::new(Semaphore::new(CAPACITY)),
            request_ledger: Arc::new(Mutex::new(HashMap::new())),
            delivery: Arc::new(InMemoryDeliveryTracker::default()),
            next_delivery: Arc::new(AtomicU64::new(1)),
            notifications: Arc::new(Semaphore::new(CAPACITY)),
            sends: Arc::new(Semaphore::new(CAPACITY)),
            send_deadline: SEND_DEADLINE,
        }
    }

    pub(super) fn with_delivery_tracker(mut self, delivery: Arc<dyn DeliveryTracker>) -> Self {
        self.delivery = delivery;
        self
    }
}

fn rejected() -> io::Error {
    io::Error::other("MCP transport resource policy rejected operation")
}

impl<T: Transport<RoleServer>> Transport<RoleServer> for AdmittedTransport<T> {
    type Error = io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        // Acquire synchronously: rmcp may retain many unpolled send futures.
        // Keep request ownership through the SDK's intermediate response queue
        // and this send future. Cancelled responses are suppressed by rmcp, so
        // their bounded ledger tombstones intentionally remain until teardown.
        let response_id = match &item {
            JsonRpcMessage::Response(response) => Some(response.id.clone()),
            JsonRpcMessage::Error(error) => error.id.clone(),
            JsonRpcMessage::Request(_) | JsonRpcMessage::Notification(_) => None,
        };
        // rmcp has finished dispatch and removed its request token before this
        // handoff. Transfer the lease to the send future: the peer may observe
        // the frame and reuse its ID before stdout's flush future completes.
        // Completion must never remove a newer ledger entry using the same ID.
        let outstanding = match self.request_ledger.lock() {
            Ok(mut ledger) => response_id.as_ref().and_then(|id| ledger.remove(id)),
            Err(_) => {
                self.failed.record();
                None
            }
        };
        let permit = Arc::clone(&self.sends).try_acquire_owned();
        let failed = Arc::clone(&self.failed);
        let delivery = Arc::clone(&self.delivery);
        let deadline = tokio::time::Instant::now() + self.send_deadline;
        let send = if permit.is_ok() && !failed.occurred() {
            Some(self.inner.send(item))
        } else {
            failed.record();
            None
        };
        async move {
            let request_lease = outstanding.map(|request| (request.lease, request.delivery));
            let _permit = permit.map_err(|_| rejected())?;
            let send = send.ok_or_else(rejected)?;
            match tokio::time::timeout_at(deadline, send).await {
                Ok(Ok(())) => {
                    if let Some((_, token)) = request_lease.as_ref() {
                        delivery.mark_delivered(*token);
                    }
                    Ok(())
                }
                _ => {
                    failed.record();
                    Err(rejected())
                }
            }
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        if self.failed.occurred() {
            return None;
        }
        let Some(mut message) = self.inner.receive().await else {
            self.failed.end();
            return None;
        };
        let slots = match &message {
            JsonRpcMessage::Request(_) => &self.requests,
            JsonRpcMessage::Notification(_) => &self.notifications,
            // Responses have no detached server handler. SDK handles them inline.
            JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => return Some(message),
        };
        match Arc::clone(slots).try_acquire_owned() {
            Ok(permit) => {
                let lease = AdmissionLease {
                    _permit: Arc::new(permit),
                };
                let delivery = if let JsonRpcMessage::Request(request) = &message {
                    let Some(delivery) =
                        DeliveryToken::new(self.next_delivery.fetch_add(1, Ordering::AcqRel))
                    else {
                        self.failed.record();
                        return None;
                    };
                    match self.request_ledger.lock() {
                        Ok(mut ledger) => {
                            if ledger.contains_key(&request.id) {
                                self.failed.record();
                                return None;
                            }
                            ledger.insert(
                                request.id.clone(),
                                OutstandingRequest {
                                    lease: lease.clone(),
                                    delivery,
                                },
                            );
                        }
                        Err(_) => {
                            self.failed.record();
                            return None;
                        }
                    }
                    Some(delivery)
                } else {
                    None
                };
                message.insert_extension(lease);
                if let Some(delivery) = delivery {
                    message.insert_extension(delivery);
                }
                Some(message)
            }
            Err(_) => {
                // Never wait here: cancellation must not be stuck behind an
                // admission queue. Overload cancels the complete SDK session.
                self.failed.record();
                None
            }
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.failed.end();
        let result = tokio::time::timeout(self.send_deadline, self.inner.close()).await;
        // This is terminal transport teardown, never cancellation recycling.
        match self.request_ledger.lock() {
            Ok(mut ledger) => ledger.clear(),
            Err(_) => self.failed.record(),
        }
        match result {
            Ok(Ok(())) => Ok(()),
            _ => {
                self.failed.record();
                Err(rejected())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stdio::workers::JobExecutionSignal;
    use rmcp::model::{EmptyResult, GetExtensions, ServerResult};
    use rust_engineering_application::job::{
        DeliveryToken, InMemoryDeliveryTracker, InMemoryJobRegistry, JobAuthority, JobClock,
        JobError, JobEvent, JobEvents, JobExecutor, JobIds, JobPermit, JobRegistry, JobSignal,
        JobSubmission,
    };
    use rust_engineering_domain::{
        ProjectRef,
        job::{JobBudget, JobKind, JobOwnerBinding, JobState, Milliseconds},
    };
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Fixture {
        messages: VecDeque<RxJsonRpcMessage<RoleServer>>,
        stall: bool,
    }

    impl Transport<RoleServer> for Fixture {
        type Error = io::Error;
        fn send(
            &mut self,
            _: TxJsonRpcMessage<RoleServer>,
        ) -> impl Future<Output = io::Result<()>> + Send + 'static {
            let stall = self.stall;
            async move {
                if stall {
                    std::future::pending::<()>().await;
                }
                Ok(())
            }
        }
        async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
            if self.stall && self.messages.is_empty() {
                std::future::pending::<()>().await;
            }
            self.messages.pop_front()
        }
        async fn close(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn run(test: impl Future<Output = io::Result<()>>) -> io::Result<()> {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()?
            .block_on(test)
    }

    fn request(id: i64) -> io::Result<RxJsonRpcMessage<RoleServer>> {
        serde_json::from_value(serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/list"}))
            .map_err(io::Error::other)
    }

    fn response() -> TxJsonRpcMessage<RoleServer> {
        JsonRpcMessage::response(
            ServerResult::EmptyResult(EmptyResult {}),
            rmcp::model::NumberOrString::Number(1),
        )
    }

    #[test]
    fn sdk_message_decoder_rejects_batch_arrays() {
        for batch in [
            serde_json::json!([]),
            serde_json::json!([{"jsonrpc":"2.0", "id":1, "method":"tools/list"}]),
            serde_json::json!([
                {"jsonrpc":"2.0", "id":1, "method":"tools/list"},
                {"jsonrpc":"2.0", "method":"notifications/cancelled", "params":{"requestId":1}}
            ]),
        ] {
            assert!(serde_json::from_value::<RxJsonRpcMessage<RoleServer>>(batch).is_err());
        }
    }

    #[test]
    fn completed_dispatch_without_sent_response_retains_admission() -> io::Result<()> {
        run(async {
            let messages = (0..=CAPACITY)
                .map(|id| request(id as i64))
                .collect::<io::Result<_>>()?;
            let failed = Arc::new(IoFailure::default());
            let mut transport = AdmittedTransport::new(
                Fixture {
                    messages,
                    stall: false,
                },
                Arc::clone(&failed),
            );
            for _ in 0..CAPACITY {
                let message = transport.receive().await.ok_or_else(rejected)?;
                let JsonRpcMessage::Request(request) = message else {
                    return Err(rejected());
                };
                // Model completed dispatch: all handler-owned copies disappear,
                // but the SDK has not consumed/sent its response yet.
                drop(lease(request.request.extensions()).ok_or_else(rejected)?);
                drop(request);
            }
            assert!(transport.receive().await.is_none());
            assert!(failed.cancellation().is_cancelled());
            Ok(())
        })
    }

    #[test]
    fn duplicate_outstanding_request_id_closes_session() -> io::Result<()> {
        run(async {
            let failed = Arc::new(IoFailure::default());
            let mut transport = AdmittedTransport::new(
                Fixture {
                    messages: VecDeque::from([request(1)?, request(1)?]),
                    stall: false,
                },
                failed.clone(),
            );
            drop(transport.receive().await.ok_or_else(rejected)?);
            assert!(transport.receive().await.is_none());
            assert!(failed.occurred());
            Ok(())
        })
    }

    #[test]
    fn reused_id_during_pending_send_retains_both_request_leases() -> io::Result<()> {
        run(async {
            let failed = Arc::new(IoFailure::default());
            let mut transport = AdmittedTransport::new(
                Fixture {
                    messages: VecDeque::from([request(1)?, request(1)?]),
                    stall: true,
                },
                failed.clone(),
            );
            drop(transport.receive().await.ok_or_else(rejected)?);
            let pending = transport.send(response());
            assert_eq!(transport.requests.available_permits(), CAPACITY - 1);
            drop(transport.receive().await.ok_or_else(rejected)?);
            assert_eq!(transport.requests.available_permits(), CAPACITY - 2);
            assert!(!failed.occurred());
            drop(pending);
            assert_eq!(transport.requests.available_permits(), CAPACITY - 1);
            Ok(())
        })
    }

    #[test]
    fn old_send_completion_cannot_release_reused_request_id() -> io::Result<()> {
        run(async {
            let failed = Arc::new(IoFailure::default());
            let mut transport = AdmittedTransport::new(
                Fixture {
                    messages: VecDeque::from([request(1)?, request(1)?, request(1)?]),
                    stall: false,
                },
                failed.clone(),
            );
            drop(transport.receive().await.ok_or_else(rejected)?);
            let old_send = transport.send(response());
            drop(transport.receive().await.ok_or_else(rejected)?);
            assert_eq!(transport.requests.available_permits(), CAPACITY - 2);
            old_send.await?;
            assert_eq!(transport.requests.available_permits(), CAPACITY - 1);
            assert!(transport.receive().await.is_none());
            assert!(failed.occurred());
            Ok(())
        })
    }

    #[test]
    fn cancellation_tombstone_is_retained_until_teardown() -> io::Result<()> {
        run(async {
            let failed = Arc::new(IoFailure::default());
            let cancellation = serde_json::from_value(serde_json::json!({
                "jsonrpc":"2.0", "method":"notifications/cancelled", "params":{"requestId":1}
            }))
            .map_err(io::Error::other)?;
            let mut transport = AdmittedTransport::new(
                Fixture {
                    messages: VecDeque::from([request(1)?, cancellation]),
                    stall: false,
                },
                failed,
            );
            drop(transport.receive().await.ok_or_else(rejected)?);
            drop(transport.receive().await.ok_or_else(rejected)?);
            assert_eq!(transport.requests.available_permits(), CAPACITY - 1);
            transport.close().await?;
            assert_eq!(transport.requests.available_permits(), CAPACITY);
            Ok(())
        })
    }

    #[test]
    fn cancellation_has_separate_notification_capacity() -> io::Result<()> {
        run(async {
            let mut messages = (0..CAPACITY)
                .map(|id| request(id as i64))
                .collect::<io::Result<VecDeque<_>>>()?;
            for _ in 0..=CAPACITY {
                messages.push_back(serde_json::from_value(serde_json::json!({
                    "jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 1}
                })).map_err(io::Error::other)?);
            }
            let failed = Arc::new(IoFailure::default());
            let mut transport = AdmittedTransport::new(
                Fixture {
                    messages,
                    stall: false,
                },
                Arc::clone(&failed),
            );
            let mut held = Vec::new();
            for _ in 0..CAPACITY * 2 {
                held.push(transport.receive().await.ok_or_else(rejected)?);
            }
            assert!(!failed.occurred());
            assert!(transport.receive().await.is_none());
            assert!(failed.occurred());
            Ok(())
        })
    }

    #[test]
    fn normal_response_send_releases_slot_for_sequential_calls() -> io::Result<()> {
        run(async {
            let messages = (0..CAPACITY * 3)
                .map(|id| request(id as i64))
                .collect::<io::Result<_>>()?;
            let failed = Arc::new(IoFailure::default());
            let mut transport = AdmittedTransport::new(
                Fixture {
                    messages,
                    stall: false,
                },
                Arc::clone(&failed),
            );
            for id in 0..CAPACITY * 3 {
                let message = transport.receive().await.ok_or_else(rejected)?;
                drop(message);
                transport
                    .send(JsonRpcMessage::response(
                        ServerResult::EmptyResult(EmptyResult {}),
                        RequestId::Number(id as i64),
                    ))
                    .await?;
            }
            assert!(!failed.occurred());
            Ok(())
        })
    }

    #[test]
    fn delivery_is_marked_only_after_successful_send_completion() -> io::Result<()> {
        run(async {
            let failed = Arc::new(IoFailure::default());
            let tracker = Arc::new(InMemoryDeliveryTracker::default());
            let mut transport = AdmittedTransport::new(
                Fixture {
                    messages: VecDeque::from([request(1)?]),
                    stall: false,
                },
                failed,
            )
            .with_delivery_tracker(tracker.clone());
            let message = transport.receive().await.ok_or_else(rejected)?;
            let JsonRpcMessage::Request(request) = message else {
                return Err(rejected());
            };
            let token = delivery_token(request.request.extensions()).ok_or_else(rejected)?;
            tracker.register(token);
            assert!(!tracker.was_delivered(token));
            drop(request);
            transport.send(response()).await?;
            assert!(tracker.was_delivered(token));
            Ok(())
        })
    }

    #[test]
    fn unpolled_send_futures_are_bounded_and_dropping_releases_capacity() -> io::Result<()> {
        run(async {
            let failed = Arc::new(IoFailure::default());
            let mut transport = AdmittedTransport::new(
                Fixture {
                    messages: VecDeque::new(),
                    stall: true,
                },
                Arc::clone(&failed),
            );
            let mut pending = Vec::new();
            for _ in 0..CAPACITY {
                pending.push(transport.send(response()));
            }
            drop(pending.pop());
            pending.push(transport.send(response()));
            assert!(!failed.occurred());
            assert!(transport.send(response()).await.is_err());
            assert!(failed.occurred());
            Ok(())
        })
    }

    struct WaitingService {
        started: tokio_util::sync::CancellationToken,
        cancelled: tokio_util::sync::CancellationToken,
        cleanup: tokio_util::sync::CancellationToken,
    }

    struct FixedClock;
    impl JobClock for FixedClock {
        fn monotonic_millis(&self) -> Milliseconds {
            Milliseconds(0)
        }
        fn utc_now(&self) -> Result<String, JobError> {
            Ok("2026-09-05T12:00:00Z".into())
        }
    }
    struct FixedIds;
    impl JobIds for FixedIds {
        fn random_128(&self) -> Result<[u8; 16], JobError> {
            Ok([7; 16])
        }
    }
    struct Allow;
    impl JobAuthority for Allow {
        fn revalidate(&self, _: &JobOwnerBinding, _: &ProjectRef, _: u64) -> bool {
            true
        }
    }
    struct Held(AtomicBool);
    impl JobPermit for Held {
        fn is_held(&self) -> bool {
            self.0.load(Ordering::Acquire)
        }
        fn release_after_cleanup(&self) {
            self.0.store(false, Ordering::Release);
        }
    }
    struct Silent;
    impl JobEvents for Silent {
        fn record(&self, _: JobEvent) {}
    }

    struct QueuedJobService {
        queued: tokio_util::sync::CancellationToken,
        request_cancelled: tokio_util::sync::CancellationToken,
    }

    struct PrecommitCancelledService {
        executor: Arc<JobExecutor>,
        signal: Arc<JobExecutionSignal>,
        permit: Arc<Held>,
        rolled_back: tokio_util::sync::CancellationToken,
    }
    impl Service<RoleServer> for PrecommitCancelledService {
        async fn handle_request(
            &self,
            _: ClientRequest,
            context: RequestContext<RoleServer>,
        ) -> Result<ServerResult, ErrorData> {
            context.ct.cancelled().await;
            let project_ref = "prj_00000000000000000000000000000001"
                .parse()
                .map_err(|_| ErrorData::internal_error("fixture project", None))?;
            let outcome = self.executor.submit_guarded(
                JobSubmission {
                    kind: JobKind::TestNextest,
                    owner: JobOwnerBinding::new([9; 32]),
                    project_ref,
                    policy_generation: 1,
                    budget: JobBudget::asynchronous_default()
                        .map_err(|_| ErrorData::internal_error("fixture budget", None))?,
                    delivery_token: DeliveryToken::new(1)
                        .ok_or_else(|| ErrorData::internal_error("fixture token", None))?,
                    reserved_result_bytes: 1,
                    signal: Arc::clone(&self.signal) as Arc<dyn JobSignal>,
                    permit: Arc::clone(&self.permit) as Arc<dyn JobPermit>,
                },
                || !context.ct.is_cancelled(),
            );
            if outcome != Err(JobError::Unavailable)
                || self.permit.is_held()
                || self.signal.cancellation_requested()
            {
                return Err(ErrorData::internal_error("fixture rollback", None));
            }
            self.rolled_back.cancel();
            Ok(ServerResult::EmptyResult(EmptyResult {}))
        }
        async fn handle_notification(
            &self,
            _: ClientNotification,
            _: NotificationContext<RoleServer>,
        ) -> Result<(), ErrorData> {
            Ok(())
        }
        fn get_info(&self) -> ServerInfo {
            ServerInfo::default()
        }
    }
    impl Service<RoleServer> for QueuedJobService {
        async fn handle_request(
            &self,
            _: ClientRequest,
            context: RequestContext<RoleServer>,
        ) -> Result<ServerResult, ErrorData> {
            let request_cancelled = self.request_cancelled.clone();
            tokio::spawn(async move {
                context.ct.cancelled().await;
                request_cancelled.cancel();
            });
            self.queued.cancel();
            Ok(ServerResult::EmptyResult(EmptyResult {}))
        }
        async fn handle_notification(
            &self,
            _: ClientNotification,
            _: NotificationContext<RoleServer>,
        ) -> Result<(), ErrorData> {
            Ok(())
        }
        fn get_info(&self) -> ServerInfo {
            ServerInfo::default()
        }
    }

    impl Service<RoleServer> for WaitingService {
        async fn handle_request(
            &self,
            _: ClientRequest,
            context: RequestContext<RoleServer>,
        ) -> Result<ServerResult, ErrorData> {
            let token = context.ct.clone();
            drop(context);
            self.started.cancel();
            token.cancelled().await;
            self.cancelled.cancel();
            self.cleanup.cancelled().await;
            Ok(ServerResult::EmptyResult(EmptyResult {}))
        }
        async fn handle_notification(
            &self,
            _: ClientNotification,
            _: NotificationContext<RoleServer>,
        ) -> Result<(), ErrorData> {
            Ok(())
        }
        fn get_info(&self) -> ServerInfo {
            ServerInfo::default()
        }
    }

    #[test]
    fn sdk_dispatch_retains_lease_until_cancellation_cleanup_finishes() -> io::Result<()> {
        run(async {
            let failed = Arc::new(IoFailure::default());
            let transport = AdmittedTransport::new(
                Fixture {
                    messages: VecDeque::from([request(1)?]),
                    stall: false,
                },
                failed,
            );
            let slots = Arc::clone(&transport.requests);
            let started = tokio_util::sync::CancellationToken::new();
            let cancelled = tokio_util::sync::CancellationToken::new();
            let cleanup = tokio_util::sync::CancellationToken::new();
            let session = tokio_util::sync::CancellationToken::new();
            let service = rmcp::service::serve_directly_with_ct(
                AdmittedService::new(WaitingService {
                    started: started.clone(),
                    cancelled: cancelled.clone(),
                    cleanup: cleanup.clone(),
                }),
                transport,
                None,
                session.clone(),
            );
            tokio::time::timeout(Duration::from_secs(1), started.cancelled())
                .await
                .map_err(io::Error::other)?;
            assert_eq!(slots.available_permits(), CAPACITY - 1);
            session.cancel();
            tokio::time::timeout(Duration::from_secs(1), cancelled.cancelled())
                .await
                .map_err(io::Error::other)?;
            assert_eq!(slots.available_permits(), CAPACITY - 1);
            cleanup.cancel();
            tokio::time::timeout(Duration::from_secs(1), service.waiting())
                .await
                .map_err(io::Error::other)?
                .map_err(io::Error::other)?;
            assert_eq!(slots.available_permits(), CAPACITY);
            Ok(())
        })
    }

    #[test]
    fn sdk_suppressed_cancelled_response_retains_tombstone() -> io::Result<()> {
        run(async {
            let failed = Arc::new(IoFailure::default());
            let cancellation = serde_json::from_value(serde_json::json!({
                "jsonrpc":"2.0", "method":"notifications/cancelled", "params":{"requestId":1}
            }))
            .map_err(io::Error::other)?;
            let transport = AdmittedTransport::new(
                Fixture {
                    messages: VecDeque::from([request(1)?, cancellation]),
                    stall: true,
                },
                failed,
            );
            let slots = Arc::clone(&transport.requests);
            let started = tokio_util::sync::CancellationToken::new();
            let cancelled = tokio_util::sync::CancellationToken::new();
            let cleanup = tokio_util::sync::CancellationToken::new();
            let service = rmcp::service::serve_directly_with_ct(
                AdmittedService::new(WaitingService {
                    started,
                    cancelled: cancelled.clone(),
                    cleanup: cleanup.clone(),
                }),
                transport,
                None,
                tokio_util::sync::CancellationToken::new(),
            );
            tokio::time::timeout(Duration::from_secs(1), cancelled.cancelled())
                .await
                .map_err(io::Error::other)?;
            cleanup.cancel();
            // Let SDK dispatch finish and suppress the cancelled response.
            tokio::task::yield_now().await;
            tokio::task::yield_now().await;
            assert_eq!(slots.available_permits(), CAPACITY - 1);
            tokio::time::timeout(Duration::from_secs(1), service.cancel())
                .await
                .map_err(io::Error::other)?
                .map_err(io::Error::other)?;
            assert_eq!(slots.available_permits(), CAPACITY);
            Ok(())
        })
    }

    #[test]
    fn rmcp_post_queue_request_cancellation_does_not_cancel_registry_job_signal() -> io::Result<()>
    {
        run(async {
            let executor = Arc::new(JobExecutor::new(
                Arc::new(InMemoryJobRegistry::default()),
                Arc::new(FixedClock),
                Arc::new(FixedIds),
                Arc::new(Allow),
                Arc::new(InMemoryDeliveryTracker::default()),
                Arc::new(Silent),
            ));
            let signal = JobExecutionSignal::new();
            let project_ref = "prj_00000000000000000000000000000001"
                .parse()
                .map_err(io::Error::other)?;
            let seed = executor
                .submit(JobSubmission {
                    kind: JobKind::TestNextest,
                    owner: JobOwnerBinding::new([9; 32]),
                    project_ref,
                    policy_generation: 1,
                    budget: JobBudget::asynchronous_default().map_err(io::Error::other)?,
                    delivery_token: DeliveryToken::new(1)
                        .ok_or_else(|| io::Error::other("token"))?,
                    reserved_result_bytes: 1,
                    signal: Arc::clone(&signal) as Arc<dyn JobSignal>,
                    permit: Arc::new(Held(AtomicBool::new(true))),
                })
                .map_err(io::Error::other)?;
            executor.start(&seed.id).map_err(io::Error::other)?;

            let cancellation = serde_json::from_value(serde_json::json!({
                "jsonrpc":"2.0", "method":"notifications/cancelled", "params":{"requestId":1}
            }))
            .map_err(io::Error::other)?;
            let failed = Arc::new(IoFailure::default());
            let transport = AdmittedTransport::new(
                Fixture {
                    messages: VecDeque::from([request(1)?, cancellation]),
                    stall: true,
                },
                failed,
            );
            let queued = tokio_util::sync::CancellationToken::new();
            let request_cancelled = tokio_util::sync::CancellationToken::new();
            let session = tokio_util::sync::CancellationToken::new();
            let service = rmcp::service::serve_directly_with_ct(
                AdmittedService::new(QueuedJobService {
                    queued: queued.clone(),
                    request_cancelled: request_cancelled.clone(),
                }),
                transport,
                None,
                session.clone(),
            );
            tokio::time::timeout(Duration::from_secs(1), queued.cancelled())
                .await
                .map_err(io::Error::other)?;
            tokio::time::timeout(Duration::from_secs(1), request_cancelled.cancelled())
                .await
                .map_err(io::Error::other)?;
            assert!(!signal.cancellation_requested());
            assert!(!signal.cancellation().is_cancelled());
            assert_eq!(
                executor.status(&seed.id).map_err(io::Error::other)?.state,
                JobState::Running
            );
            signal.observe_cleanup();
            session.cancel();
            tokio::time::timeout(Duration::from_secs(1), service.cancel())
                .await
                .map_err(io::Error::other)?
                .map_err(io::Error::other)?;
            Ok(())
        })
    }

    #[test]
    fn rmcp_precommit_request_cancellation_rolls_back_without_publishing_a_job() -> io::Result<()> {
        run(async {
            let registry = Arc::new(InMemoryJobRegistry::default());
            let executor = Arc::new(JobExecutor::new(
                registry.clone(),
                Arc::new(FixedClock),
                Arc::new(FixedIds),
                Arc::new(Allow),
                Arc::new(InMemoryDeliveryTracker::default()),
                Arc::new(Silent),
            ));
            let signal = JobExecutionSignal::new();
            let permit = Arc::new(Held(AtomicBool::new(true)));
            let rolled_back = tokio_util::sync::CancellationToken::new();
            let cancellation = serde_json::from_value(serde_json::json!({
                "jsonrpc":"2.0", "method":"notifications/cancelled", "params":{"requestId":1}
            }))
            .map_err(io::Error::other)?;
            let service = rmcp::service::serve_directly_with_ct(
                AdmittedService::new(PrecommitCancelledService {
                    executor,
                    signal: signal.clone(),
                    permit: permit.clone(),
                    rolled_back: rolled_back.clone(),
                }),
                AdmittedTransport::new(
                    Fixture {
                        messages: VecDeque::from([request(1)?, cancellation]),
                        stall: true,
                    },
                    Arc::new(IoFailure::default()),
                ),
                None,
                tokio_util::sync::CancellationToken::new(),
            );
            tokio::time::timeout(Duration::from_secs(1), rolled_back.cancelled())
                .await
                .map_err(io::Error::other)?;
            assert!(registry.snapshot().map_err(io::Error::other)?.is_empty());
            assert!(!permit.is_held());
            assert!(!signal.cancellation_requested());
            tokio::time::timeout(Duration::from_secs(1), service.cancel())
                .await
                .map_err(io::Error::other)?
                .map_err(io::Error::other)?;
            Ok(())
        })
    }

    #[test]
    fn queued_send_deadline_cancels_session() -> io::Result<()> {
        run(async {
            let failed = Arc::new(IoFailure::default());
            let mut transport = AdmittedTransport::new(
                Fixture {
                    messages: VecDeque::new(),
                    stall: true,
                },
                Arc::clone(&failed),
            );
            transport.send_deadline = Duration::from_millis(5);
            assert!(transport.send(response()).await.is_err());
            assert!(failed.occurred());
            Ok(())
        })
    }
}
