//! Closed USTAR profile for writable guest staging and hostile candidate export.
use rust_engineering_application::ExecutionError;
use rust_engineering_domain::{
    SOURCE_MAX_ENTRIES, SOURCE_MAX_FILE_BYTES, SOURCE_MAX_TOTAL_BYTES, SourceBundle, SourceFile,
    validate_source_path,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const MAX_ARCHIVE: usize = 24 * 1024 * 1024;
const BLOCK: usize = 512;
const END_BLOCKS: usize = 2;
const OWNER: usize = 65534;
const DIRECTORY_MODE: usize = 0o700;
const FILE_MODE: usize = 0o600;

fn denied<T>() -> Result<T, ExecutionError> {
    Err(ExecutionError::Denied)
}

fn write_octal(field: &mut [u8], value: usize) -> Result<(), ExecutionError> {
    let text = format!("{value:0width$o}", width = field.len() - 1);
    if text.len() != field.len() - 1 {
        return denied();
    }
    field[..text.len()].copy_from_slice(text.as_bytes());
    Ok(())
}

fn append_entry(
    output: &mut Vec<u8>,
    name: &str,
    bytes: &[u8],
    directory: bool,
) -> Result<(), ExecutionError> {
    let padded = bytes.len().div_ceil(BLOCK) * BLOCK;
    let required = output
        .len()
        .checked_add(BLOCK)
        .and_then(|size| size.checked_add(padded))
        .and_then(|size| size.checked_add(END_BLOCKS * BLOCK))
        .ok_or(ExecutionError::Denied)?;
    if required > MAX_ARCHIVE || name.len() > 100 {
        return denied();
    }
    let mut header = [0u8; BLOCK];
    header[..name.len()].copy_from_slice(name.as_bytes());
    write_octal(
        &mut header[100..108],
        if directory { DIRECTORY_MODE } else { FILE_MODE },
    )?;
    write_octal(&mut header[108..116], OWNER)?;
    write_octal(&mut header[116..124], OWNER)?;
    write_octal(&mut header[124..136], bytes.len())?;
    write_octal(&mut header[136..148], 0)?;
    header[148..156].fill(b' ');
    header[156] = if directory { b'5' } else { b'0' };
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    write_octal(&mut header[329..337], 0)?;
    write_octal(&mut header[337..345], 0)?;
    let checksum = header.iter().map(|byte| usize::from(*byte)).sum();
    write_octal(&mut header[148..155], checksum)?;
    header[154] = 0;
    header[155] = b' ';
    output.extend_from_slice(&header);
    output.extend_from_slice(bytes);
    output.resize(output.len() + padded - bytes.len(), 0);
    Ok(())
}

pub(super) fn encode(source: &SourceBundle) -> Result<Vec<u8>, ExecutionError> {
    let mut output = Vec::new();
    for directory in source.directories() {
        append_entry(&mut output, directory, &[], true)?;
    }
    for file in source.files() {
        append_entry(&mut output, file.path(), file.bytes(), false)?;
    }
    output.resize(output.len() + END_BLOCKS * BLOCK, 0);
    Ok(output)
}

fn strict_string(field: &[u8]) -> Result<&[u8], ExecutionError> {
    if let Some(end) = field.iter().position(|byte| *byte == 0) {
        if field[end..].iter().any(|byte| *byte != 0) {
            return denied();
        }
        Ok(&field[..end])
    } else {
        // POSIX permits a field that occupies every byte without a NUL.
        Ok(field)
    }
}

fn strict_octal(field: &[u8]) -> Result<usize, ExecutionError> {
    let (&terminator, digits) = field.split_last().ok_or(ExecutionError::Denied)?;
    if terminator != 0
        || digits.is_empty()
        || !digits.iter().all(|byte| (b'0'..=b'7').contains(byte))
    {
        return denied();
    }
    let mut value = 0usize;
    for digit in digits {
        value = value
            .checked_mul(8)
            .and_then(|value| value.checked_add(usize::from(*digit - b'0')))
            .ok_or(ExecutionError::Denied)?;
    }
    Ok(value)
}

fn checksum(header: &[u8]) -> usize {
    header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                usize::from(b' ')
            } else {
                usize::from(*byte)
            }
        })
        .sum()
}

fn header_checksum(header: &[u8]) -> Result<usize, ExecutionError> {
    if header[154] != 0 || header[155] != b' ' {
        return denied();
    }
    strict_octal(&header[148..155])
}

