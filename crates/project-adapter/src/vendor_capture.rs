//! ADR-078: the host side of the offline vendor capture.
//!
//! Three things live here and nothing else: the SHA-256 the domain's framing is
//! fed to, the two entry points a host uses (capture a tree, verify an
//! artifact), and the replayable handle a runtime receives. Everything about
//! *what* is admitted is in `rust_engineering_domain::vendor_capture`;
//! everything about *how* the filesystem is touched is in the macOS module.
use rust_engineering_application::vendor_capture::{VendorCaptureAccess, VerifiedVendorCapture};
use rust_engineering_application::{OperationControl, ProjectError};
use rust_engineering_domain::vendor_capture::{CaptureHasher, VendorCapture, VendorCaptureError};
use rust_engineering_domain::{OperationalErrorCode, SourceFingerprint};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

/// SHA-256 behind the domain's framing-only hasher. The domain decides what is
/// hashed and in what order; this decides nothing.
#[derive(Default)]
pub struct Sha256Hasher(Sha256);

impl CaptureHasher for Sha256Hasher {
    fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    fn finish(self) -> Result<SourceFingerprint, VendorCaptureError> {
        let mut encoded = String::from("sha256:");
        for byte in self.0.finalize() {
            use std::fmt::Write;
            write!(&mut encoded, "{byte:02x}").map_err(|_| VendorCaptureError::Invalid)?;
        }
        encoded.parse().map_err(|_| VendorCaptureError::Invalid)
    }
}

/// One mapping, so every site reports the same refusal for the same cause.
pub fn capture_error(error: VendorCaptureError) -> ProjectError {
    ProjectError::Rejected(match error {
        VendorCaptureError::Limits => OperationalErrorCode::OutputLimitExceeded,
        // A link, a moved tree and a wrong digest are all refusals of host
        // data, not limits: they say the capture is not what it claimed to be.
        VendorCaptureError::Invalid
        | VendorCaptureError::Link
        | VendorCaptureError::Mutated
        | VendorCaptureError::Digest => OperationalErrorCode::InvalidProject,
    })
}

/// Capture `directory` into `store` and return the identity of the artifact.
///
/// ADR-078 §3: this is provisioning. It is reached from the explicit CLI and
/// never as a side effect of a measurement.
pub fn capture_vendor_tree(
    directory: &Path,
    store: &Path,
    control: &dyn OperationControl,
) -> Result<VendorCapture, ProjectError> {
    #[cfg(target_os = "macos")]
    {
        crate::filesystem::capture_vendor_tree(directory, store, control)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (directory, store, control);
        Err(ProjectError::Rejected(
            OperationalErrorCode::UnsupportedPlatform,
        ))
    }
}

/// An artifact whose digest was checked against the declared one, held open at
/// the descriptor the verification read.
///
/// ADR-078 §2 is why this is a type and not a `(VendorCapture, PathBuf)`: if a
/// consumer reopened the path it would be using an artifact nobody verified.
pub struct NativeVendorCapture {
    capture: VendorCapture,
    artifact: Mutex<CaptureReplay>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CaptureFileStamp {
    len: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    mode: u32,
    #[cfg(unix)]
    links: u64,
    #[cfg(unix)]
    uid: u32,
    #[cfg(unix)]
    gid: u32,
    #[cfg(unix)]
    modified_seconds: i64,
    #[cfg(unix)]
    modified_nanoseconds: i64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
    #[cfg(not(unix))]
    modified: Option<std::time::SystemTime>,
}

impl CaptureFileStamp {
    fn of(file: &std::fs::File) -> std::io::Result<Self> {
        let metadata = file.metadata()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                len: metadata.len(),
                device: metadata.dev(),
                inode: metadata.ino(),
                mode: metadata.mode(),
                links: metadata.nlink(),
                uid: metadata.uid(),
                gid: metadata.gid(),
                modified_seconds: metadata.mtime(),
                modified_nanoseconds: metadata.mtime_nsec(),
                changed_seconds: metadata.ctime(),
                changed_nanoseconds: metadata.ctime_nsec(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                len: metadata.len(),
                modified: metadata.modified().ok(),
            })
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplayStatus {
    Streaming,
    Complete,
    Mutated,
}

struct CaptureReplay {
    file: std::fs::File,
    stamp: CaptureFileStamp,
    hasher: Sha256Hasher,
    bytes_read: u64,
    status: ReplayStatus,
}

impl CaptureReplay {
    #[cfg(target_os = "macos")]
    fn new(file: std::fs::File, expected_bytes: u64) -> Result<Self, ProjectError> {
        let stamp = CaptureFileStamp::of(&file).map_err(|_| ProjectError::Internal)?;
        if stamp.len != expected_bytes {
            return Err(ProjectError::Rejected(OperationalErrorCode::InvalidProject));
        }
        Ok(Self {
            file,
            stamp,
            hasher: Sha256Hasher::default(),
            bytes_read: 0,
            status: ReplayStatus::Streaming,
        })
    }

