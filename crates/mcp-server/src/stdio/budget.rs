//! Byte budgets only; JSON parsing and protocol errors remain in rmcp.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Sleep;
use tokio_util::sync::CancellationToken;

const MAX_LINE_BYTES: usize = 1024 * 1024;
const READ_CHUNK_BYTES: usize = 8192;
const FRAME_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(super) struct IoFailure {
    failed: AtomicBool,
    cancellation: CancellationToken,
    workers: CancellationToken,
}

impl IoFailure {
    pub(super) fn new(workers: CancellationToken) -> Self {
        Self {
            workers,
            ..Self::default()
        }
    }
    pub(super) fn end(&self) {
        self.workers.cancel();
    }

    pub(super) fn record(&self) {
        self.failed.store(true, Ordering::Relaxed);
        // rmcp may otherwise keep receiving after a response write error.
        self.cancellation.cancel();
        self.end();
    }

    pub(super) fn occurred(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    pub(super) fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}

pub(super) struct BudgetedReader<R> {
    inner: R,
    failed: Arc<IoFailure>,
    line_bytes: usize,
    ended: bool,
    deadline: Option<Pin<Box<Sleep>>>,
    timeout: Duration,
}

impl<R> BudgetedReader<R> {
    pub(super) fn new(inner: R, failed: Arc<IoFailure>) -> Self {
        Self {
            inner,
            failed,
            line_bytes: 0,
            ended: false,
            deadline: None,
            timeout: FRAME_DEADLINE,
        }
    }

    fn fail(&mut self) -> Poll<io::Result<()>> {
        self.ended = true;
        self.failed.record();
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "MCP input rejected",
        )))
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for BudgetedReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.ended || output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if this
            .deadline
            .as_mut()
            .is_some_and(|timer| timer.as_mut().poll(cx).is_ready())
        {
            return this.fail();
        }
        let mut scratch = [0; READ_CHUNK_BYTES];
        let capacity = output.remaining().min(scratch.len());
        let mut chunk = ReadBuf::new(&mut scratch[..capacity]);
        if ready!(Pin::new(&mut this.inner).poll_read(cx, &mut chunk)).is_err() {
            return this.fail();
        }
        if chunk.filled().is_empty() {
            if this.line_bytes != 0 {
                return this.fail();
            }
            this.ended = true;
            return Poll::Ready(Ok(()));
        }
        for byte in chunk.filled() {
            if *byte == b'\n' {
                this.line_bytes = 0;
                this.deadline = None;
            } else if this.line_bytes == MAX_LINE_BYTES {
                return this.fail();
            } else {
                if this.line_bytes == 0 {
                    this.deadline = Some(Box::pin(tokio::time::sleep(this.timeout)));
                }
                this.line_bytes += 1;
            }
        }
        output.put_slice(chunk.filled());
        Poll::Ready(Ok(()))
    }
}

pub(super) struct CheckedWriter<W> {
    inner: W,
    failed: Arc<IoFailure>,
    line_bytes: usize,
    deadline: Option<Pin<Box<Sleep>>>,
    timeout: Duration,
}

impl<W> CheckedWriter<W> {
    pub(super) fn new(inner: W, failed: Arc<IoFailure>) -> Self {
        Self {
            inner,
            failed,
            line_bytes: 0,
            deadline: None,
            timeout: FRAME_DEADLINE,
        }
    }

    fn fail<T>(&self) -> Poll<io::Result<T>> {
        self.failed.record();
        Poll::Ready(Err(io::Error::other("MCP output rejected")))
    }

