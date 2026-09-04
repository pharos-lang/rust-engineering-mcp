//! Bounded host snapshot acquisition using the existing APFS capability boundary.
use super::{FileStamp, SecureProjects, checked_path, invalid, map_io, rejected};
use crate::MAX_HOST_SNAPSHOT_BYTES;
use rust_engineering_application::{OperationControl, ProjectError};
use rust_engineering_domain::OperationalErrorCode;
use rustix::fs::fstat;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Reads an explicitly trusted host configuration path, never an MCP argument.
///
/// The path must be absolute and physical on macOS 26+ / APFS. This acquires only
/// the parent directory's authority and does not require a Cargo manifest. All
/// opens use the existing no-follow/beneath primitives and retained root handles.
/// Regular, single-link files are checked for size and stable metadata before and
/// after acquisition, including a reopen through the original root authority.
/// As with project capture, stamps detect observed changes, not an atomic snapshot
/// against a privileged writer or rename-ABA. Callers verify the owned bytes' hash.
pub fn read_host_snapshot(
    path: &Path,
    control: &dyn OperationControl,
) -> Result<Vec<u8>, ProjectError> {
    control.check()?;
    let path = checked_path(path)?;
    let parent = path.parent().ok_or_else(invalid)?.to_path_buf();
    let projects = SecureProjects::new(&[parent])?;
    let fd = projects.open_path(&path, false)?;
    let before = FileStamp::from_stat(fstat(&fd).map_err(map_io)?)?;
    if before.size < 0 || before.size as u64 > MAX_HOST_SNAPSHOT_BYTES as u64 {
        return Err(rejected(OperationalErrorCode::OutputLimitExceeded));
    }
    let mut file = File::from(fd);
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        control.check()?;
        // At most one byte beyond the limit is consumed, even if the file grows.
        let allowed = chunk.len().min(MAX_HOST_SNAPSHOT_BYTES - bytes.len() + 1);
        let read = file.read(&mut chunk[..allowed]).map_err(|_| invalid())?;
        if read == 0 {
            break;
        }
        if read > MAX_HOST_SNAPSHOT_BYTES - bytes.len() {
            return Err(rejected(OperationalErrorCode::OutputLimitExceeded));
        }
        bytes
            .try_reserve_exact(read)
            .map_err(|_| rejected(OperationalErrorCode::OutputLimitExceeded))?;
        bytes.extend_from_slice(&chunk[..read]);
    }
    control.check()?;
    if bytes.len() as u64 != before.size as u64
        || FileStamp::from_stat(fstat(&file).map_err(map_io)?)? != before
    {
        return Err(invalid());
    }
    let reopened = projects.open_path(&path, false)?;
    if FileStamp::from_stat(fstat(&reopened).map_err(map_io)?)? != before
        || FileStamp::from_stat(fstat(&file).map_err(map_io)?)? != before
    {
        return Err(invalid());
    }
    Ok(bytes)
}