    fn refuse_mutated<T>(&mut self) -> Result<T, VendorCaptureAccess> {
        self.status = ReplayStatus::Mutated;
        Err(VendorCaptureAccess::Mutated)
    }

    fn check_stamp(&mut self, expected_bytes: u64) -> Result<(), VendorCaptureAccess> {
        let current = CaptureFileStamp::of(&self.file).map_err(|_| VendorCaptureAccess::Io)?;
        if current != self.stamp || current.len != expected_bytes {
            return self.refuse_mutated();
        }
        Ok(())
    }

    fn rewind(&mut self, expected_bytes: u64) -> Result<(), VendorCaptureAccess> {
        use std::io::Seek;
        if self.status == ReplayStatus::Mutated {
            return Err(VendorCaptureAccess::Mutated);
        }
        self.check_stamp(expected_bytes)?;
        self.file
            .seek(std::io::SeekFrom::Start(0))
            .map_err(|_| VendorCaptureAccess::Io)?;
        self.check_stamp(expected_bytes)?;
        self.hasher = Sha256Hasher::default();
        self.bytes_read = 0;
        self.status = ReplayStatus::Streaming;
        Ok(())
    }

    fn read(
        &mut self,
        buffer: &mut [u8],
        expected_bytes: u64,
        expected_digest: &SourceFingerprint,
    ) -> Result<usize, VendorCaptureAccess> {
        if self.status == ReplayStatus::Mutated {
            return Err(VendorCaptureAccess::Mutated);
        }
        if self.status == ReplayStatus::Complete {
            return Ok(0);
        }
        if buffer.is_empty() {
            return Err(VendorCaptureAccess::Io);
        }

        self.check_stamp(expected_bytes)?;
        let read_len = self
            .file
            .read(buffer)
            .map_err(|_| VendorCaptureAccess::Io)?;
        let read = u64::try_from(read_len).map_err(|_| VendorCaptureAccess::Io)?;
        let Some(total) = self.bytes_read.checked_add(read) else {
            return self.refuse_mutated();
        };
        if read == 0 || total > expected_bytes {
            return self.refuse_mutated();
        }

        self.hasher.update(&buffer[..read_len]);
        self.bytes_read = total;
        if total < expected_bytes {
            self.check_stamp(expected_bytes)?;
            return Ok(read_len);
        }

        // The supervisor stops asking its InputSource for bytes as soon as it
        // has received the declared length. Authenticate that final chunk
        // before returning it; checking only a later EOF would never protect
        // the ingest path.
        let mut extra = [0_u8; 1];
        let extra_read = self
            .file
            .read(&mut extra)
            .map_err(|_| VendorCaptureAccess::Io)?;
        self.check_stamp(expected_bytes)?;
        let actual_digest = match std::mem::take(&mut self.hasher).finish() {
            Ok(digest) => digest,
            Err(_) => return self.refuse_mutated(),
        };
        if extra_read != 0 || &actual_digest != expected_digest {
            return self.refuse_mutated();
        }
        self.status = ReplayStatus::Complete;
        Ok(read_len)
    }
}

impl NativeVendorCapture {
    /// The verified identity. Inherent as well as on the trait, so a host tool
    /// can read it without importing the runtime port.
    pub fn capture(&self) -> &VendorCapture {
        &self.capture
    }
}

impl VerifiedVendorCapture for NativeVendorCapture {
    fn capture(&self) -> &VendorCapture {
        &self.capture
    }

    fn rewind(&self) -> Result<(), VendorCaptureAccess> {
        let mut artifact = self.artifact.lock().map_err(|_| VendorCaptureAccess::Io)?;
        artifact.rewind(self.capture.artifact_bytes())
    }

