//! The bounded duplex LSP session: the one place in this product that writes
//! frames to a child while concurrently reading its answers.
//!
//! This is a *second* supervisor, not a change to [`crate::supervisor`]. The
//! existing one writes all of stdin and then reads until the child exits, which
//! is exactly wrong for a language server: rust-analyzer answers the first
//! request long before it will ever exit, and it stops making progress unless
//! its stdout is drained while more input arrives (ADR-084 §2 phase 3).
//!
//! Shape of the implementation, and why:
//!
//! * **No reader threads.** All three pipes are put in non-blocking mode and
//!   driven from one loop ([`LspSession::step`]). Every iteration of every wait
//!   therefore has the cancellation token, the overall deadline and the byte
//!   budgets in scope, with no shared mutable state and no thread to join on a
//!   kill path. The one-shot supervisor needs threads because it blocks on the
//!   child's exit; nothing here ever blocks.
//! * **Every error is terminal.** A violated budget, a fatal codec error, a
//!   write that cannot complete, the deadline and the cancellation token all
//!   kill the child immediately and poison the session, because none of them
//!   leaves a peer this adapter is still willing to believe. Late and duplicate
//!   responses are the only inbound faults that are *not* terminal: the codec
//!   classifies them as "discard and count" (D25 §1.6), so they never reach a
//!   caller and never end a session.
//! * **The peer's stderr never leaves.** It is accumulated into a bounded
//!   buffer only so its length and digest can be published; no method returns
//!   the bytes, because they can carry project content (D25 §1.6).
//!
//! Killing the child kills the `docker` client, not the guest container. The
//! container's own kill, removal and absence check belong to the gateway, which
//! performs them before releasing its single-flight lock (gateway G3).

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use rust_engineering_application::{ExecutionCancellation, ExecutionError};
use rust_engineering_domain as domain;
use sha2::{Digest, Sha256};

use crate::lsp_codec::{
    self, CodecError, Correlator, Decoder, DecoderLimits, Matched, OutgoingMessage, RawMessage,
    ResponseError,
};

/// The readiness notification this lifecycle observes (ADR-084 §5). Every
/// other notification is counted and discarded.
pub(crate) const SERVER_STATUS: &str = "experimental/serverStatus";

/// How long a wait sleeps when neither pipe moved. Small enough that a one
/// second initialize budget is still measured in the right order of magnitude,
/// large enough that a sixty second wait is not a spin.
const POLL: Duration = Duration::from_millis(2);
/// Bytes read from one pipe in a single [`LspSession::step`]. Bounds the work
/// per iteration so the cancellation and deadline checks stay responsive even
/// against a peer that floods.
const READ_BUDGET: usize = 512 * 1024;
const READ_CHUNK: usize = 8192;
/// Bytes written to the child in one step, mirroring the one-shot supervisor.
const WRITE_CHUNK: usize = 8192;

/// Every way a session ends other than by answering.
///
/// Contract: every variant is terminal — by the time one is returned the child
/// has been killed and reaped and the session is poisoned, so a caller that
/// ignores the error cannot obtain data from the peer afterwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionError {
    /// A fatal framing or JSON-RPC fault from the peer. A `Content-Length`
    /// above the frame bound arrives here as [`CodecError::FrameLimit`], with
    /// the declared length kept in the outcome: the decoder enforces that bound
    /// while parsing the header, and that is the `FRAME_LIMIT` budget of
    /// ADR-084 §8.
    Codec(CodecError),
    /// An outgoing message could not be framed. Unreachable for the values this
    /// module sends, and still reported rather than silently skipped.
    Encode,
    /// An outgoing frame would exceed the frame bound.
    FrameLimit,
    /// The per-job message count was reached.
    MessageLimit,
    /// The stdout or stderr byte budget was reached.
    ByteLimit,
    /// A pipe failed, or the peer closed stdin under a write.
    Io,
    /// A wait ran out of budget.
    Timeout,
    /// The cancellation token was observed.
    Cancelled,
    /// The peer closed stdout while an answer was still owed.
    Eof,
}

/// The budgets of ADR-084 §8 that the session itself enforces.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SessionBudget {
    /// Absolute end of the whole session, derived from the call's total budget.
    pub(crate) deadline: Instant,
    pub(crate) max_messages: usize,
    pub(crate) max_frame_bytes: usize,
    pub(crate) max_stdout_bytes: usize,
    pub(crate) max_stderr_bytes: usize,
}

impl SessionBudget {
    /// The ADR-084 §8 values: 1 MiB per frame, 4096 messages per job, and
    /// 16 MiB / 1 MiB of stdout / stderr.
    pub(crate) fn standard(deadline: Instant) -> Self {
        Self {
            deadline,
            max_messages: domain::MAX_MESSAGES_PER_JOB,
            max_frame_bytes: domain::MAX_FRAME_BYTES,
            max_stdout_bytes: domain::MAX_STDOUT_BYTES,
            max_stderr_bytes: domain::MAX_STDERR_BYTES,
        }
    }
}

/// One `experimental/serverStatus` notification, with the time since the
/// session opened. `health` is `None` only when the server used a spelling
/// outside the closed set, which is recorded rather than assumed healthy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StatusRecord {
    pub(crate) quiescent: bool,
    pub(crate) health: Option<domain::ServerHealth>,
    pub(crate) elapsed_ms: u64,
}

/// What one inbound message turned into for the caller. Server→client
/// requests, and every notification other than [`SERVER_STATUS`], are handled
/// inside the session and never surface here.
#[derive(Debug, PartialEq)]
pub(crate) enum Event {
    Response(Matched),
    ServerStatus(StatusRecord),
    /// The peer closed stdout. Terminal for the session, but not an error on
    /// its own: a caller that was owed nothing may accept it.
    Eof,
}

