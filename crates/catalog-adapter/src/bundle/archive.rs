//! Deliberately narrow USTAR reader: regular files only; never extract to paths.
use super::{BundleError, MAX_FILES};

pub(super) fn entries(bytes: &[u8]) -> Result<Vec<(String, &[u8])>, BundleError> {
    if bytes.len() < 1024 || !bytes.len().is_multiple_of(512) {
        return Err(BundleError::InvalidArchive);
    }
    let mut offset = 0usize;
    let mut files = Vec::new();
    while offset + 1024 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            if !bytes[offset..].iter().all(|b| *b == 0) {
                return Err(BundleError::InvalidArchive);
            }
            return Ok(files);
        }
        if files.len() == MAX_FILES {
            return Err(BundleError::Budget);
        }
        if &header[257..263] != b"ustar\0"
            || &header[263..265] != b"00"
            || !matches!(header[156], 0 | b'0')
            || header[157..257].iter().any(|b| *b != 0)
            || header[345..512].iter().any(|b| *b != 0)
            || octal(&header[329..337])? != 0
            || octal(&header[337..345])? != 0
        {
            return Err(BundleError::InvalidArchive);
        }
        let checksum: u64 = header
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u64::from(*b)
                }
            })
            .sum();
        if checksum != octal(&header[148..156])? {
            return Err(BundleError::InvalidArchive);
        }
        // Numeric metadata must be bounded ASCII octal, never base256 or extensions.
        for range in [100..108, 108..116, 116..124, 136..148] {
            octal(&header[range])?;
        }
        let path = terminated(&header[..100])?;
        if path.is_empty()
            || path.len() > 99
            || path.starts_with('/')
            || path.contains('\\')
            || !path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
            || path
                .split('/')
                .any(|s| s.is_empty() || matches!(s, "." | ".."))
            || files.iter().any(|(p, _)| p == path)
        {
            return Err(BundleError::InvalidArchive);
        }
        let size = usize::try_from(octal(&header[124..136])?).map_err(|_| BundleError::Budget)?;
        let start = offset + 512;
        let end = start.checked_add(size).ok_or(BundleError::Budget)?;
        let padded = end
            .checked_add((512 - size % 512) % 512)
            .ok_or(BundleError::Budget)?;
        if padded > bytes.len() || end > bytes.len() || bytes[end..padded].iter().any(|b| *b != 0) {
            return Err(BundleError::InvalidArchive);
        }
        files.push((path.to_owned(), &bytes[start..end]));
        offset = padded;
    }
    Err(BundleError::InvalidArchive)
}
fn terminated(bytes: &[u8]) -> Result<&str, BundleError> {
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .ok_or(BundleError::InvalidArchive)?;
    if bytes[end..].iter().any(|b| *b != 0) {
        return Err(BundleError::InvalidArchive);
    }
    std::str::from_utf8(&bytes[..end]).map_err(|_| BundleError::InvalidArchive)
}
fn octal(bytes: &[u8]) -> Result<u64, BundleError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| BundleError::InvalidArchive)?
        .trim_matches(['\0', ' ']);
    if text.is_empty() {
        return Ok(0);
    }
    if !text.bytes().all(|b| matches!(b, b'0'..=b'7')) {
        return Err(BundleError::InvalidArchive);
    }
    u64::from_str_radix(text, 8).map_err(|_| BundleError::InvalidArchive)
}