fn exported_path(header: &[u8], directory: bool) -> Result<Option<String>, ExecutionError> {
    let name = strict_string(&header[..100])?;
    let prefix = strict_string(&header[345..500])?;
    if name.is_empty() {
        return denied();
    }
    let mut raw = Vec::with_capacity(prefix.len() + usize::from(!prefix.is_empty()) + name.len());
    if !prefix.is_empty() {
        raw.extend_from_slice(prefix);
        raw.push(b'/');
    }
    raw.extend_from_slice(name);
    let raw = std::str::from_utf8(&raw).map_err(|_| ExecutionError::Denied)?;
    if raw == "./" {
        return if directory { Ok(None) } else { denied() };
    }
    let relative = raw.strip_prefix("./").ok_or(ExecutionError::Denied)?;
    let relative = if directory {
        relative.strip_suffix('/').ok_or(ExecutionError::Denied)?
    } else {
        if relative.ends_with('/') {
            return denied();
        }
        relative
    };
    validate_source_path(relative).map_err(|_| ExecutionError::Denied)?;
    Ok(Some(relative.to_owned()))
}

fn benign_identity_name(field: &[u8]) -> Result<(), ExecutionError> {
    let value = strict_string(field)?;
    if value
        .iter()
        .any(|byte| !byte.is_ascii_alphanumeric() && !b"_.-".contains(byte))
    {
        return denied();
    }
    Ok(())
}

pub(super) fn decode(
    archive: &[u8],
    before: &SourceBundle,
) -> Result<SourceBundle, ExecutionError> {
    decode_inner(archive, before, false)
}

pub(super) fn decode_resolution(
    archive: &[u8],
    before: &SourceBundle,
    transient_lock: bool,
) -> Result<SourceBundle, ExecutionError> {
    decode_inner(archive, before, transient_lock)
}

