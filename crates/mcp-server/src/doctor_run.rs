//! Join observations and gateway cleanup after cancellation, never detach work.
use crate::doctor::{Invocation, Report};
use rust_engineering_application::{ExecutionCancellation, OperationControl, ProjectError};
use rust_engineering_domain::OperationalErrorCode;
use std::{
    io::{self, Write},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
const MAX_REPORT: usize = 128 * 1024;
struct Control {
    cancelled: AtomicBool,
    deadline: Instant,
}
impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(ProjectError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(ProjectError::Rejected(OperationalErrorCode::CommandTimeout))
        } else {
            Ok(())
        }
    }
}
impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        self.check().is_err()
    }
}
#[cfg(unix)]
struct Signals {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
    hangup: tokio::signal::unix::Signal,
}
#[cfg(unix)]
impl Signals {
    fn register() -> Result<Self, ProjectError> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            interrupt: signal(SignalKind::interrupt()).map_err(|_| ProjectError::Internal)?,
            terminate: signal(SignalKind::terminate()).map_err(|_| ProjectError::Internal)?,
            hangup: signal(SignalKind::hangup()).map_err(|_| ProjectError::Internal)?,
        })
    }
    async fn receive(&mut self) {
        tokio::select! {_=self.interrupt.recv()=>(),_=self.terminate.recv()=>(),_=self.hangup.recv()=>()}
    }
}
#[cfg(windows)]
struct Signals {
    interrupt: tokio::signal::windows::CtrlC,
    terminate: tokio::signal::windows::CtrlBreak,
}
#[cfg(windows)]
impl Signals {
    fn register() -> Result<Self, ProjectError> {
        Ok(Self {
            interrupt: tokio::signal::windows::ctrl_c().map_err(|_| ProjectError::Internal)?,
            terminate: tokio::signal::windows::ctrl_break().map_err(|_| ProjectError::Internal)?,
        })
    }
    async fn receive(&mut self) {
        tokio::select! {_=self.interrupt.recv()=>(),_=self.terminate.recv()=>()}
    }
}
#[cfg(not(any(unix, windows)))]
struct Signals;
#[cfg(not(any(unix, windows)))]
impl Signals {
    fn register() -> Result<Self, ProjectError> {
        Err(ProjectError::Rejected(
            OperationalErrorCode::UnsupportedPlatform,
        ))
    }
    async fn receive(&mut self) {
        std::future::pending::<()>().await;
    }
}
async fn observe(invocation: Invocation, started: Instant, signals: &mut Signals) -> Report {
    let active = invocation.active;
    let deadline = started + Duration::from_secs(if active { 900 } else { 120 });
    let control = Arc::new(Control {
        cancelled: AtomicBool::new(false),
        deadline,
    });
    let work_control = Arc::clone(&control);
    let mut work = tokio::task::spawn_blocking(move || {
        crate::doctor::inspect(&invocation, work_control.as_ref())
    });
    let result = tokio::select! {
        result=&mut work=>result,
        _=signals.receive()=>{control.cancelled.store(true,Ordering::Release);work.await},
        _=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>{work.await},
    };
    let mut report = match result {
        Ok(Ok(report)) => report,
        Ok(Err(error)) => Report::failure(active, error),
        Err(_) => Report::worker_failure(active),
    };
    if let Err(error) = control.check() {
        report.record_failure(error);
    }
    report
}
pub(crate) fn run(invocation: Invocation) -> ExitCode {
    let started = Instant::now();
    let json = invocation.json;
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::FAILURE,
    };
    let code = runtime.block_on(async {
        // Registration precedes all work and remains live through response delivery.
        let mut signals = match Signals::register() {
            Ok(signals) => signals,
            Err(_) => return ExitCode::FAILURE,
        };
        let report = observe(invocation, started, &mut signals).await;
        let (bytes, code) = match render(report, json, started) {
            Ok(value) => value,
            Err(()) => return ExitCode::FAILURE,
        };
        // All observation and gateway cleanup is joined before output starts.
        // Only this bounded response write may outlive its await on a stalled
        // consumer. Runtime shutdown is bounded; returning from main ends it.
        // It owns no catalog administration or execution resources.
        let mut output = tokio::task::spawn_blocking(move || {
            let mut stdout = io::stdout().lock();
            stdout.write_all(&bytes)?;
            stdout.flush()
        });
        tokio::select! {
            result = &mut output => if matches!(result, Ok(Ok(()))) { ExitCode::from(code) } else { ExitCode::FAILURE },
            _ = signals.receive() => ExitCode::FAILURE,
            _ = tokio::time::sleep(Duration::from_secs(5)) => ExitCode::FAILURE,
        }
    });
    runtime.shutdown_timeout(Duration::from_millis(100));
    code
}
fn render(mut report: Report, json: bool, started: Instant) -> Result<(Vec<u8>, u8), ()> {
    report.duration_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    let mut bytes = match encoded(&report, json) {
        Ok(bytes) => bytes,
        Err(error) => {
            let active = report.is_active();
            report = Report::failure(active, error);
            report.duration_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
            encoded(&report, json).map_err(|_| ())?
        }
    };
    bytes.push(b'\n');
    Ok((bytes, report.exit_code()))
}
fn encoded(report: &Report, json: bool) -> Result<Vec<u8>, ProjectError> {
    let bytes = if json {
        serde_json::to_vec(report).map_err(|_| ProjectError::Internal)?
    } else {
        report.human().into_bytes()
    };
    if bytes.len() + 1 > MAX_REPORT {
        Err(ProjectError::Rejected(
            OperationalErrorCode::OutputLimitExceeded,
        ))
    } else {
        Ok(bytes)
    }
}
