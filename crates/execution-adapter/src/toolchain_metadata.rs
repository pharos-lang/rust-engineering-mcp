//! Bounded observations of the immutable approved Linux ARM64 runtime.
use rust_engineering_application::InspectionError;
use rust_engineering_domain::{
    InstalledComponent, InstalledComponentKind, ToolchainChannel, ToolchainInventory,
};

const MAX_BYTES: usize = 16 * 1024;
const MAX_IDENTIFIER: usize = 128;
const MAX_ENTRIES: usize = 32;
const VERSION: &str = "1.98.1";
const HOST: &str = "aarch64-unknown-linux-gnu";

// Observed in the M3 provisioning receipt for approved image 384a1742... . These
// are runtime output records, not Cargo's distribution-package version 0.99.0.
const RUST_HEADER: &str = "rustc 1.98.1 (48a229cea 2026-09-01)";
const CARGO_HEADER: &str = "cargo 1.98.1 (797e8a9bc 2026-08-05)";
const RUST_FIELDS: &[(&str, &str)] = &[
    ("binary", "rustc"),
    ("commit-hash", "48a229ceaefd4985c50990b14116b6d856af0985"),
    ("commit-date", "2026-09-01"),
    ("host", HOST),
    ("release", VERSION),
    ("LLVM version", "22.1.8"),
];
const CARGO_FIELDS: &[(&str, &str)] = &[
    ("release", VERSION),
    ("commit-hash", "797e8a9bca276c1c9f9f738d2a20f484fa4eea9d"),
    ("commit-date", "2026-08-05"),
    ("host", HOST),
    ("libgit2", "1.9.4 (sys:0.21.0 vendored)"),
    (
        "libcurl",
        "8.21.0-DEV (sys:0.4.90+curl-8.21.0 vendored ssl:OpenSSL/3.6.3)",
    ),
    ("ssl", "OpenSSL 3.6.3 9 Jun 2026"),
    ("os", "Debian 12.0.0 (bookworm) [64-bit]"),
];

fn lines(bytes: &[u8]) -> Result<Vec<&str>, InspectionError> {
    if bytes.len() > MAX_BYTES {
        return Err(InspectionError::OutputLimit);
    }
    // These fixed Linux tools emit LF-terminated ASCII records. Reject control
    // bytes, malformed UTF-8 and a cut final line rather than repairing evidence.
    if bytes.is_empty()
        || !bytes.ends_with(b"\n")
        || bytes
            .iter()
            .any(|b| *b != b'\n' && !(0x20..=0x7e).contains(b))
    {
        return Err(InspectionError::InvalidMetadata);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| InspectionError::InvalidMetadata)?;
    let lines: Vec<_> = text.split_terminator('\n').take(MAX_ENTRIES + 1).collect();
    if lines.len() > MAX_ENTRIES {
        return Err(InspectionError::OutputLimit);
    }
    if lines.iter().any(|line| line.is_empty()) {
        return Err(InspectionError::InvalidMetadata);
    }
    Ok(lines)
}

fn version_records(
    bytes: &[u8],
    header: &str,
    expected: &[(&str, &str)],
) -> Result<(), InspectionError> {
    let lines = lines(bytes)?;
    if lines[0].len() > MAX_IDENTIFIER {
        return Err(InspectionError::OutputLimit);
    }
    if lines[0] != header {
        return Err(InspectionError::InvalidMetadata);
    }
    let mut seen = vec![false; expected.len()];
    for line in &lines[1..] {
        let (name, value) = line
            .split_once(": ")
            .ok_or(InspectionError::InvalidMetadata)?;
        if name.len() > MAX_IDENTIFIER || value.len() > MAX_IDENTIFIER {
            return Err(InspectionError::OutputLimit);
        }
        let index = expected
            .iter()
            .position(|(key, _)| *key == name)
            .ok_or(InspectionError::InvalidMetadata)?;
        if seen[index] || value != expected[index].1 {
            return Err(InspectionError::InvalidMetadata);
        }
        seen[index] = true;
    }
    if seen.iter().any(|present| !present) {
        return Err(InspectionError::InvalidMetadata);
    }
    Ok(())
}