fn decode_inner(
    archive: &[u8],
    before: &SourceBundle,
    transient_lock: bool,
) -> Result<SourceBundle, ExecutionError> {
    if archive.len() > MAX_ARCHIVE || !archive.len().is_multiple_of(BLOCK) {
        return denied();
    }
    let expected_files = before
        .files()
        .iter()
        .map(|file| file.path())
        .collect::<BTreeSet<_>>();
    let expected_directories = before
        .directories()
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    let mut directories = BTreeSet::<String>::new();
    let mut root_seen = false;
    let mut offset = 0usize;
    let mut entries = 0usize;
    let mut total = 0usize;
    let mut ended = false;

    while offset < archive.len() {
        let end = offset.checked_add(BLOCK).ok_or(ExecutionError::Denied)?;
        let header = archive.get(offset..end).ok_or(ExecutionError::Denied)?;
        if header.iter().all(|byte| *byte == 0) {
            let second_end = end.checked_add(BLOCK).ok_or(ExecutionError::Denied)?;
            let second = archive.get(end..second_end).ok_or(ExecutionError::Denied)?;
            if second.iter().any(|byte| *byte != 0)
                || archive[second_end..].iter().any(|byte| *byte != 0)
            {
                return denied();
            }
            ended = true;
            break;
        }
        entries = entries.checked_add(1).ok_or(ExecutionError::Denied)?;
        if entries > SOURCE_MAX_ENTRIES + 1
            || header_checksum(header)? != checksum(header)
            || &header[257..265] != b"ustar\x0000"
            || header[157..257].iter().any(|byte| *byte != 0)
            || header[500..].iter().any(|byte| *byte != 0)
            || strict_octal(&header[108..116])? != OWNER
            || strict_octal(&header[116..124])? != OWNER
            || strict_octal(&header[329..337])? != 0
            || strict_octal(&header[337..345])? != 0
        {
            return denied();
        }
        benign_identity_name(&header[265..297])?;
        benign_identity_name(&header[297..329])?;
        let _mtime = strict_octal(&header[136..148])?;
        let size = strict_octal(&header[124..136])?;
        let directory = match header[156] {
            b'0' => false,
            b'5' => true,
            _ => return denied(),
        };
        if directory && size != 0 {
            return denied();
        }
        let path = exported_path(header, directory)?;
        let mode = strict_octal(&header[100..108])?;
        let mode_ok = mode == if directory { DIRECTORY_MODE } else { FILE_MODE }
            || (!directory
                && transient_lock
                && path.as_deref() == Some("Cargo.lock")
                && mode == 0o644);
        if !mode_ok {
            return denied();
        }
        let data_start = end;
        let data_end = data_start.checked_add(size).ok_or(ExecutionError::Denied)?;
        let padded_end = data_start
            .checked_add(size.div_ceil(BLOCK) * BLOCK)
            .ok_or(ExecutionError::Denied)?;
        let data = archive
            .get(data_start..data_end)
            .ok_or(ExecutionError::Denied)?;
        let padding = archive
            .get(data_end..padded_end)
            .ok_or(ExecutionError::Denied)?;
        if padding.iter().any(|byte| *byte != 0) {
            return denied();
        }
        match path {
            None => {
                if root_seen {
                    return denied();
                }
                root_seen = true;
            }
            Some(path) if directory => {
                if !expected_directories.contains(path.as_str())
                    || expected_files.contains(path.as_str())
                    || !directories.insert(path)
                {
                    return denied();
                }
            }
            Some(path) => {
                total = total.checked_add(size).ok_or(ExecutionError::Denied)?;
                if size > SOURCE_MAX_FILE_BYTES
                    || total > SOURCE_MAX_TOTAL_BYTES
                    || !expected_files.contains(path.as_str())
                    || expected_directories.contains(path.as_str())
                    || files.insert(path, data.to_vec()).is_some()
                {
                    return denied();
                }
            }
        }
        offset = padded_end;
    }
    if !ended
        || !root_seen
        || files.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected_files
        || directories
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != expected_directories
    {
        return denied();
    }
    let files = files
        .into_iter()
        .map(|(path, bytes)| SourceFile::new(path, bytes).map_err(|_| ExecutionError::Denied))
        .collect::<Result<Vec<_>, _>>()?;
    SourceBundle::with_directories(files, directories.into_iter().collect())
        .map_err(|_| ExecutionError::Denied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(files: &[(&str, &[u8])], directories: &[&str]) -> Result<SourceBundle, String> {
        let files = files
            .iter()
            .map(|(path, bytes)| SourceFile::new((*path).into(), bytes.to_vec()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("{error:?}"))?;
        SourceBundle::with_directories(
            files,
            directories.iter().map(|path| (*path).into()).collect(),
        )
        .map_err(|error| format!("{error:?}"))
    }

    fn exported(before: &SourceBundle, replacements: &BTreeMap<&str, Vec<u8>>) -> Vec<u8> {
        let mut output = Vec::new();
        test_entry(
            &mut output,
            "./",
            &[],
            true,
            DIRECTORY_MODE,
            OWNER,
            OWNER,
            b'5',
        );
        for directory in before.directories() {
            test_entry(
                &mut output,
                &format!("./{directory}/"),
                &[],
                true,
                DIRECTORY_MODE,
                OWNER,
                OWNER,
                b'5',
            );
        }
        for file in before.files() {
            let bytes = replacements
                .get(file.path())
                .map_or_else(|| file.bytes(), Vec::as_slice);
            test_entry(
                &mut output,
                &format!("./{}", file.path()),
                bytes,
                false,
                FILE_MODE,
                OWNER,
                OWNER,
                b'0',
            );
        }
        output.resize(output.len() + END_BLOCKS * BLOCK, 0);
        output
    }

    #[allow(clippy::too_many_arguments)]
    fn test_entry(
        output: &mut Vec<u8>,
        path: &str,
        bytes: &[u8],
        directory: bool,
        mode: usize,
        uid: usize,
        gid: usize,
        kind: u8,
    ) {
        let mut header = [0u8; BLOCK];
        let (prefix, name) = if path.len() <= 100 {
            ("", path)
        } else {
            (".", path.strip_prefix("./").unwrap_or(path))
        };
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        test_octal(&mut header[100..108], mode);
        test_octal(&mut header[108..116], uid);
        test_octal(&mut header[116..124], gid);
        test_octal(&mut header[124..136], bytes.len());
        test_octal(&mut header[136..148], 1);
        header[148..156].fill(b' ');
        header[156] = kind;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[265..272].copy_from_slice(b"nobody\0");
        header[297..305].copy_from_slice(b"nogroup\0");
        test_octal(&mut header[329..337], 0);
        test_octal(&mut header[337..345], 0);
        set_checksum(&mut header);
        output.extend_from_slice(&header);
        output.extend_from_slice(bytes);
        output.resize(
            output.len() + bytes.len().div_ceil(BLOCK) * BLOCK - bytes.len(),
            0,
        );
        let _ = directory;
    }

    fn test_octal(field: &mut [u8], value: usize) {
        assert!(write_octal(field, value).is_ok());
    }

    fn set_checksum(header: &mut [u8]) {
        header[148..156].fill(b' ');
        let value = checksum(header);
        test_octal(&mut header[148..155], value);
        header[154] = 0;
        header[155] = b' ';
    }

    fn first_header(archive: &mut [u8], index: usize) -> &mut [u8] {
        let mut offset = 0;
        for _ in 0..index {
            let size = strict_octal(&archive[offset + 124..offset + 136]).unwrap_or(0);
            offset += BLOCK + size.div_ceil(BLOCK) * BLOCK;
        }
        &mut archive[offset..offset + BLOCK]
    }

    #[test]
    fn writable_encoder_uses_closed_modes_and_identity() -> Result<(), String> {
        let before = source(&[("src/lib.rs", b"x")], &["empty"])?;
        let archive = encode(&before).map_err(|error| format!("{error:?}"))?;
        assert!(archive.len() <= MAX_ARCHIVE);
        for (index, expected_mode) in [DIRECTORY_MODE, DIRECTORY_MODE, FILE_MODE]
            .into_iter()
            .enumerate()
        {
            let mut copy = archive.clone();
            let header = first_header(&mut copy, index);
            assert_eq!(
                strict_octal(&header[100..108]).map_err(|error| format!("{error:?}"))?,
                expected_mode
            );
            assert_eq!(
                strict_octal(&header[108..116]).map_err(|error| format!("{error:?}"))?,
                OWNER
            );
            assert_eq!(
                strict_octal(&header[116..124]).map_err(|error| format!("{error:?}"))?,
                OWNER
            );
        }
        Ok(())
    }

    #[test]
    fn decodes_changed_binary_bytes_and_strict_prefix_path() -> Result<(), String> {
        let long = format!("d/{}", "x".repeat(98));
        let before = source(&[(long.as_str(), b"old"), ("raw", b"old")], &[])?;
        let replacements =
            BTreeMap::from([(long.as_str(), vec![0, 255, 10]), ("raw", b"new".to_vec())]);
        let candidate = decode(&exported(&before, &replacements), &before)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(candidate.files()[0].bytes(), &[0, 255, 10]);
        assert_eq!(candidate.files()[1].bytes(), b"new");
        Ok(())
    }

    #[test]
    fn rejects_qualified_receipt_export_with_extra_wrong_mode_file() -> Result<(), String> {
        let receipt: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/validation/M2-D04-native-qualification.json"
        ))
        .map_err(|error| error.to_string())?;
        let text = receipt["observations"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item["case"] == "guardian_preserved_complete_export")
            })
            .and_then(|item| item["export"]["base64"].as_str())
            .ok_or("missing export")?;
        let archive = decode_base64(text)?;
        let before = source(
            &[
                ("sentinel.bin", b"guardian-kept-bytes\x00\xff"),
                ("src/main.rs", b"fn main() {}\n"),
            ],
            &[],
        )?;
        assert!(matches!(
            decode(&archive, &before),
            Err(ExecutionError::Denied)
        ));
        Ok(())
    }

    fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
        let mut output = Vec::new();
        let mut chunk = [0u8; 4];
        for bytes in text.as_bytes().chunks(4) {
            if bytes.len() != 4 {
                return Err("base64 length".into());
            }
            for (index, byte) in bytes.iter().enumerate() {
                chunk[index] = match byte {
                    b'A'..=b'Z' => byte - b'A',
                    b'a'..=b'z' => byte - b'a' + 26,
                    b'0'..=b'9' => byte - b'0' + 52,
                    b'+' => 62,
                    b'/' => 63,
                    b'=' => 64,
                    _ => return Err("base64 alphabet".into()),
                };
            }
            output.push((chunk[0] << 2) | (chunk[1] >> 4));
            if chunk[2] != 64 {
                output.push((chunk[1] << 4) | (chunk[2] >> 2));
            }
            if chunk[3] != 64 {
                output.push((chunk[2] << 6) | chunk[3]);
            }
        }
        Ok(output)
    }

    #[test]
    fn rejects_framing_checksum_padding_and_terminator_attacks() -> Result<(), String> {
        let before = source(&[("f", b"old")], &[])?;
        let valid = exported(&before, &BTreeMap::new());
        let mut cases = vec![vec![], valid[..valid.len() - 1].to_vec()];
        let mut bad_checksum = valid.clone();
        bad_checksum[0] ^= 1;
        cases.push(bad_checksum);
        let mut bad_padding = valid.clone();
        bad_padding[2 * BLOCK + 3] = 1;
        cases.push(bad_padding);
        let mut one_end = valid.clone();
        one_end.truncate(one_end.len() - BLOCK);
        cases.push(one_end);
        let mut after_end = valid.clone();
        *after_end.last_mut().ok_or("last")? = 1;
        cases.push(after_end);
        for archive in cases {
            assert!(matches!(
                decode(&archive, &before),
                Err(ExecutionError::Denied)
            ));
        }
        for kind in *b"1346gS" {
            let mut archive = exported(&before, &BTreeMap::new());
            let header = first_header(&mut archive, 1);
            header[156] = kind;
            set_checksum(header);
            assert!(matches!(
                decode(&archive, &before),
                Err(ExecutionError::Denied)
            ));
        }
        Ok(())
    }

    #[test]
    fn rejects_extensions_links_ownership_and_modes() -> Result<(), String> {
        let before = source(&[("f", b"old")], &[])?;
        for mutation in 0..11 {
            let mut archive = exported(&before, &BTreeMap::new());
            let header = first_header(&mut archive, if mutation == 0 { 0 } else { 1 });
            match mutation {
                0 => test_octal(&mut header[100..108], 0o755),
                1 => header[156] = b'x',
                2 => header[156] = b'2',
                3 => header[157] = b'x',
                4 => test_octal(&mut header[100..108], 0o644),
                5 => test_octal(&mut header[108..116], 0),
                6 => test_octal(&mut header[116..124], 0),
                7 => test_octal(&mut header[329..337], 1),
                8 => header[257] = b'X',
                9 => header[100] = b'8',
                10 => header[154] = b' ',
                _ => unreachable!(),
            }
            if mutation != 10 {
                set_checksum(header);
            }
            assert!(matches!(
                decode(&archive, &before),
                Err(ExecutionError::Denied)
            ));
        }
        Ok(())
    }

    #[test]
    fn resolution_allows_0644_only_for_a_transient_root_lock() -> Result<(), String> {
        let before = source(&[("Cargo.lock", b""), ("src/lib.rs", b"old")], &[])?;
        let mut transient = exported(&before, &BTreeMap::new());
        let lock = first_header(&mut transient, 2);
        test_octal(&mut lock[100..108], 0o644);
        set_checksum(lock);
        assert!(decode_resolution(&transient, &before, true).is_ok());
        assert!(matches!(
            decode_resolution(&transient, &before, false),
            Err(ExecutionError::Denied)
        ));
        assert!(matches!(
            decode(&transient, &before),
            Err(ExecutionError::Denied)
        ));

        let mut other = exported(&before, &BTreeMap::new());
        let source = first_header(&mut other, 3);
        test_octal(&mut source[100..108], 0o644);
        set_checksum(source);
        assert!(matches!(
            decode_resolution(&other, &before, true),
            Err(ExecutionError::Denied)
        ));
        let mut permissive = transient;
        let lock = first_header(&mut permissive, 2);
        test_octal(&mut lock[100..108], 0o777);
        set_checksum(lock);
        assert!(matches!(
            decode_resolution(&permissive, &before, true),
            Err(ExecutionError::Denied)
        ));
        Ok(())
    }

    #[test]
    fn rejects_path_normalization_and_encoding_attacks() -> Result<(), String> {
        let before = source(&[("f", b"old")], &[])?;
        for raw in [b"/f".as_slice(), b"f", b"./../f", b"./f/"] {
            let mut archive = exported(&before, &BTreeMap::new());
            let header = first_header(&mut archive, 1);
            header[..100].fill(0);
            header[..raw.len()].copy_from_slice(raw);
            set_checksum(header);
            assert!(matches!(
                decode(&archive, &before),
                Err(ExecutionError::Denied)
            ));
        }
        let mut non_utf8 = exported(&before, &BTreeMap::new());
        let header = first_header(&mut non_utf8, 1);
        header[..100].fill(0);
        header[..4].copy_from_slice(b"./f\xff");
        set_checksum(header);
        assert!(matches!(
            decode(&non_utf8, &before),
            Err(ExecutionError::Denied)
        ));
        let mut hidden_suffix = exported(&before, &BTreeMap::new());
        let header = first_header(&mut hidden_suffix, 1);
        header[4] = b'x';
        set_checksum(header);
        assert!(matches!(
            decode(&hidden_suffix, &before),
            Err(ExecutionError::Denied)
        ));
        Ok(())
    }

    #[test]
    fn rejects_extra_missing_duplicate_and_type_collision() -> Result<(), String> {
        let before = source(&[("d/f", b"old")], &[])?;
        let mut missing = exported(&before, &BTreeMap::new());
        missing.drain(BLOCK..BLOCK * 2);
        let mut missing_file = exported(&before, &BTreeMap::new());
        missing_file.drain(2 * BLOCK..4 * BLOCK);
        let mut extra = exported(&before, &BTreeMap::new());
        extra.truncate(extra.len() - END_BLOCKS * BLOCK);
        test_entry(
            &mut extra, "./extra", b"", false, FILE_MODE, OWNER, OWNER, b'0',
        );
        extra.resize(extra.len() + END_BLOCKS * BLOCK, 0);
        let mut duplicate = exported(&before, &BTreeMap::new());
        duplicate.truncate(duplicate.len() - END_BLOCKS * BLOCK);
        test_entry(
            &mut duplicate,
            "./d/f",
            b"old",
            false,
            FILE_MODE,
            OWNER,
            OWNER,
            b'0',
        );
        duplicate.resize(duplicate.len() + END_BLOCKS * BLOCK, 0);
        let mut collision = exported(&before, &BTreeMap::new());
        let header = first_header(&mut collision, 1);
        header[156] = b'0';
        test_octal(&mut header[100..108], FILE_MODE);
        set_checksum(header);
        for archive in [missing, missing_file, extra, duplicate, collision] {
            assert!(matches!(
                decode(&archive, &before),
                Err(ExecutionError::Denied)
            ));
        }
        Ok(())
    }

    #[test]
    fn enforces_file_total_entry_and_archive_bounds() -> Result<(), String> {
        let one = source(&[("f", b"")], &[])?;
        let oversized = exported(
            &one,
            &BTreeMap::from([("f", vec![0; SOURCE_MAX_FILE_BYTES + 1])]),
        );
        assert!(matches!(
            decode(&oversized, &one),
            Err(ExecutionError::Denied)
        ));

        let names = (0..17).map(|index| format!("f{index}")).collect::<Vec<_>>();
        let files = names
            .iter()
            .map(|name| (name.as_str(), &b""[..]))
            .collect::<Vec<_>>();
        let before = source(&files, &[])?;
        let replacements = names
            .iter()
            .map(|name| (name.as_str(), vec![0; SOURCE_MAX_FILE_BYTES]))
            .collect::<BTreeMap<_, _>>();
        assert!(matches!(
            decode(&exported(&before, &replacements), &before),
            Err(ExecutionError::Denied)
        ));
        assert!(matches!(
            decode(&vec![0; MAX_ARCHIVE + BLOCK], &one),
            Err(ExecutionError::Denied)
        ));
        Ok(())
    }

    #[test]
    fn maximum_source_bytes_roundtrip_and_encoder_remain_bounded() -> Result<(), String> {
        let names = (0..16).map(|index| format!("f{index}")).collect::<Vec<_>>();
        let owned = vec![vec![42; SOURCE_MAX_FILE_BYTES]; names.len()];
        let files = names
            .iter()
            .zip(&owned)
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>();
        let before = source(&files, &[])?;
        let encoded = encode(&before).map_err(|error| format!("{error:?}"))?;
        assert!(encoded.len() <= MAX_ARCHIVE);
        let decoded = decode(&exported(&before, &BTreeMap::new()), &before)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(decoded, before);

        let names = (0..SOURCE_MAX_ENTRIES)
            .map(|index| format!("e{index}"))
            .collect::<Vec<_>>();
        let files = names
            .iter()
            .map(|name| (name.as_str(), &b""[..]))
            .collect::<Vec<_>>();
        let maximum_entries = source(&files, &[])?;
        assert!(
            encode(&maximum_entries)
                .map_err(|error| format!("{error:?}"))?
                .len()
                <= MAX_ARCHIVE
        );
        let decoded = decode(
            &exported(&maximum_entries, &BTreeMap::new()),
            &maximum_entries,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(decoded, maximum_entries);
        Ok(())
    }
}