    fn read(&self, buffer: &mut [u8]) -> Result<usize, VendorCaptureAccess> {
        let mut artifact = self.artifact.lock().map_err(|_| VendorCaptureAccess::Io)?;
        artifact.read(
            buffer,
            self.capture.artifact_bytes(),
            self.capture.artifact_digest(),
        )
    }
}

/// Open the artifact `declared` names, re-derive its identity incrementally and
/// refuse it if the digest is not the declared one.
pub fn open_verified_capture(
    artifact: &Path,
    declared: &SourceFingerprint,
    control: &dyn OperationControl,
) -> Result<NativeVendorCapture, ProjectError> {
    #[cfg(target_os = "macos")]
    {
        let (capture, file) =
            crate::filesystem::verify_vendor_capture(artifact, declared, control)?;
        let artifact = CaptureReplay::new(file, capture.artifact_bytes())?;
        Ok(NativeVendorCapture {
            capture,
            artifact: Mutex::new(artifact),
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (artifact, declared, control);
        Err(ProjectError::Rejected(
            OperationalErrorCode::UnsupportedPlatform,
        ))
    }
}

/// The artifact file name a capture is published under: the 64 hex digits of
/// its tree digest, so an existing capture is found by digest (ADR-078 §2).
pub fn capture_artifact_name(digest: &SourceFingerprint) -> Option<String> {
    digest
        .as_str()
        .strip_prefix("sha256:")
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_owned)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::io::{Seek as _, Write as _};
    use std::path::PathBuf;

    struct Never;
    impl OperationControl for Never {
        fn check(&self) -> Result<(), ProjectError> {
            Ok(())
        }
    }

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(label: &str) -> Result<(NativeVendorCapture, PathBuf, Cleanup), String> {
        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).map_err(|error| error.to_string())?;
        let root = PathBuf::from("/private/tmp").join(format!(
            "rms-replay-{label}-{}-{}",
            std::process::id(),
            u64::from_le_bytes(nonce)
        ));
        std::fs::create_dir(&root).map_err(|error| error.to_string())?;
        let cleanup = Cleanup(root.clone());
        let vendor = root.join("vendor");
        let store = root.join("store");
        let package = vendor.join("replay-fixture-1.0.0");
        std::fs::create_dir_all(package.join("src")).map_err(|error| error.to_string())?;
        std::fs::create_dir(&store).map_err(|error| error.to_string())?;
        std::fs::write(package.join("Cargo.toml"), b"[package]\n")
            .map_err(|error| error.to_string())?;
        std::fs::write(package.join("src/lib.rs"), vec![7_u8; 5_000])
            .map_err(|error| error.to_string())?;

        let captured =
            capture_vendor_tree(&vendor, &store, &Never).map_err(|error| format!("{error:?}"))?;
        let name = capture_artifact_name(captured.tree_digest())
            .ok_or_else(|| "capture artifact name".to_owned())?;
        let path = store.join(name);
        let verified = open_verified_capture(&path, captured.tree_digest(), &Never)
            .map_err(|error| format!("{error:?}"))?;
        Ok((verified, path, cleanup))
    }

    #[test]
    fn replay_authenticates_the_final_chunk_before_returning_declared_length() -> Result<(), String>
    {
        let (verified, _path, _cleanup) = fixture("complete")?;
        verified.rewind().map_err(|error| error.to_string())?;
        let expected = verified.capture().artifact_bytes();
        let mut received = 0_u64;
        let mut buffer = [0_u8; 777];
        while received < expected {
            let read = verified
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            assert_ne!(read, 0);
            received += u64::try_from(read).map_err(|error| error.to_string())?;
        }
        assert_eq!(received, expected);
        Ok(())
    }

    #[test]
    fn same_length_rewrite_is_refused_before_the_final_chunk_is_returned() -> Result<(), String> {
        let (verified, path, _cleanup) = fixture("rewrite")?;
        let expected = verified.capture().artifact_bytes();

        let mut writer = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        let mut byte = [0_u8; 1];
        writer
            .read_exact(&mut byte)
            .map_err(|error| error.to_string())?;
        writer
            .seek(std::io::SeekFrom::Start(0))
            .map_err(|error| error.to_string())?;
        byte[0] ^= 0xff;
        writer.write_all(&byte).map_err(|error| error.to_string())?;
        writer.sync_all().map_err(|error| error.to_string())?;

        // Model the narrow race in which the file changes after filesystem
        // verification but before this module records its replay stamp. The
        // stamp alone then agrees; only the digest can reject the replay.
        {
            let mut replay = verified.artifact.lock().map_err(|_| "lock".to_owned())?;
            replay.stamp = CaptureFileStamp::of(&replay.file).map_err(|error| error.to_string())?;
        }
        verified.rewind().map_err(|error| error.to_string())?;

        let mut accepted = 0_u64;
        let mut buffer = [0_u8; 777];
        loop {
            match verified.read(&mut buffer) {
                Ok(read) => {
                    assert_ne!(read, 0, "the supervisor does not require an EOF read");
                    accepted += u64::try_from(read).map_err(|error| error.to_string())?;
                }
                Err(VendorCaptureAccess::Mutated) => break,
                Err(error) => return Err(format!("unexpected replay error: {error}")),
            }
        }
        assert!(
            accepted < expected,
            "the unauthenticated final chunk must be withheld"
        );
        assert_eq!(
            verified.read(&mut buffer),
            Err(VendorCaptureAccess::Mutated),
            "a rejected replay remains rejected"
        );
        Ok(())
    }
}
