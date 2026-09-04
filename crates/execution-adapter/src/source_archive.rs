//! Bounded generated USTAR only; never accepts arbitrary archive bytes.
use rust_engineering_application::ExecutionError;
use rust_engineering_domain::SourceBundle;
const MAX_ARCHIVE: usize = 24 * 1024 * 1024;
fn octal(field: &mut [u8], value: usize) -> Result<(), ExecutionError> {
    let text = format!("{value:0width$o}", width = field.len() - 1);
    if text.len() != field.len() - 1 {
        return Err(ExecutionError::Denied);
    }
    field[..text.len()].copy_from_slice(text.as_bytes());
    Ok(())
}
fn entry(
    out: &mut Vec<u8>,
    name: &str,
    bytes: &[u8],
    directory: bool,
) -> Result<(), ExecutionError> {
    let padded = bytes.len().div_ceil(512) * 512;
    if out.len() + 512 + padded + 1024 > MAX_ARCHIVE {
        return Err(ExecutionError::Denied);
    }
    let mut header = [0u8; 512];
    if name.len() > 100 {
        return Err(ExecutionError::Denied);
    }
    header[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut header[100..108], if directory { 0o755 } else { 0o444 })?;
    octal(&mut header[108..116], 0)?;
    octal(&mut header[116..124], 0)?;
    octal(&mut header[124..136], bytes.len())?;
    octal(&mut header[136..148], 0)?;
    header[148..156].fill(b' ');
    header[156] = if directory { b'5' } else { b'0' };
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|v| usize::from(*v)).sum();
    octal(&mut header[148..155], checksum)?;
    header[155] = b' ';
    out.extend_from_slice(&header);
    out.extend_from_slice(bytes);
    out.resize(out.len() + padded - bytes.len(), 0);
    Ok(())
}
pub(super) fn encode(source: &SourceBundle) -> Result<Vec<u8>, ExecutionError> {
    let mut out = Vec::new();
    // Domain sorting puts every parent before its descendants, including empties.
    for directory in source.directories() {
        entry(&mut out, directory, &[], true)?;
    }
    for file in source.files() {
        entry(&mut out, file.path(), file.bytes(), false)?;
    }
    out.resize(out.len() + 1024, 0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::SourceFile;
    #[test]
    fn archive_has_only_plain_ustar_entries_and_preserves_binary_bytes() -> Result<(), String> {
        let file = SourceFile::new("d/f".into(), vec![0, 255, 10]).map_err(|e| format!("{e:?}"))?;
        let source = SourceBundle::with_directories(vec![file], vec!["empty".into()])
            .map_err(|e| format!("{e:?}"))?;
        let bytes = encode(&source).map_err(|e| format!("{e:?}"))?;
        assert_eq!(bytes.len(), 3072);
        assert_eq!(&bytes[..2], b"d\0");
        assert_eq!(bytes[156], b'5');
        assert_eq!(&bytes[512..518], b"empty\0");
        assert_eq!(bytes[512 + 156], b'5');
        assert_eq!(&bytes[1024..1028], b"d/f\0");
        assert_eq!(bytes[1024 + 156], b'0');
        assert_eq!(&bytes[1536..1539], &[0, 255, 10]);
        assert!(bytes[1539..].iter().all(|b| *b == 0));
        for offset in [0, 512, 1024] {
            assert_eq!(&bytes[offset + 257..offset + 265], b"ustar\x0000");
            assert!(bytes[offset + 157..offset + 257].iter().all(|b| *b == 0));
            let stored = std::str::from_utf8(&bytes[offset + 148..offset + 154])
                .map_err(|e| e.to_string())?;
            let expected = usize::from_str_radix(stored, 8).map_err(|e| e.to_string())?;
            let actual: usize = bytes[offset..offset + 512]
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    if (148..156).contains(&i) {
                        32
                    } else {
                        usize::from(*b)
                    }
                })
                .sum();
            assert_eq!(expected, actual);
        }
        Ok(())
    }
}