/// What the store cannot see once an archive is one opaque member.
///
/// ADR-061 bounds `members/job` **including archive entries**, but the quality
/// store charges one descriptor per `ArchiveBundle`. The egress/wiring layer
/// must therefore charge `entries` against the job's declared member budget
/// before publishing the bundle; that aggregate is the integrator's obligation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)] // consumed by the M3 quality egress gateway.
pub struct ArchiveBundleStats {
    /// Every USTAR entry counted, including the `./` root and directories.
    pub entries: u16,
}

/// ADR-061 `ArchiveBundle` ingress: revalidate a bounded closed-USTAR stream
/// written by the fixed approved guest program and re-encode it canonically.
///
/// The profile is exactly the closed M2 export profile — one `./` root entry,
/// regular files and directories only, fixed mode/owner, no links, devices,
/// FIFOs, sparse members, PAX or GNU extensions, no `..`, bounded name, entry
/// count and size — plus ADR-061's member ceiling. Member names remain data:
/// nothing here opens, creates or extracts to a host path, and the returned
/// bytes are stored as one opaque `application/x-tar` member.
///
/// The returned [`ArchiveBundleStats`] is what makes the ADR's aggregate
/// members/job bound computable by the caller that publishes the member.
#[allow(dead_code)] // wired by the M3 quality egress gateway; ingress profile qualified here.
pub fn revalidate_quality_archive(
    archive: &[u8],
) -> Result<(Vec<u8>, ArchiveBundleStats), ExecutionError> {
    // First pass: read only the declared member names, so the strict decoder
    // below can be reused unchanged against an archive with no prior bundle.
    if archive.len() > MAX_ARCHIVE || !archive.len().is_multiple_of(BLOCK) {
        return denied();
    }
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut offset = 0usize;
    let mut entries = 0usize;
    while offset < archive.len() {
        let end = offset.checked_add(BLOCK).ok_or(ExecutionError::Denied)?;
        let header = archive.get(offset..end).ok_or(ExecutionError::Denied)?;
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        entries = entries.checked_add(1).ok_or(ExecutionError::Denied)?;
        if entries > usize::from(rust_engineering_domain::QUALITY_MAX_JOB_MEMBERS) {
            return denied();
        }
        let directory = match header[156] {
            b'0' => false,
            b'5' => true,
            _ => return denied(),
        };
        if let Some(path) = exported_path(header, directory)? {
            if directory {
                directories.push(path);
            } else {
                files.push(SourceFile::new(path, Vec::new()).map_err(|_| ExecutionError::Denied)?);
            }
        }
        let size = strict_octal(&header[124..136])?;
        offset = end
            .checked_add(size.div_ceil(BLOCK) * BLOCK)
            .ok_or(ExecutionError::Denied)?;
    }
    let declared =
        SourceBundle::with_directories(files, directories).map_err(|_| ExecutionError::Denied)?;
    let stats = ArchiveBundleStats {
        entries: u16::try_from(entries).map_err(|_| ExecutionError::Denied)?,
    };
    Ok((encode(&decode(archive, &declared)?)?, stats))
}