    fn expired(&mut self, cx: &mut Context<'_>) -> bool {
        if self.failed.occurred() {
            return true;
        }
        self.deadline
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(self.timeout)))
            .as_mut()
            .poll(cx)
            .is_ready()
    }

    fn record<T>(&self, result: io::Result<T>) -> Poll<io::Result<T>> {
        if result.is_err() {
            self.failed.record();
        }
        Poll::Ready(result)
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for CheckedWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if this.expired(cx) {
            return this.fail();
        }
        // Check the complete offered buffer before forwarding any bytes. Actual
        // accounting below uses only accepted bytes when the inner writer is short.
        let mut line_bytes = this.line_bytes;
        for byte in bytes {
            if *byte == b'\n' {
                line_bytes = 0;
            } else if line_bytes == MAX_LINE_BYTES {
                return this.fail();
            } else {
                line_bytes += 1;
            }
        }
        let result = ready!(Pin::new(&mut this.inner).poll_write(cx, bytes));
        if let Ok(written) = result {
            if written == 0 {
                return this.fail();
            }
            for byte in &bytes[..written] {
                if *byte == b'\n' {
                    this.line_bytes = 0;
                } else {
                    this.line_bytes += 1;
                }
            }
        }
        this.record(result)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.expired(cx) {
            return this.fail();
        }
        let result = ready!(Pin::new(&mut this.inner).poll_flush(cx));
        // Pinned rmcp 3.2.0 AsyncRwTransport::send uses SinkExt::send, which
        // flushes each framed message (async_rw.rs:115). Successful flush is
        // therefore the frame-send boundary; a newline alone is insufficient.
        // A partial frame retains its original deadline even if flushed.
        if result.is_ok() && this.line_bytes == 0 {
            this.deadline = None;
        }
        this.record(result)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.expired(cx) {
            return this.fail();
        }
        let result = ready!(Pin::new(&mut this.inner).poll_shutdown(cx));
        this.record(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn run(test: impl Future<Output = io::Result<()>>) -> io::Result<()> {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()?
            .block_on(test)
    }

    #[test]
    fn idle_reader_has_no_deadline_but_partial_frame_does() -> io::Result<()> {
        run(async {
            let (mut peer, stream) = tokio::io::duplex(8);
            let failure = Arc::new(IoFailure::default());
            let mut reader = BudgetedReader::new(stream, Arc::clone(&failure));
            reader.timeout = Duration::from_millis(5);
            let mut byte = [0];
            assert!(
                tokio::time::timeout(Duration::from_millis(15), reader.read(&mut byte))
                    .await
                    .is_err()
            );
            assert!(!failure.occurred());
            peer.write_all(b"a").await?;
            assert_eq!(reader.read(&mut byte).await?, 1);
            assert!(reader.read(&mut byte).await.is_err());
            assert!(failure.cancellation().is_cancelled());
            Ok(())
        })
    }

    #[test]
    fn completed_input_frame_resets_deadline() -> io::Result<()> {
        run(async {
            let (mut peer, stream) = tokio::io::duplex(8);
            let failure = Arc::new(IoFailure::default());
            let mut reader = BudgetedReader::new(stream, Arc::clone(&failure));
            reader.timeout = Duration::from_millis(5);
            peer.write_all(b"a\n").await?;
            let mut bytes = [0; 2];
            reader.read_exact(&mut bytes).await?;
            assert!(
                tokio::time::timeout(Duration::from_millis(15), reader.read(&mut bytes))
                    .await
                    .is_err()
            );
            assert!(!failure.occurred());
            Ok(())
        })
    }

    #[test]
    fn output_cap_counts_across_writes_and_newline_resets_it() -> io::Result<()> {
        run(async {
            let failure = Arc::new(IoFailure::default());
            let mut writer = CheckedWriter::new(tokio::io::sink(), Arc::clone(&failure));
            writer.write_all(&vec![b'a'; MAX_LINE_BYTES]).await?;
            writer.write_all(b"\nb").await?;
            assert!(writer.write_all(&vec![b'a'; MAX_LINE_BYTES]).await.is_err());
            assert!(failure.occurred());
            Ok(())
        })
    }

    #[test]
    fn pending_output_is_deadlined() -> io::Result<()> {
        run(async {
            let (stream, _unread_peer) = tokio::io::duplex(1);
            let failure = Arc::new(IoFailure::default());
            let mut writer = CheckedWriter::new(stream, Arc::clone(&failure));
            writer.timeout = Duration::from_millis(5);
            assert!(writer.write_all(b"ab\n").await.is_err());
            assert!(failure.occurred());
            Ok(())
        })
    }

    #[test]
    fn stalled_partial_output_keeps_only_written_prefix() -> io::Result<()> {
        run(async {
            let (stream, mut peer) = tokio::io::duplex(2);
            let failure = Arc::new(IoFailure::default());
            let mut writer = CheckedWriter::new(stream, Arc::clone(&failure));
            writer.timeout = Duration::from_millis(5);
            assert!(writer.write_all(b"abc").await.is_err());
            assert!(failure.occurred());
            assert!(failure.cancellation().is_cancelled());
            drop(writer);
            let mut received = Vec::new();
            peer.read_to_end(&mut received).await?;
            assert_eq!(received, b"ab");
            // Timeout can leave an incomplete frame: no response is promised.
            assert!(!received.contains(&b'\n'));
            Ok(())
        })
    }

    struct PendingFlush;
    impl AsyncWrite for PendingFlush {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }

    #[test]
    fn pending_flush_and_shutdown_are_deadlined() -> io::Result<()> {
        run(async {
            for shutdown in [false, true] {
                let failure = Arc::new(IoFailure::default());
                let mut writer = CheckedWriter::new(PendingFlush, Arc::clone(&failure));
                writer.timeout = Duration::from_millis(5);
                writer.write_all(b"a\n").await?;
                let result = if shutdown {
                    writer.shutdown().await
                } else {
                    writer.flush().await
                };
                assert!(result.is_err());
                assert!(failure.occurred());
            }
            Ok(())
        })
    }

    #[test]
    fn flushed_partial_frame_does_not_restart_output_deadline() -> io::Result<()> {
        run(async {
            let failure = Arc::new(IoFailure::default());
            let mut writer = CheckedWriter::new(tokio::io::sink(), Arc::clone(&failure));
            writer.timeout = Duration::from_millis(5);
            writer.write_all(b"a").await?;
            writer.flush().await?;
            tokio::time::sleep(Duration::from_millis(10)).await;
            assert!(writer.write_all(b"b").await.is_err());
            Ok(())
        })
    }
}
