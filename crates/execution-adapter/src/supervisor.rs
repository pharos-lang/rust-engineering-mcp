//! Only this module spawns product child processes; arguments come from gateway enums.
use rust_engineering_application::{ExecutionCancellation, ExecutionError};
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Exited,
    TimedOut,
    Cancelled,
    OutputLimit,
}
pub struct Capture {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub stop: Stop,
    pub duration_ms: u64,
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Readers {
    stop: Arc<AtomicBool>,
    handles: Vec<thread::JoinHandle<()>>,
}
impl Drop for Readers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}
#[cfg(target_os = "macos")]
fn nonblocking(fd: &impl std::os::fd::AsFd) -> Result<(), ExecutionError> {
    let flags = rustix::fs::fcntl_getfl(fd).map_err(|_| ExecutionError::Infrastructure)?;
    rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)
        .map_err(|_| ExecutionError::Infrastructure)
}
#[cfg(not(target_os = "macos"))]
fn nonblocking<T>(_: &T) -> Result<(), ExecutionError> {
    Err(ExecutionError::Unavailable)
}

pub fn run(
    command: Command,
    deadline: Duration,
    limit: usize,
    cancel: &dyn ExecutionCancellation,
) -> Result<Capture, ExecutionError> {
    run_with_input(command, deadline, limit, cancel, &[])
}

// ADR-031 bounds source bytes to 16 MiB plus USTAR headers/padding/directories.
// Reject larger archives before creating a process or copying the input.
const MAX_INPUT_BYTES: usize = 24 * 1024 * 1024;
const INPUT_CHUNK_BYTES: usize = 8192;

/// Writes only from the supervisor thread. Each nonblocking step leaves the
/// cancellation/deadline/output checks runnable while reader threads drain both
/// output pipes; there is no detached writer or additional process boundary.
struct InputWriter<'a, W> {
    stream: Option<W>,
    input: &'a [u8],
    written: usize,
}

impl<'a, W: Write> InputWriter<'a, W> {
    fn step(&mut self) -> std::io::Result<bool> {
        if self.written == self.input.len() {
            self.stream.take();
            return Ok(false);
        }
        let Some(stream) = self.stream.as_mut() else {
            return Err(std::io::Error::other("input pipe unavailable"));
        };
        let end = self.written + INPUT_CHUNK_BYTES.min(self.input.len() - self.written);
        match stream.write(&self.input[self.written..end]) {
            Ok(0) => Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "input pipe made no progress",
            )),
            Ok(count) => {
                self.written += count;
                if self.written == self.input.len() {
                    // EOF is essential: a reader such as tar/cat may await it
                    // before exiting. ChildStdin is unbuffered; no flush needed.
                    self.stream.take();
                }
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
            Err(error) => Err(error),
        }
    }

    fn complete(&self) -> bool {
        self.written == self.input.len()
    }
}