#[cfg(test)]
mod quality_archive_tests {
    use super::*;

    /// A guest-side USTAR writer, independent of the encoder under test.
    fn entry(output: &mut Vec<u8>, path: &str, bytes: &[u8], kind: u8, mode: usize) {
        let mut header = [0u8; BLOCK];
        let name = path.as_bytes();
        header[..name.len().min(100)].copy_from_slice(&name[..name.len().min(100)]);
        fn octal(field: &mut [u8], value: usize) {
            let text = format!("{value:0width$o}", width = field.len() - 1);
            let take = text.len().min(field.len());
            field[..take].copy_from_slice(&text.as_bytes()[..take]);
        }
        octal(&mut header[100..108], mode);
        octal(&mut header[108..116], OWNER);
        octal(&mut header[116..124], OWNER);
        octal(&mut header[124..136], bytes.len());
        octal(&mut header[136..148], 1);
        header[148..156].fill(b' ');
        header[156] = kind;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[265..272].copy_from_slice(b"nobody\0");
        header[297..305].copy_from_slice(b"nogroup\0");
        octal(&mut header[329..337], 0);
        octal(&mut header[337..345], 0);
        let value = checksum(&header);
        octal(&mut header[148..155], value);
        header[154] = 0;
        header[155] = b' ';
        output.extend_from_slice(&header);
        output.extend_from_slice(bytes);
        output.resize(
            output.len() + bytes.len().div_ceil(BLOCK) * BLOCK - bytes.len(),
            0,
        );
    }