/// Everything a finished session is willing to say about itself.
#[derive(Clone, Debug)]
pub(crate) struct SessionOutcome {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stop: domain::SessionStop,
    pub(crate) messages_in: u64,
    pub(crate) messages_out: u64,
    pub(crate) bytes_in: u64,
    pub(crate) bytes_out: u64,
    /// Every stderr byte observed, including bytes dropped once the retained
    /// buffer was full.
    pub(crate) stderr_len: u64,
    /// Digest of the *retained* stderr bytes, paired with `stderr_truncated` so
    /// it is never mistaken for the digest of the whole stream.
    pub(crate) stderr_sha256: String,
    pub(crate) stderr_truncated: bool,
    pub(crate) server_requests: Vec<String>,
    pub(crate) notifications_dropped: u64,
    pub(crate) late_responses: u64,
    pub(crate) status_transcript: Vec<StatusRecord>,
    pub(crate) fatal: Option<SessionError>,
    /// The `Content-Length` the peer declared for a frame above the bound.
    pub(crate) declared_frame_bytes: Option<u64>,
    /// The io error *kind* of a kill or a reap that failed, and nothing more:
    /// no pid, no path, no message. `None` means the call reported success.
    pub(crate) kill_error: Option<String>,
    pub(crate) reap_error: Option<String>,
    pub(crate) duration_ms: u64,
}

/// Kills and reaps on every exit path, including an unwind.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(target_os = "macos")]
fn nonblocking(fd: &impl std::os::fd::AsFd) -> Result<(), ExecutionError> {
    let flags = rustix::fs::fcntl_getfl(fd).map_err(|_| ExecutionError::Infrastructure)?;
    rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)
        .map_err(|_| ExecutionError::Infrastructure)
}
// The positive native scope is a macOS ARM64 host with a Linux ARM64 guest
// (ADR-084 §10). Every other host fails closed here rather than running a
// session whose pipes might block. The one-shot supervisor keeps its own copy
// of this helper: it belongs to a package this one does not edit.
#[cfg(not(target_os = "macos"))]
fn nonblocking<T>(_: &T) -> Result<(), ExecutionError> {
    Err(ExecutionError::Unavailable)
}