pub fn run_with_input(
    mut command: Command,
    deadline: Duration,
    limit: usize,
    cancel: &dyn ExecutionCancellation,
    input: &[u8],
) -> Result<Capture, ExecutionError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(ExecutionError::Denied);
    }
    command
        .stdin(if input.is_empty() {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = ChildGuard(command.spawn().map_err(|_| ExecutionError::Unavailable)?);
    let stdin = if input.is_empty() {
        None
    } else {
        let stdin = child.0.stdin.take().ok_or(ExecutionError::Infrastructure)?;
        nonblocking(&stdin)?;
        Some(stdin)
    };
    let mut writer = InputWriter {
        stream: stdin,
        input,
        written: 0,
    };
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
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let mut readers = Readers {
        stop: Arc::new(AtomicBool::new(false)),
        handles: Vec::new(),
    };
    let exceeded = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::sync_channel(2);
    for (is_stdout, mut stream) in [
        (true, Box::new(stdout) as Box<dyn Read + Send>),
        (false, Box::new(stderr) as Box<dyn Read + Send>),
    ] {
        let sender = sender.clone();
        let exceeded = Arc::clone(&exceeded);
        let stop = Arc::clone(&readers.stop);
        readers.handles.push(thread::spawn(move || {
            let mut captured = Vec::new();
            let mut truncated = false;
            let mut buffer = [0; 8192];
            let mut error = false;
            loop {
                if stop.load(Ordering::Acquire) {
                    error = true;
                    break;
                }
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        let take = n.min(limit.saturating_sub(captured.len()));
                        captured.extend_from_slice(&buffer[..take]);
                        if take < n {
                            truncated = true;
                            exceeded.store(true, Ordering::Release);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => {
                        error = true;
                        break;
                    }
                }
            }
            let _ = sender.send((is_stdout, captured, truncated, error));
        }));
    }
    drop(sender);
    let started = Instant::now();
    let (stop, code) = loop {
        if cancel.is_cancelled() {
            break (Stop::Cancelled, None);
        }
        if exceeded.load(Ordering::Acquire) {
            break (Stop::OutputLimit, None);
        }
        if started.elapsed() >= deadline {
            break (Stop::TimedOut, None);
        }
        if let Some(status) = child
            .0
            .try_wait()
            .map_err(|_| ExecutionError::Infrastructure)?
        {
            break (Stop::Exited, status.code());
        }
        let progressed = writer.step().map_err(|_| ExecutionError::Infrastructure)?;
        if !progressed {
            thread::sleep(Duration::from_millis(5));
        }
    };
    // Synchronous writer is now stopped; close stdin before waiting/killing.
    // Dropping InputWriter also closes it on every early-return error path.
    writer.stream.take();
    if stop == Stop::Exited && !writer.complete() {
        return Err(ExecutionError::Infrastructure);
    }
    if stop != Stop::Exited {
        if child.0.kill().is_err()
            && child
                .0
                .try_wait()
                .map_err(|_| ExecutionError::Infrastructure)?
                .is_none()
        {
            return Err(ExecutionError::Infrastructure);
        }
        child.0.wait().map_err(|_| ExecutionError::Infrastructure)?;
    }
    let mut capture = Capture {
        code,
        stdout: Vec::new(),
        stderr: Vec::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        stop,
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    };
    for _ in 0..2 {
        let (out, bytes, truncated, error) = receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| ExecutionError::Infrastructure)?;
        if error {
            return Err(ExecutionError::Infrastructure);
        }
        if out {
            capture.stdout = bytes;
            capture.stdout_truncated = truncated;
        } else {
            capture.stderr = bytes;
            capture.stderr_truncated = truncated;
        }
    }
    // A fast child can exit between the last budget poll and pipe draining.
    if capture.stop == Stop::Exited && (capture.stdout_truncated || capture.stderr_truncated) {
        capture.stop = Stop::OutputLimit;
        capture.code = None;
    }
    Ok(capture)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_application::NeverCancel;
    use std::collections::VecDeque;
    use std::io;

    struct ScriptedWriter {
        actions: VecDeque<io::Result<usize>>,
        received: Arc<std::sync::Mutex<Vec<u8>>>,
        closed: Arc<AtomicBool>,
    }
    impl Write for ScriptedWriter {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            let accepted = self.actions.pop_front().unwrap_or(Ok(input.len()))?;
            assert!(accepted <= input.len());
            self.received
                .lock()
                .map_err(|_| io::Error::other("test mutex"))?
                .extend_from_slice(&input[..accepted]);
            Ok(accepted)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Drop for ScriptedWriter {
        fn drop(&mut self) {
            self.closed.store(true, Ordering::Release);
        }
    }

    #[test]
    fn nonblocking_steps_preserve_input_after_short_writes_and_retries() -> io::Result<()> {
        let received = Arc::new(std::sync::Mutex::new(Vec::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let mut writer = InputWriter {
            stream: Some(ScriptedWriter {
                actions: VecDeque::from([
                    Ok(2),
                    Err(io::ErrorKind::WouldBlock.into()),
                    Err(io::ErrorKind::Interrupted.into()),
                    Ok(3),
                ]),
                received: received.clone(),
                closed: closed.clone(),
            }),
            input: b"abcde",
            written: 0,
        };
        assert!(writer.step()?);
        assert!(!writer.step()?);
        assert!(!writer.step()?);
        assert!(!closed.load(Ordering::Acquire));
        assert!(writer.step()?);
        assert!(writer.complete());
        assert!(closed.load(Ordering::Acquire));
        assert_eq!(
            *received
                .lock()
                .map_err(|_| io::Error::other("test mutex"))?,
            b"abcde"
        );
        Ok(())
    }

    #[test]
    fn each_input_step_is_bounded_and_zero_write_is_an_error() -> io::Result<()> {
        let input = vec![b'x'; INPUT_CHUNK_BYTES * 2];
        let mut writer = InputWriter {
            stream: Some(io::sink()),
            input: &input,
            written: 0,
        };
        assert!(writer.step()?);
        assert_eq!(writer.written, INPUT_CHUNK_BYTES);
        assert!(!writer.complete());
        let mut writer = InputWriter {
            stream: Some(&mut [][..]),
            input: b"x",
            written: 0,
        };
        assert_eq!(
            writer.step().err().map(|error| error.kind()),
            Some(io::ErrorKind::WriteZero)
        );
        Ok(())
    }

    #[test]
    fn oversized_input_is_rejected_before_attempting_spawn() {
        let result = run_with_input(
            Command::new("/nonexistent-rust-mcp-supervisor-test-program"),
            Duration::from_secs(1),
            1,
            &NeverCancel,
            &vec![0; MAX_INPUT_BYTES + 1],
        );
        assert!(matches!(result, Err(ExecutionError::Denied)));
    }

    // These host fixtures are fixed trusted system utilities, with cleared
    // environment and cwd. They exercise pipes only; no project code runs here.
    #[cfg(target_os = "macos")]
    fn trusted(program: &str) -> Command {
        let mut command = Command::new(program);
        command.env_clear().current_dir("/");
        command
    }

    #[cfg(target_os = "macos")]
    fn capture(result: Result<Capture, ExecutionError>) -> io::Result<Capture> {
        result.map_err(|error| io::Error::other(format!("{error:?}")))
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn actual_pipe_drains_output_while_sending_input_and_closes_eof() -> io::Result<()> {
        let input: Vec<u8> = (0..1024 * 1024).map(|index| (index % 251) as u8).collect();
        let output = capture(run_with_input(
            trusted("/bin/cat"),
            Duration::from_secs(5),
            input.len(),
            &NeverCancel,
            &input,
        ))?;
        assert_eq!(output.stop, Stop::Exited);
        assert_eq!(output.code, Some(0));
        assert_eq!(output.stdout, input);
        assert!(output.stderr.is_empty());
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn empty_input_preserves_null_stdin_behavior() -> io::Result<()> {
        let output = capture(run(
            trusted("/bin/cat"),
            Duration::from_secs(1),
            1024,
            &NeverCancel,
        ))?;
        assert_eq!(output.stop, Stop::Exited);
        assert_eq!(output.code, Some(0));
        assert!(output.stdout.is_empty());
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn blocked_child_input_obeys_timeout_and_joins_readers() -> io::Result<()> {
        let mut command = trusted("/bin/sleep");
        command.arg("10");
        let started = Instant::now();
        let output = capture(run_with_input(
            command,
            Duration::from_millis(30),
            1024,
            &NeverCancel,
            &vec![b'x'; 1024 * 1024],
        ))?;
        assert_eq!(output.stop, Stop::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }

    #[cfg(target_os = "macos")]
    struct CancelAfter(Instant);
    #[cfg(target_os = "macos")]
    impl ExecutionCancellation for CancelAfter {
        fn is_cancelled(&self) -> bool {
            Instant::now() >= self.0
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn blocked_child_input_obeys_cancellation() -> io::Result<()> {
        let mut command = trusted("/bin/sleep");
        command.arg("10");
        let started = Instant::now();
        let cancel = CancelAfter(started + Duration::from_millis(30));
        let output = capture(run_with_input(
            command,
            Duration::from_secs(5),
            1024,
            &cancel,
            &vec![b'x'; 1024 * 1024],
        ))?;
        assert_eq!(output.stop, Stop::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn early_child_exit_cannot_certify_an_incomplete_transfer() {
        let result = run_with_input(
            trusted("/usr/bin/true"),
            Duration::from_secs(1),
            1024,
            &NeverCancel,
            &vec![b'x'; 1024 * 1024],
        );
        assert!(matches!(result, Err(ExecutionError::Infrastructure)));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn simultaneous_input_and_output_overflow_remains_bounded() -> io::Result<()> {
        let output = capture(run_with_input(
            trusted("/bin/cat"),
            Duration::from_secs(5),
            64,
            &NeverCancel,
            &vec![b'x'; 1024 * 1024],
        ))?;
        assert_eq!(output.stop, Stop::OutputLimit);
        assert_eq!(output.stdout.len(), 64);
        assert!(output.stdout_truncated);
        assert!(output.code.is_none());
        Ok(())
    }
}