    fn stream(files: &[(&str, &[u8])], directories: &[&str]) -> Vec<u8> {
        let mut output = Vec::new();
        entry(&mut output, "./", &[], b'5', DIRECTORY_MODE);
        for directory in directories {
            entry(
                &mut output,
                &format!("./{directory}/"),
                &[],
                b'5',
                DIRECTORY_MODE,
            );
        }
        for (path, bytes) in files {
            entry(&mut output, &format!("./{path}"), bytes, b'0', FILE_MODE);
        }
        output.resize(output.len() + END_BLOCKS * BLOCK, 0);
        output
    }

    fn bundle(files: &[(&str, &[u8])], directories: &[&str]) -> Result<SourceBundle, String> {
        let files = files
            .iter()
            .map(|(path, bytes)| SourceFile::new((*path).into(), bytes.to_vec()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("{error:?}"))?;
        SourceBundle::with_directories(
            files,
            directories.iter().map(|path| (*path).into()).collect(),
        )
        .map_err(|error| format!("{error:?}"))
    }

    #[test]
    fn a_bounded_regular_archive_canonicalizes_without_extraction() -> Result<(), String> {
        let members: &[(&str, &[u8])] = &[
            ("report/index.html", b"<p>ok</p>"),
            ("report/style.css", b"x"),
        ];
        let bundle = bundle(members, &["report", "report/empty"])?;
        let stream = stream(members, &["report", "report/empty"]);
        let (canonical, stats) =
            revalidate_quality_archive(&stream).map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            canonical,
            encode(&bundle).map_err(|error| format!("{error:?}"))?
        );
        // The aggregate the ADR bounds is the entry count, not the member: `./`,
        // two directories and two files, which the integrator charges to the job.
        assert_eq!(stats, ArchiveBundleStats { entries: 5 });
        // Re-encoding is stable, so the stored member is a canonical function
        // of the guest bytes and never of their arrival order.
        assert_eq!(
            revalidate_quality_archive(&stream).map_err(|error| format!("{error:?}"))?,
            (canonical.clone(), stats)
        );
        assert!(canonical.len() <= MAX_ARCHIVE);
        Ok(())
    }