fn sha256_text(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut text = String::from("sha256:");
    for byte in hash {
        use std::fmt::Write;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

/// A bounded, cancellable duplex LSP session over one child process.
pub(crate) struct LspSession<'a> {
    child: ChildGuard,
    /// Taken once the write side is finished with or has failed; dropping it
    /// gives the peer EOF on stdin.
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    decoder: Decoder,
    correlator: Correlator,
    inbox: VecDeque<RawMessage>,
    /// The single frame currently being written, and how much of it went out.
    outgoing: Vec<u8>,
    written: usize,
    budget: SessionBudget,
    cancel: &'a dyn ExecutionCancellation,
    started: Instant,
    bytes_in: u64,
    bytes_out: u64,
    messages_in: u64,
    messages_out: u64,
    stderr_len: u64,
    stderr_kept: Vec<u8>,
    stderr_truncated: bool,
    notifications_dropped: u64,
    late_responses: u64,
    server_requests: Vec<String>,
    status_transcript: Vec<StatusRecord>,
    stdout_closed: bool,
    fatal: Option<SessionError>,
    /// `true` once a `wait` really returned a status for this child. Until it
    /// does, no code here claims the child left.
    reaped: bool,
    kill_error: Option<String>,
    reap_error: Option<String>,
}

impl<'a> LspSession<'a> {
    /// Spawns `command` with all three pipes in non-blocking mode.
    ///
    /// Preconditions: `command` was built by the gateway from a closed phase.
    /// This module never constructs a program or an argument.
    ///
    /// Postconditions: on `Ok` the child is owned by a kill-on-drop guard, so
    /// every later path — including a panic — leaves no live child. On `Err`
    /// nothing was spawned, or what was spawned is already reaped.
    pub(crate) fn open(
        mut command: Command,
        budget: SessionBudget,
        cancel: &'a dyn ExecutionCancellation,
    ) -> Result<Self, ExecutionError> {
        if cancel.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = ChildGuard(command.spawn().map_err(|_| ExecutionError::Unavailable)?);
        let stdin = child.0.stdin.take().ok_or(ExecutionError::Infrastructure)?;
        let stdout = child
            .0
            .stdout
            .take()
            .ok_or(ExecutionError::Infrastructure)?;
        let stderr = child
            .0
            .stderr
            .take()
            .ok_or(ExecutionError::Infrastructure)?;
        nonblocking(&stdin)?;
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr: Some(stderr),
            decoder: Decoder::new(DecoderLimits {
                max_frame_bytes: budget.max_frame_bytes,
                max_messages: budget.max_messages,
                max_total_bytes: budget.max_stdout_bytes,
            }),
            correlator: Correlator::new(),
            inbox: VecDeque::new(),
            outgoing: Vec::new(),
            written: 0,
            budget,
            cancel,
            started: Instant::now(),
            bytes_in: 0,
            bytes_out: 0,
            messages_in: 0,
            messages_out: 0,
            stderr_len: 0,
            stderr_kept: Vec::new(),
            stderr_truncated: false,
            notifications_dropped: 0,
            late_responses: 0,
            server_requests: Vec::new(),
            status_transcript: Vec::new(),
            stdout_closed: false,
            fatal: None,
            reaped: false,
            kill_error: None,
            reap_error: None,
        })
    }

    /// Frames and writes one message in full, or fails.
    ///
    /// Both directions keep moving while the write drains, so a peer that is
    /// slow to read its stdin cannot deadlock this side by filling the stdout
    /// pipe it is not being read from.
    ///
    /// `until` is the caller's *phase* deadline (see [`Self::until`]), not just
    /// the session's: a peer that stops reading its stdin spends that phase's
    /// budget and no more, and the resulting [`SessionError::Timeout`] is named
    /// by the phase it happened in rather than published as a claim about the
    /// server's indexing.
    pub(crate) fn send(
        &mut self,
        message: &OutgoingMessage,
        until: Instant,
    ) -> Result<(), SessionError> {
        self.guard()?;
        let bytes = lsp_codec::encode(message).map_err(|_| SessionError::Encode)?;
        if bytes.len() > self.budget.max_frame_bytes {
            return Err(self.fail(SessionError::FrameLimit));
        }
        if self.messages_out >= self.budget.max_messages as u64 {
            return Err(self.fail(SessionError::MessageLimit));
        }
        self.outgoing = bytes;
        self.written = 0;
        while self.written < self.outgoing.len() {
            self.guard()?;
            let moved = self.step()?;
            if self.written >= self.outgoing.len() {
                break;
            }
            if Instant::now() >= until {
                return Err(self.fail(SessionError::Timeout));
            }
            if !moved {
                std::thread::sleep(POLL);
            }
        }
        self.messages_out += 1;
        Ok(())
    }

    /// The next complete inbound message, or `None` at end of stdout.
    ///
    /// `timeout` bounds this call; the session's own deadline bounds it further
    /// and can never be extended by it.
    ///
    /// The analyzer lifecycle uses [`Self::next_event`] instead, which also
    /// refuses server→client requests and discards notifications. This raw
    /// primitive stays part of the session's contract — and is exercised by the
    /// tests below — because a caller that must see an unclassified message has
    /// no other way to.
    #[allow(dead_code)]
    pub(crate) fn recv(&mut self, timeout: Duration) -> Result<Option<RawMessage>, SessionError> {
        let until = self.until(timeout);
        self.recv_until(until)
    }

    /// The next message the caller has to decide about.
    ///
    /// Handled here and never returned: a server→client request, which is
    /// answered with `-32601` and counted (it grants nothing, ADR-084 §4); a
    /// notification other than [`SERVER_STATUS`], which is counted and dropped;
    /// and a late or duplicate response, which the codec classifies as
    /// non-fatal and which is counted and dropped.
    ///
    /// `timeout` bounds the whole call, not each message inside it.
    pub(crate) fn next_event(&mut self, timeout: Duration) -> Result<Event, SessionError> {
        let until = self.until(timeout);
        self.next_event_until(until)
    }

    /// Sends one request and returns its matching response.
    ///
    /// Only one request is ever outstanding in this lifecycle, so a response
    /// the correlator accepts for a different id is a peer contradicting
    /// itself and is fatal. A [`SERVER_STATUS`] notification arriving
    /// mid-request is recorded in the transcript and does not end the wait.
    pub(crate) fn request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
        timeout: Duration,
    ) -> Result<Matched, SessionError> {
        // One deadline for the whole request: the write and the wait share the
        // phase budget instead of each getting one.
        let until = self.until(timeout);
        let id = self.correlator.allocate();
        self.send(
            &OutgoingMessage::Request {
                id: id.clone(),
                method: method.to_owned(),
                params,
            },
            until,
        )?;
        self.correlator.expect(id.clone());
        loop {
            match self.next_event_until(until)? {
                Event::Response(matched) if matched.id == id => return Ok(matched),
                Event::Response(_) => {
                    return Err(self.fail(SessionError::Codec(CodecError::UnknownResponseId)));
                }
                Event::ServerStatus(_) => (),
                Event::Eof => return Err(self.fail(SessionError::Eof)),
            }
        }
    }

    /// Ends the session: `shutdown`, `exit`, a bounded wait for the process to
    /// leave, then a kill.
    ///
    /// The handshake is skipped when the session is already poisoned or the
    /// peer's stdout is already closed — there is nobody left to shake hands
    /// with and attempting it would only spend budget. The returned outcome
    /// never reports a clean exit for a session that was killed.
    pub(crate) fn close(mut self, grace: Duration) -> SessionOutcome {
        let mut handshake = false;
        if self.fatal.is_none() && !self.stdout_closed {
            let until = self.until(grace);
            handshake = self
                .request("shutdown", None, grace)
                .is_ok_and(|matched| matched.result.is_ok())
                && self
                    .send(
                        &OutgoingMessage::Notification {
                            method: "exit".to_owned(),
                            params: None,
                        },
                        until,
                    )
                    .is_ok();
        }
        // EOF on stdin is part of the exit contract for a peer waiting on it,
        // and this is the last moment anything is written.
        self.stdin.take();
        let exit_code = self.wait_for_exit(grace);
        let stop = match (self.fatal, exit_code) {
            // Nothing below is claimed for a child whose departure was never
            // confirmed: the guarantee that nothing survives the call is the
            // container's verified absence (gateway G3), not this side's kill.
            _ if !self.reaped => domain::SessionStop::KillUncertain,
            (Some(SessionError::Cancelled), _) => domain::SessionStop::Cancelled,
            (Some(SessionError::Timeout), _) => domain::SessionStop::Timeout,
            (Some(SessionError::Eof), _) => domain::SessionStop::Eof,
            (Some(_), _) => domain::SessionStop::Killed,
            // A peer that closed stdout on its own ended the session that way
            // even if it then exited cleanly: no handshake took place.
            (None, _) if self.stdout_closed && !handshake => domain::SessionStop::Eof,
            (None, Some(_)) => domain::SessionStop::Exited,
            (None, None) => domain::SessionStop::Killed,
        };
        SessionOutcome {
            exit_code,
            stop,
            messages_in: self.messages_in,
            messages_out: self.messages_out,
            bytes_in: self.bytes_in,
            bytes_out: self.bytes_out,
            stderr_len: self.stderr_len,
            stderr_sha256: sha256_text(&self.stderr_kept),
            stderr_truncated: self.stderr_truncated,
            server_requests: std::mem::take(&mut self.server_requests),
            notifications_dropped: self.notifications_dropped,
            late_responses: self.late_responses,
            status_transcript: std::mem::take(&mut self.status_transcript),
            fatal: self.fatal,
            declared_frame_bytes: self.decoder.declared_frame_bytes(),
            kill_error: self.kill_error.take(),
            reap_error: self.reap_error.take(),
            duration_ms: elapsed_ms(self.started),
        }
    }

    /// The absolute instant a wait must end: the sooner of the caller's
    /// timeout and the session deadline.
    pub(crate) fn until(&self, timeout: Duration) -> Instant {
        Instant::now()
            .checked_add(timeout)
            .map_or(self.budget.deadline, |until| {
                until.min(self.budget.deadline)
            })
    }

    fn recv_until(&mut self, until: Instant) -> Result<Option<RawMessage>, SessionError> {
        loop {
            if let Some(message) = self.inbox.pop_front() {
                return Ok(Some(message));
            }
            self.guard()?;
            if self.stdout_closed {
                return Ok(None);
            }
            let moved = self.step()?;
            if !self.inbox.is_empty() || self.stdout_closed {
                continue;
            }
            if Instant::now() >= until {
                return Err(self.fail(SessionError::Timeout));
            }
            if !moved {
                std::thread::sleep(POLL);
            }
        }
    }

    fn next_event_until(&mut self, until: Instant) -> Result<Event, SessionError> {
        loop {
            let Some(message) = self.recv_until(until)? else {
                return Ok(Event::Eof);
            };
            match message {
                RawMessage::Response { .. } => match self.correlator.accept(message) {
                    Ok(matched) => return Ok(Event::Response(matched)),
                    Err(error) if !error.is_fatal() => self.late_responses += 1,
                    Err(error) => return Err(self.fail(SessionError::Codec(error))),
                },
                RawMessage::Request { id, method, .. } => {
                    self.server_requests.push(method.clone());
                    let refusal = OutgoingMessage::Response {
                        id,
                        error: ResponseError::method_not_found(&method),
                    };
                    // The refusal is written inside the wait it interrupted, so
                    // it cannot spend more than the phase the caller allowed.
                    self.send(&refusal, until)?;
                }
                RawMessage::Notification { method, params } => {
                    if method == SERVER_STATUS {
                        return Ok(Event::ServerStatus(self.status(params)?));
                    }
                    self.notifications_dropped += 1;
                }
            }
        }
    }

    /// Parses the one notification this lifecycle depends on.
    ///
    /// A `serverStatus` whose payload does not match the closed DTO is fatal:
    /// the server advertised the capability, this client depends on it as its
    /// only readiness oracle (ADR-084 §5), and a shape this adapter cannot read
    /// is not something to guess at.
    fn status(&mut self, params: Option<serde_json::Value>) -> Result<StatusRecord, SessionError> {
        let parsed = params
            .ok_or(())
            .and_then(|value| {
                serde_json::from_value::<lsp_codec::ServerStatusParams>(value).map_err(|_| ())
            })
            .map_err(|()| self.fail(SessionError::Codec(CodecError::MalformedMessage)))?;
        let record = StatusRecord {
            quiescent: parsed.quiescent,
            health: domain::ServerHealth::from_wire(&parsed.health),
            elapsed_ms: elapsed_ms(self.started),
        };
        self.status_transcript.push(record.clone());
        Ok(record)
    }

    /// Waits up to `grace` for the process to exit, then kills and reaps it.
    /// Returns an exit code only when the process really reported one.
    fn wait_for_exit(&mut self, grace: Duration) -> Option<i32> {
        let until = Instant::now().checked_add(grace);
        loop {
            match self.child.0.try_wait() {
                Ok(Some(status)) => {
                    self.reaped = true;
                    return status.code();
                }
                Ok(None) => (),
                // A child this side cannot even ask about is not a child it may
                // report as gone: the kill below is attempted anyway and the
                // failure to confirm reaches the outcome as `KillUncertain`.
                Err(error) => {
                    self.record_io(|session| &mut session.reap_error, &error);
                    self.kill_and_reap();
                    return None;
                }
            }
            if until.is_none_or(|until| Instant::now() >= until) {
                self.kill_and_reap();
                // A killed process has no exit code of its own; reporting one
                // would describe a termination that did not happen.
                return None;
            }
            self.drain_quietly();
            std::thread::sleep(POLL);
        }
    }

    /// Signals the child and waits for it, recording what each call reported.
    ///
    /// Postconditions: `reaped` is `true` only if a `wait` really returned a
    /// status. Both errors are reduced to their [`std::io::ErrorKind`], which is
    /// a closed vocabulary with no pid, path or peer text in it.
    fn kill_and_reap(&mut self) {
        if let Err(error) = self.child.0.kill() {
            self.record_io(|session| &mut session.kill_error, &error);
        }
        match self.child.0.wait() {
            Ok(_) => self.reaped = true,
            Err(error) => self.record_io(|session| &mut session.reap_error, &error),
        }
    }

    /// Keeps the *first* error of a kind: a later attempt that fails the same
    /// way adds nothing, and overwriting would hide the one that started it.
    fn record_io(&mut self, field: fn(&mut Self) -> &mut Option<String>, error: &std::io::Error) {
        let kind = format!("{:?}", error.kind());
        let slot = field(self);
        if slot.is_none() {
            *slot = Some(kind);
        }
    }

    /// Keeps both output pipes moving while the peer shuts down, so a final log
    /// line cannot block its exit on a full pipe.
    ///
    /// The bytes are counted and, for stderr, retained up to the bound, but they
    /// are never decoded and never fail the session: the conversation is over,
    /// and turning a trailing byte into a kill would replace a clean exit with
    /// a manufactured one.
    fn drain_quietly(&mut self) {
        let mut buffer = [0u8; READ_CHUNK];
        for stdout in [true, false] {
            let mut total = 0usize;
            while total < READ_BUDGET {
                let stream: Option<&mut dyn Read> = if stdout {
                    self.stdout.as_mut().map(|s| s as &mut dyn Read)
                } else {
                    self.stderr.as_mut().map(|s| s as &mut dyn Read)
                };
                let Some(stream) = stream else { break };
                match stream.read(&mut buffer) {
                    Ok(0) => {
                        if stdout {
                            self.stdout_closed = true;
                            self.stdout.take();
                        } else {
                            self.stderr.take();
                        }
                        break;
                    }
                    Ok(count) => {
                        total += count;
                        if stdout {
                            self.bytes_in += count as u64;
                        } else {
                            self.stderr_len += count as u64;
                            let room = self
                                .budget
                                .max_stderr_bytes
                                .saturating_sub(self.stderr_kept.len());
                            let keep = room.min(count);
                            self.stderr_kept.extend_from_slice(&buffer[..keep]);
                            if keep < count {
                                self.stderr_truncated = true;
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
                    Err(_) => break,
                }
            }
        }
    }

    /// Poisons the session, kills the child and returns the reason.
    fn fail(&mut self, error: SessionError) -> SessionError {
        let reason = self.fatal.unwrap_or(error);
        self.fatal = Some(reason);
        self.stdin.take();
        self.kill_and_reap();
        reason
    }

    /// Refuses to do anything further once the session is over, cancelled or
    /// out of budget. Cancellation is checked before the deadline so a
    /// cancelled call is never reported as a timeout.
    fn guard(&mut self) -> Result<(), SessionError> {
        if let Some(error) = self.fatal {
            return Err(error);
        }
        if self.cancel.is_cancelled() {
            return Err(self.fail(SessionError::Cancelled));
        }
        if Instant::now() >= self.budget.deadline {
            return Err(self.fail(SessionError::Timeout));
        }
        Ok(())
    }

    /// One non-blocking pass over the three pipes. `true` when anything moved,
    /// so a caller only sleeps when the peer is genuinely idle.
    fn step(&mut self) -> Result<bool, SessionError> {
        let mut moved = self.write_step()?;
        moved |= self.read_stdout()?;
        moved |= self.read_stderr()?;
        Ok(moved)
    }

    fn write_step(&mut self) -> Result<bool, SessionError> {
        if self.written >= self.outgoing.len() {
            return Ok(false);
        }
        let end = (self.written + WRITE_CHUNK).min(self.outgoing.len());
        let written = match (&mut self.stdin, &self.outgoing) {
            (Some(stream), outgoing) => stream.write(&outgoing[self.written..end]),
            (None, _) => return Err(self.fail(SessionError::Io)),
        };
        match written {
            Ok(0) => Err(self.write_died()),
            Ok(count) => {
                self.written += count;
                self.bytes_out += count as u64;
                Ok(true)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(false)
            }
            Err(_) => Err(self.write_died()),
        }
    }

    /// Classifies a write that the peer will not accept any more of.
    ///
    /// A peer that is gone breaks both pipes at once, and which half this side
    /// notices first is a race between a read and a write. So the read is
    /// resolved before the verdict: if stdout is at end of file the session
    /// ended in [`SessionError::Eof`] — the peer left — and only a write that
    /// fails against a stdout still open is [`SessionError::Io`]. Without this,
    /// the same event is published as `Eof` or as a kill depending on timing.
    fn write_died(&mut self) -> SessionError {
        let _ = self.read_stdout();
        let error = if self.stdout_closed {
            SessionError::Eof
        } else {
            SessionError::Io
        };
        self.fail(error)
    }

    fn read_stdout(&mut self) -> Result<bool, SessionError> {
        let mut buffer = [0u8; READ_CHUNK];
        let mut total = 0usize;
        let mut moved = false;
        while total < READ_BUDGET {
            let read = match &mut self.stdout {
                Some(stream) => stream.read(&mut buffer),
                None => return Ok(moved),
            };
            match read {
                Ok(0) => {
                    self.stdout_closed = true;
                    self.stdout.take();
                    return Ok(true);
                }
                Ok(count) => {
                    total += count;
                    moved = true;
                    self.bytes_in += count as u64;
                    if self.bytes_in > self.budget.max_stdout_bytes as u64 {
                        return Err(self.fail(SessionError::ByteLimit));
                    }
                    match self.decoder.feed(&buffer[..count]) {
                        Ok(messages) => {
                            self.messages_in += messages.len() as u64;
                            self.inbox.extend(messages);
                        }
                        Err(error) => return Err(self.fail(SessionError::Codec(error))),
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(moved),
                Err(_) => return Err(self.fail(SessionError::Io)),
            }
        }
        Ok(moved)
    }

    fn read_stderr(&mut self) -> Result<bool, SessionError> {
        let mut buffer = [0u8; READ_CHUNK];
        let mut total = 0usize;
        let mut moved = false;
        while total < READ_BUDGET {
            let read = match &mut self.stderr {
                Some(stream) => stream.read(&mut buffer),
                None => return Ok(moved),
            };
            match read {
                Ok(0) => {
                    self.stderr.take();
                    return Ok(true);
                }
                Ok(count) => {
                    total += count;
                    moved = true;
                    self.stderr_len += count as u64;
                    let room = self
                        .budget
                        .max_stderr_bytes
                        .saturating_sub(self.stderr_kept.len());
                    let keep = room.min(count);
                    self.stderr_kept.extend_from_slice(&buffer[..keep]);
                    if keep < count {
                        self.stderr_truncated = true;
                    }
                    if self.stderr_len > self.budget.max_stderr_bytes as u64 {
                        return Err(self.fail(SessionError::ByteLimit));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(moved),
                Err(_) => return Err(self.fail(SessionError::Io)),
            }
        }
        Ok(moved)
    }

    /// The total stderr bytes observed, the bytes retained, and whether the
    /// retained buffer dropped any. The bytes themselves stay inside.
    #[cfg(test)]
    fn stderr_evidence(&self) -> (u64, usize, bool) {
        (
            self.stderr_len,
            self.stderr_kept.len(),
            self.stderr_truncated,
        )
    }

    #[cfg(test)]
    fn server_requests(&self) -> &[String] {
        &self.server_requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_application::NeverCancel;

    type Failure = Box<dyn std::error::Error>;

    /// Fixed, trusted host utilities standing in for the guest peer, with a
    /// cleared environment and a fixed working directory. No project code runs
    /// here and no argument comes from a peer: every script below is a literal
    /// of this test module. The session itself never builds a command — the
    /// gateway hands it one built from a closed phase.
    fn peer(program: &str, arguments: &[&str]) -> Command {
        let mut command = Command::new(program);
        command.env_clear().current_dir("/").args(arguments);
        command
    }

    fn shell(script: &str) -> Command {
        peer("/bin/sh", &["-c", script])
    }

    fn budget(seconds: u64) -> SessionBudget {
        SessionBudget::standard(Instant::now() + Duration::from_secs(seconds))
    }

    fn notification(method: &str) -> OutgoingMessage {
        OutgoingMessage::Notification {
            method: method.to_owned(),
            params: None,
        }
    }

    /// A literal LSP frame for a `printf` script. The length is computed here,
    /// so a hand-counted header can never drift from its body.
    fn frame_literal(body: &str) -> String {
        format!(
            "Content-Length: {}\\r\\n\\r\\n{}",
            body.len(),
            body.replace('%', "%%")
        )
    }

    fn status_record(event: &Event) -> Option<&StatusRecord> {
        match event {
            Event::ServerStatus(record) => Some(record),
            _ => None,
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn an_echoing_peer_round_trips_a_framed_message() -> Result<(), Failure> {
        let cancel = NeverCancel;
        let mut session = LspSession::open(peer("/bin/cat", &[]), budget(10), &cancel)
            .map_err(|error| format!("{error:?}"))?;
        let until = session.until(Duration::from_secs(5));
        session
            .send(&notification("initialized"), until)
            .map_err(|error| format!("{error:?}"))?;
        let message = session
            .recv(Duration::from_secs(5))
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            message,
            Some(RawMessage::Notification {
                method: "initialized".to_owned(),
                params: None,
            })
        );
        // `cat` also echoes the shutdown handshake, so the inbound count after
        // `close` is not this test's subject; the round trip and the untouched
        // stderr evidence are.
        let outcome = session.close(Duration::from_secs(5));
        assert!(outcome.messages_in >= 1);
        assert!(outcome.bytes_out > 0 && outcome.bytes_in > 0);
        assert_eq!(outcome.stderr_len, 0);
        assert_eq!(outcome.stderr_sha256, sha256_text(&[]));
        assert_eq!(outcome.fatal, None);
        assert_eq!(outcome.exit_code, Some(0), "the peer left on stdin EOF");
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_frame_over_the_bound_is_fatal_and_the_peer_is_killed() -> Result<(), Failure> {
        let cancel = NeverCancel;
        let script = format!(
            "printf 'Content-Length: {}\\r\\n\\r\\n'; /bin/cat > /dev/null",
            domain::MAX_FRAME_BYTES + 1
        );
        let mut session = LspSession::open(shell(&script), budget(10), &cancel)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            session.recv(Duration::from_secs(5)).err(),
            Some(SessionError::Codec(CodecError::FrameLimit)),
            "the decoder enforces the frame bound while parsing the header"
        );
        let outcome = session.close(Duration::from_secs(2));
        assert_eq!(outcome.stop, domain::SessionStop::Killed);
        assert_eq!(outcome.exit_code, None);
        assert_eq!(
            outcome.fatal,
            Some(SessionError::Codec(CodecError::FrameLimit))
        );
        assert_eq!(
            outcome.declared_frame_bytes,
            Some(domain::MAX_FRAME_BYTES as u64 + 1),
            "the length the peer declared is published, not only the refusal"
        );
        assert_eq!(
            (outcome.kill_error.as_deref(), outcome.reap_error.as_deref()),
            (None, None),
            "the kill and the reap both reported success"
        );
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_message_flood_hits_the_per_job_count_and_kills_the_peer() -> Result<(), Failure> {
        let cancel = NeverCancel;
        let mut limits = budget(20);
        limits.max_messages = 2;
        let frame = frame_literal("{\"jsonrpc\":\"2.0\",\"method\":\"n\"}");
        let script = format!(
            "printf '{frame}'; sleep 0.2; printf '{frame}'; sleep 0.2; printf '{frame}'; \
             /bin/cat > /dev/null"
        );
        let mut session =
            LspSession::open(shell(&script), limits, &cancel).map_err(|e| format!("{e:?}"))?;
        let mut seen = 0usize;
        let error = loop {
            match session.recv(Duration::from_secs(5)) {
                Ok(Some(_)) => seen += 1,
                Ok(None) => break None,
                Err(error) => break Some(error),
            }
        };
        // The budget is the subject, not the chunking: the decoder discards the
        // messages produced by the same `feed` call as a fatal limit, so how many
        // of the three frames were delivered before the refusal depends on how
        // the peer's bytes happened to be split across reads. What must hold
        // whatever the split is that no more than the budget was ever delivered
        // and that the excess killed the session.
        assert!(
            seen <= limits.max_messages,
            "{seen} messages delivered under a budget of {}",
            limits.max_messages
        );
        assert_eq!(error, Some(SessionError::Codec(CodecError::MessageLimit)));
        let outcome = session.close(Duration::from_secs(2));
        assert_eq!(outcome.stop, domain::SessionStop::Killed);
        assert!(outcome.messages_in <= limits.max_messages as u64);
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn the_message_count_bounds_this_side_too() -> Result<(), Failure> {
        let cancel = NeverCancel;
        let mut limits = budget(20);
        limits.max_messages = 3;
        let mut session = LspSession::open(peer("/bin/cat", &[]), limits, &cancel)
            .map_err(|error| format!("{error:?}"))?;
        let until = session.until(Duration::from_secs(5));
        for _ in 0..3 {
            session
                .send(&notification("initialized"), until)
                .map_err(|error| format!("{error:?}"))?;
        }
        assert_eq!(
            session.send(&notification("initialized"), until).err(),
            Some(SessionError::MessageLimit)
        );
        let outcome = session.close(Duration::from_secs(2));
        assert_eq!(outcome.messages_out, 3);
        assert_eq!(outcome.stop, domain::SessionStop::Killed);
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn an_outgoing_frame_over_the_bound_is_refused_before_a_byte_is_written() -> Result<(), Failure>
    {
        let cancel = NeverCancel;
        let mut limits = budget(10);
        limits.max_frame_bytes = 256;
        let mut session = LspSession::open(peer("/bin/cat", &[]), limits, &cancel)
            .map_err(|error| format!("{error:?}"))?;
        let huge = OutgoingMessage::Notification {
            method: "x".repeat(1024),
            params: None,
        };
        let until = session.until(Duration::from_secs(5));
        assert_eq!(
            session.send(&huge, until).err(),
            Some(SessionError::FrameLimit)
        );
        let outcome = session.close(Duration::from_secs(2));
        assert_eq!(outcome.bytes_out, 0);
        assert_eq!(outcome.stop, domain::SessionStop::Killed);
        Ok(())
    }

    /// The write half is bounded by the phase, not only by the session.
    ///
    /// The peer never reads its stdin, so a frame larger than the pipe buffer
    /// cannot drain. Before the phase bound existed this consumed the whole
    /// session deadline and was then published as a claim about the server's
    /// readiness; now it is the timeout of the phase that asked for it.
    #[test]
    #[cfg(target_os = "macos")]
    fn a_peer_that_never_reads_stdin_exhausts_the_phase_and_not_the_session() -> Result<(), Failure>
    {
        let cancel = NeverCancel;
        let started = Instant::now();
        let mut session = LspSession::open(peer("/bin/sleep", &["30"]), budget(30), &cancel)
            .map_err(|error| format!("{error:?}"))?;
        let unread = OutgoingMessage::Notification {
            method: "x".repeat(512 * 1024),
            params: None,
        };
        let until = session.until(Duration::from_millis(200));
        assert_eq!(
            session.send(&unread, until).err(),
            Some(SessionError::Timeout)
        );
        let outcome = session.close(Duration::from_secs(2));
        assert_eq!(outcome.stop, domain::SessionStop::Timeout);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the wait is the phase budget, not the session deadline"
        );
        assert!(
            outcome.bytes_out > 0 && outcome.messages_out == 0,
            "the frame was started and never completed: {} bytes, {} messages",
            outcome.bytes_out,
            outcome.messages_out
        );
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_stderr_flood_stays_bounded_and_ends_the_session() -> Result<(), Failure> {
        let cancel = NeverCancel;
        let mut limits = budget(20);
        limits.max_stderr_bytes = 64 * 1024;
        let mut session = LspSession::open(
            // No `exec`: the shell keeps the stdout pipe open, so this is a
            // stderr flood and not an end of stdout wearing its clothes.
            shell("/usr/bin/yes rust-analyzer-stderr 1>&2"),
            limits,
            &cancel,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            session.recv(Duration::from_secs(10)).err(),
            Some(SessionError::ByteLimit)
        );
        let (observed, kept, truncated) = session.stderr_evidence();
        assert!(kept <= 64 * 1024, "the retained buffer stays in bounds");
        assert!(observed > kept as u64, "overflow is counted, not retained");
        assert!(truncated);
        let outcome = session.close(Duration::from_secs(2));
        assert_eq!(outcome.stop, domain::SessionStop::Killed);
        assert!(outcome.stderr_truncated);
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_peer_that_never_answers_times_out_and_is_joined() -> Result<(), Failure> {
        let cancel = NeverCancel;
        let started = Instant::now();
        let mut session = LspSession::open(peer("/bin/sleep", &["30"]), budget(30), &cancel)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            session
                .request("initialize", None, Duration::from_millis(200))
                .err(),
            Some(SessionError::Timeout)
        );
        let outcome = session.close(Duration::from_secs(2));
        assert_eq!(outcome.stop, domain::SessionStop::Timeout);
        assert_eq!(outcome.exit_code, None);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the wait is the request budget, not the peer's lifetime"
        );
        Ok(())
    }

    struct CancelAfter(Instant);
    impl ExecutionCancellation for CancelAfter {
        fn is_cancelled(&self) -> bool {
            Instant::now() >= self.0
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn cancellation_mid_request_kills_and_joins_the_peer() -> Result<(), Failure> {
        let cancel = CancelAfter(Instant::now() + Duration::from_millis(100));
        let started = Instant::now();
        let mut session = LspSession::open(peer("/bin/sleep", &["30"]), budget(30), &cancel)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            session
                .request("initialize", None, Duration::from_secs(20))
                .err(),
            Some(SessionError::Cancelled),
            "cancellation is checked before the deadline, so it is never a timeout"
        );
        let outcome = session.close(Duration::from_secs(2));
        assert_eq!(outcome.stop, domain::SessionStop::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(10));
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_server_request_is_answered_with_method_not_found_and_nothing_else() -> Result<(), Failure>
    {
        let cancel = NeverCancel;
        let request = frame_literal(
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"workspace/applyEdit\",\"params\":{}}",
        );
        let status = frame_literal(
            "{\"jsonrpc\":\"2.0\",\"method\":\"experimental/serverStatus\",\
             \"params\":{\"health\":\"ok\",\"quiescent\":true}}",
        );
        // The peer keeps reading stdin after printing, so the refusal this
        // session writes has somewhere to go. It discards it, which is the
        // point: no server→client request is ever granted.
        let script = format!("printf '{request}{status}'; /bin/cat > /dev/null");
        let mut session = LspSession::open(shell(&script), budget(10), &cancel)
            .map_err(|error| format!("{error:?}"))?;
        let event = session
            .next_event(Duration::from_secs(5))
            .map_err(|error| format!("{error:?}"))?;
        let record = status_record(&event).ok_or("expected the serverStatus notification")?;
        assert!(record.quiescent);
        assert_eq!(record.health, Some(domain::ServerHealth::Ok));
        assert_eq!(session.server_requests(), ["workspace/applyEdit"]);
        // This peer answers nothing, so the handshake times out inside the
        // grace and the session is killed; that is the point of the grace.
        let outcome = session.close(Duration::from_millis(300));
        assert_eq!(outcome.server_requests, ["workspace/applyEdit"]);
        assert_eq!(
            outcome.messages_out, 2,
            "the refusal and the shutdown request; nothing was granted"
        );
        assert_eq!(outcome.stop, domain::SessionStop::Timeout);
        assert_eq!(outcome.status_transcript.len(), 1);
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_malformed_server_status_is_fatal_rather_than_a_guessed_readiness() -> Result<(), Failure> {
        let cancel = NeverCancel;
        let status = frame_literal(
            "{\"jsonrpc\":\"2.0\",\"method\":\"experimental/serverStatus\",\
             \"params\":{\"health\":\"ok\"}}",
        );
        let script = format!("printf '{status}'; /bin/cat > /dev/null");
        let mut session = LspSession::open(shell(&script), budget(10), &cancel)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            session.next_event(Duration::from_secs(5)).err(),
            Some(SessionError::Codec(CodecError::MalformedMessage))
        );
        assert_eq!(
            session.close(Duration::from_secs(2)).stop,
            domain::SessionStop::Killed
        );
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn an_unsolicited_notification_is_counted_and_dropped() -> Result<(), Failure> {
        let cancel = NeverCancel;
        let published = frame_literal(
            "{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/publishDiagnostics\",\
             \"params\":{\"uri\":\"file:///source/src/lib.rs\",\"diagnostics\":[]}}",
        );
        let status = frame_literal(
            "{\"jsonrpc\":\"2.0\",\"method\":\"experimental/serverStatus\",\
             \"params\":{\"health\":\"warning\",\"quiescent\":false}}",
        );
        let script = format!("printf '{published}{status}'; /bin/cat > /dev/null");
        let mut session = LspSession::open(shell(&script), budget(10), &cancel)
            .map_err(|error| format!("{error:?}"))?;
        let event = session
            .next_event(Duration::from_secs(5))
            .map_err(|error| format!("{error:?}"))?;
        let record = status_record(&event).ok_or("expected the serverStatus notification")?;
        assert!(!record.quiescent);
        assert_eq!(record.health, Some(domain::ServerHealth::Warning));
        let outcome = session.close(Duration::from_secs(2));
        assert_eq!(
            outcome.notifications_dropped, 1,
            "publishDiagnostics is never consumed by this lifecycle"
        );
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn end_of_stdout_without_a_shutdown_is_reported_as_eof() -> Result<(), Failure> {
        let cancel = NeverCancel;
        let mut session = LspSession::open(peer("/usr/bin/true", &[]), budget(10), &cancel)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(session.recv(Duration::from_secs(5)), Ok(None));
        let outcome = session.close(Duration::from_secs(2));
        assert_eq!(outcome.stop, domain::SessionStop::Eof);
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.fatal, None);
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_request_cut_short_by_end_of_stdout_is_an_eof_failure() -> Result<(), Failure> {
        let cancel = NeverCancel;
        let mut session = LspSession::open(peer("/usr/bin/true", &[]), budget(10), &cancel)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            session
                .request("initialize", None, Duration::from_secs(5))
                .err(),
            Some(SessionError::Eof)
        );
        assert_eq!(
            session.close(Duration::from_secs(2)).stop,
            domain::SessionStop::Eof
        );
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_clean_shutdown_handshake_reports_the_peer_exit_code() -> Result<(), Failure> {
        let cancel = NeverCancel;
        // Answers exactly one request with `result: null`, then leaves once its
        // stdin reader sees EOF.
        let answer = frame_literal("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":null}");
        let script = format!("printf '{answer}'; /bin/cat > /dev/null");
        let session = LspSession::open(shell(&script), budget(20), &cancel)
            .map_err(|error| format!("{error:?}"))?;
        let outcome = session.close(Duration::from_secs(5));
        assert_eq!(outcome.stop, domain::SessionStop::Exited);
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.messages_out, 2, "shutdown and exit");
        assert_eq!(outcome.fatal, None);
        Ok(())
    }

    #[test]
    fn every_session_error_is_terminal_and_named() {
        // A compile-time inventory: adding a variant without deciding what it
        // means for the outcome fails this match.
        for error in [
            SessionError::Codec(CodecError::MalformedHeader),
            SessionError::Encode,
            SessionError::FrameLimit,
            SessionError::MessageLimit,
            SessionError::ByteLimit,
            SessionError::Io,
            SessionError::Timeout,
            SessionError::Cancelled,
            SessionError::Eof,
        ] {
            let stop = match error {
                SessionError::Cancelled => domain::SessionStop::Cancelled,
                SessionError::Timeout => domain::SessionStop::Timeout,
                SessionError::Eof => domain::SessionStop::Eof,
                SessionError::Codec(_)
                | SessionError::Encode
                | SessionError::FrameLimit
                | SessionError::MessageLimit
                | SessionError::ByteLimit
                | SessionError::Io => domain::SessionStop::Killed,
            };
            assert_ne!(stop, domain::SessionStop::Exited);
            assert_ne!(stop, domain::SessionStop::NotStarted);
        }
    }
}