pub(super) fn parse(
    rustc: &[u8],
    cargo: &[u8],
    components: &[u8],
) -> Result<ToolchainInventory, InspectionError> {
    // Check every stream's byte bound before parsing/allocating any records.
    if [rustc, cargo, components]
        .iter()
        .any(|bytes| bytes.len() > MAX_BYTES)
    {
        return Err(InspectionError::OutputLimit);
    }
    version_records(rustc, RUST_HEADER, RUST_FIELDS)?;
    version_records(cargo, CARGO_HEADER, CARGO_FIELDS)?;
    let mut installed = Vec::new();
    let mut llvm_tools_seen = false;
    for name in lines(components)? {
        if name.len() > MAX_IDENTIFIER {
            return Err(InspectionError::OutputLimit);
        }
        let component = match name {
            "cargo" => InstalledComponentKind::Cargo,
            "clippy-preview" => InstalledComponentKind::Clippy,
            "rust-std-aarch64-unknown-linux-gnu" => InstalledComponentKind::RustStd,
            "rustc" => InstalledComponentKind::Rustc,
            "rustfmt-preview" => InstalledComponentKind::Rustfmt,
            // M3 adds llvm-tools-preview for later coverage phases. The M1
            // public inventory enum remains byte-stable, so this image
            // qualification marker is required but intentionally not projected.
            "llvm-tools-preview" if !llvm_tools_seen => {
                llvm_tools_seen = true;
                continue;
            }
            _ => return Err(InspectionError::InvalidMetadata),
        };
        if installed
            .iter()
            .any(|entry: &InstalledComponent| entry.component == component)
        {
            return Err(InspectionError::InvalidMetadata);
        }
        installed.push(InstalledComponent {
            component,
            target: (component == InstalledComponentKind::RustStd).then(|| HOST.to_owned()),
        });
    }
    // The five M1 public components plus the M3 llvm-tools qualification marker
    // are the full approved image inventory.
    if installed.len() != 5 || !llvm_tools_seen {
        return Err(InspectionError::InvalidMetadata);
    }
    installed.sort_by_key(|entry| entry.component);
    let installed_targets = installed
        .iter()
        .filter_map(|entry| entry.target.clone())
        .collect();
    Ok(ToolchainInventory {
        rustc_version: VERSION.to_owned(),
        cargo_version: VERSION.to_owned(),
        channel: ToolchainChannel::Stable,
        host_triple: HOST.to_owned(),
        installed_targets,
        installed_components: installed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST: &str = "rustc 1.98.1 (48a229cea 2026-09-01)\nbinary: rustc\ncommit-hash: 48a229ceaefd4985c50990b14116b6d856af0985\ncommit-date: 2026-09-01\nhost: aarch64-unknown-linux-gnu\nrelease: 1.98.1\nLLVM version: 22.1.8\n";
    const CARGO: &str = "cargo 1.98.1 (797e8a9bc 2026-08-05)\nrelease: 1.98.1\ncommit-hash: 797e8a9bca276c1c9f9f738d2a20f484fa4eea9d\ncommit-date: 2026-08-05\nhost: aarch64-unknown-linux-gnu\nlibgit2: 1.9.4 (sys:0.21.0 vendored)\nlibcurl: 8.21.0-DEV (sys:0.4.90+curl-8.21.0 vendored ssl:OpenSSL/3.6.3)\nssl: OpenSSL 3.6.3 9 Jun 2026\nos: Debian 12.0.0 (bookworm) [64-bit]\n";
    const COMPONENTS: &str = "rustfmt-preview\nrustc\nrust-std-aarch64-unknown-linux-gnu\nclippy-preview\ncargo\nllvm-tools-preview\n";

    #[test]
    fn observed_image_inventory_is_complete_normalized_and_order_independent()
    -> Result<(), InspectionError> {
        let inventory = parse(RUST.as_bytes(), CARGO.as_bytes(), COMPONENTS.as_bytes())?;
        assert_eq!(inventory.rustc_version, "1.98.1");
        assert_eq!(inventory.cargo_version, "1.98.1");
        assert_eq!(inventory.host_triple, HOST);
        assert_eq!(inventory.channel, ToolchainChannel::Stable);
        assert_eq!(inventory.installed_targets, [HOST]);
        assert_eq!(inventory.installed_components.len(), 5);
        assert_eq!(
            inventory.installed_components[0].component,
            InstalledComponentKind::Cargo
        );
        assert!(
            inventory
                .installed_components
                .iter()
                .all(|entry| entry.target.is_some()
                    == (entry.component == InstalledComponentKind::RustStd))
        );
        let mut records: Vec<_> = RUST.lines().skip(1).collect();
        records.reverse();
        let reordered = format!("{RUST_HEADER}\n{}\n", records.join("\n"));
        let mut entries: Vec<_> = COMPONENTS.lines().collect();
        entries.reverse();
        let components = format!("{}\n", entries.join("\n"));
        assert_eq!(
            inventory,
            parse(
                reordered.as_bytes(),
                CARGO.as_bytes(),
                components.as_bytes()
            )?
        );
        Ok(())
    }

    #[test]
    fn rejects_mismatched_headers_release_host_and_incomplete_or_duplicate_records() {
        for broken in [
            RUST.replace("rustc 1.98.1", "rustc 1.98.0"),
            RUST.replace("release: 1.98.1", "release: 1.98.1-nightly"),
            RUST.replace(HOST, "aarch64-apple-darwin"),
            RUST.replace("LLVM version: 22.1.8\n", ""),
            format!("{RUST}host: {HOST}\n"),
            format!("{RUST}unknown: synthetic_secret\n"),
            RUST.replace("binary: rustc", "binary: cargo"),
        ] {
            assert_eq!(
                parse(broken.as_bytes(), CARGO.as_bytes(), COMPONENTS.as_bytes()),
                Err(InspectionError::InvalidMetadata)
            );
        }
        for broken in [
            CARGO.replace("cargo 1.98.1", "cargo 0.99.0"),
            CARGO.replace("release: 1.98.1", "release: 1.98.0"),
            CARGO.replace(HOST, "x86_64-unknown-linux-gnu"),
            CARGO.replace("libgit2: 1.9.4 (sys:0.21.0 vendored)\n", ""),
            format!("{CARGO}release: 1.98.1\n"),
        ] {
            assert_eq!(
                parse(RUST.as_bytes(), broken.as_bytes(), COMPONENTS.as_bytes()),
                Err(InspectionError::InvalidMetadata)
            );
        }
    }

    #[test]
    fn rejects_supported_uninstalled_targets_and_missing_unknown_duplicate_components() {
        for broken in [
            COMPONENTS.replace("cargo\n", ""),
            COMPONENTS.replace("llvm-tools-preview\n", ""),
            COMPONENTS.replace(
                "rust-std-aarch64-unknown-linux-gnu",
                "rust-std-wasm32-unknown-unknown",
            ),
            COMPONENTS.replace("clippy-preview", "clippy"),
            format!("{COMPONENTS}rust-src\n"),
            format!("{COMPONENTS}cargo\n"),
            format!("{COMPONENTS}llvm-tools-preview\n"),
            format!("{COMPONENTS}\n"),
            "".to_owned(),
        ] {
            assert_eq!(
                parse(RUST.as_bytes(), CARGO.as_bytes(), broken.as_bytes()),
                Err(InspectionError::InvalidMetadata)
            );
        }
    }

    #[test]
    fn rejects_invalid_bytes_cut_lines_and_whitespace_in_each_stream() {
        for invalid in [
            b"\xff\n".as_slice(),
            b"\0\n",
            b"\r\n",
            b"\n",
            b"record",
            b"",
        ] {
            assert_eq!(
                parse(invalid, CARGO.as_bytes(), COMPONENTS.as_bytes()),
                Err(InspectionError::InvalidMetadata)
            );
            assert_eq!(
                parse(RUST.as_bytes(), invalid, COMPONENTS.as_bytes()),
                Err(InspectionError::InvalidMetadata)
            );
            assert_eq!(
                parse(RUST.as_bytes(), CARGO.as_bytes(), invalid),
                Err(InspectionError::InvalidMetadata)
            );
        }
        assert_eq!(
            parse(
                RUST.trim_end().as_bytes(),
                CARGO.as_bytes(),
                COMPONENTS.as_bytes()
            ),
            Err(InspectionError::InvalidMetadata)
        );
        assert_eq!(
            parse(
                RUST.as_bytes(),
                CARGO.as_bytes(),
                COMPONENTS.replace("cargo\n", "cargo \n").as_bytes()
            ),
            Err(InspectionError::InvalidMetadata)
        );
    }

    #[test]
    fn enforces_byte_identifier_and_record_limits_without_partial_inventory() {
        let over_bytes = vec![b'x'; MAX_BYTES + 1];
        for streams in [
            [
                over_bytes.as_slice(),
                CARGO.as_bytes(),
                COMPONENTS.as_bytes(),
            ],
            [
                RUST.as_bytes(),
                over_bytes.as_slice(),
                COMPONENTS.as_bytes(),
            ],
            [RUST.as_bytes(), CARGO.as_bytes(), over_bytes.as_slice()],
        ] {
            assert_eq!(
                parse(streams[0], streams[1], streams[2]),
                Err(InspectionError::OutputLimit)
            );
        }
        let name = format!("{}\n", "x".repeat(MAX_IDENTIFIER + 1));
        assert_eq!(
            parse(RUST.as_bytes(), CARGO.as_bytes(), name.as_bytes()),
            Err(InspectionError::OutputLimit)
        );
        let value = RUST.replace(
            "binary: rustc",
            &format!("binary: {}", "x".repeat(MAX_IDENTIFIER + 1)),
        );
        assert_eq!(
            parse(value.as_bytes(), CARGO.as_bytes(), COMPONENTS.as_bytes()),
            Err(InspectionError::OutputLimit)
        );
        let excess = "cargo\n".repeat(MAX_ENTRIES + 1);
        assert_eq!(
            parse(RUST.as_bytes(), CARGO.as_bytes(), excess.as_bytes()),
            Err(InspectionError::OutputLimit)
        );
        // Exactly-at-limit invalid content is a semantic error, not overflow.
        let exact = format!("{}\n", "x".repeat(MAX_BYTES - 1));
        assert_eq!(lines(exact.as_bytes()).map(|_| ()), Ok(()));
        assert_eq!(
            lines("x\n".repeat(MAX_ENTRIES).as_bytes()).map(|_| ()),
            Ok(())
        );
    }
}