    #[test]
    fn links_devices_traversal_extensions_and_over_count_are_rejected() -> Result<(), String> {
        // Every non-regular type flag: hard link, symlink, character and block
        // device, FIFO, contiguous, and the PAX/GNU extension headers.
        for kind in *b"123467xgLK" {
            let mut hostile = Vec::new();
            entry(&mut hostile, "./", &[], b'5', DIRECTORY_MODE);
            entry(&mut hostile, "./report/evil", &[], kind, FILE_MODE);
            hostile.resize(hostile.len() + END_BLOCKS * BLOCK, 0);
            assert!(
                revalidate_quality_archive(&hostile).is_err(),
                "type flag {kind}"
            );
        }
        // Traversal and absolute names.
        for name in [
            "./../escape",
            "/etc/passwd",
            "./report/../../escape",
            "../x",
        ] {
            let mut hostile = Vec::new();
            entry(&mut hostile, "./", &[], b'5', DIRECTORY_MODE);
            entry(&mut hostile, name, b"x", b'0', FILE_MODE);
            hostile.resize(hostile.len() + END_BLOCKS * BLOCK, 0);
            assert!(revalidate_quality_archive(&hostile).is_err(), "{name}");
        }
        // A member above the closed per-file ceiling.
        let big = vec![0_u8; SOURCE_MAX_FILE_BYTES + 1];
        assert!(revalidate_quality_archive(&stream(&[("report/big", &big)], &["report"])).is_err());
        // A setuid or world-readable mode is outside the closed profile.
        let mut permissive = Vec::new();
        entry(&mut permissive, "./", &[], b'5', DIRECTORY_MODE);
        entry(&mut permissive, "./report", b"x", b'0', 0o755);
        permissive.resize(permissive.len() + END_BLOCKS * BLOCK, 0);
        assert!(revalidate_quality_archive(&permissive).is_err());

        // More members than ADR-061 admits for one job.
        let ceiling = usize::from(rust_engineering_domain::QUALITY_MAX_JOB_MEMBERS);
        let names = (0..ceiling)
            .map(|index| format!("m{index}"))
            .collect::<Vec<_>>();
        let members = names
            .iter()
            .map(|name| (name.as_str(), &b"x"[..]))
            .collect::<Vec<_>>();
        assert!(revalidate_quality_archive(&stream(&members, &[])).is_err());
        // One below the ceiling, counting the `./` root entry, canonicalizes.
        let within = &members[..ceiling - 1];
        let (canonical, stats) = revalidate_quality_archive(&stream(within, &[]))
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            canonical,
            encode(&bundle(within, &[])?).map_err(|error| format!("{error:?}"))?
        );
        // Exactly the ceiling in entries: one bundle can exhaust a whole job's
        // declared member budget once the integrator charges this count.
        assert_eq!(
            stats,
            ArchiveBundleStats {
                entries: u16::try_from(ceiling).map_err(|error| format!("{error:?}"))?
            }
        );
        Ok(())
    }
}
