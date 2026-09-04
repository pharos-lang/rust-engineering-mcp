//! Test-only packaging of the existing signed SQLite fixture and real native bytes.
//! Public seed 42 is fixture authority only; never a production signing identity.
use ring::signature::Ed25519KeyPair;
#[cfg(feature = "local")]
use rust_engineering_catalog::bundle::{BundleFile, sha256};
use rust_engineering_catalog::bundle::{PublisherTrust, verify};

#[cfg(feature = "local")]
pub fn with_native_index(
    original: &[u8],
    native: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let trust = PublisherTrust::parse(&std::fs::read(
        super::fixtures().join("fixture-trust.json"),
    )?)?;
    let verified = verify(original, &trust)?;
    let mut manifest = verified.manifest().clone();
    assert_eq!(manifest.sequence, 1);
    assert_eq!(manifest.files.len(), 1);

    // Only an authenticated, small repository-owned fixture is decoded here.
    // Walk its three fixed regular members and retain the original SQLite bytes.
    let archive = zstd::stream::decode_all(original)?;
    let mut offset = 0usize;
    let mut catalog = None;
    for expected in ["manifest.json", "signature.ed25519", "catalog.sqlite"] {
        let header = archive
            .get(offset..offset + 512)
            .ok_or("short fixture header")?;
        let end = header[..100]
            .iter()
            .position(|b| *b == 0)
            .ok_or("fixture name")?;
        assert_eq!(std::str::from_utf8(&header[..end])?, expected);
        assert_eq!(&header[257..263], b"ustar\0");
        assert_eq!(header[156], b'0');
        let size = usize::from_str_radix(
            std::str::from_utf8(&header[124..136])?.trim_matches(['\0', ' ']),
            8,
        )?;
        let start = offset + 512;
        let end = start.checked_add(size).ok_or("fixture size overflow")?;
        let data = archive.get(start..end).ok_or("short fixture payload")?;
        if expected == "catalog.sqlite" {
            catalog = Some(data.to_vec());
        }
        offset = end.next_multiple_of(512);
    }
    let catalog = catalog.ok_or("catalog absent")?;
    assert_eq!(sha256(&catalog), manifest.files[0].sha256);
    manifest.semantic_index_version = Some(1);
    manifest.embedding_model_id = Some("intfloat/multilingual-e5-small".into());
    manifest.files.push(BundleFile {
        path: "semantic.index".into(),
        byte_length: native.len() as u64,
        sha256: sha256(native),
    });
    let json = serde_json::to_vec(&manifest)?;
    let key = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).map_err(|_| "fixture signing key")?;
    let mut message = b"rust-engineering-catalog-bundle-v1\0".to_vec();
    message.extend_from_slice(&json);
    let signature = key.sign(&message);
    let mut output = Vec::new();
    for (name, bytes) in [
        ("manifest.json", json.as_slice()),
        ("signature.ed25519", signature.as_ref()),
        ("catalog.sqlite", catalog.as_slice()),
        ("semantic.index", native),
    ] {
        output.extend_from_slice(&header(name, bytes.len()));
        output.extend_from_slice(bytes);
        output.resize(output.len().next_multiple_of(512), 0);
    }
    output.resize(output.len() + 1024, 0);
    Ok(zstd::stream::encode_all(output.as_slice(), 1)?)
}

#[cfg(feature = "local")]
fn header(name: &str, size: usize) -> [u8; 512] {
    let mut header = [0; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    for (start, end, value) in [
        (100, 108, 0o600usize),
        (108, 116, 0),
        (116, 124, 0),
        (124, 136, size),
        (136, 148, 0),
        (329, 337, 0),
        (337, 345, 0),
    ] {
        header[start..end]
            .copy_from_slice(format!("{:0width$o}\0", value, width = end - start - 1).as_bytes());
    }
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    header[148..156].fill(b' ');
    let sum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    header
}

pub fn resign_with_new_key(
    original: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    use ring::signature::KeyPair;
    let mut trust = PublisherTrust::parse(&std::fs::read(
        super::fixtures().join("fixture-trust.json"),
    )?)?;
    verify(original, &trust)?;
    let mut archive = zstd::stream::decode_all(original)?;
    let size = usize::from_str_radix(
        std::str::from_utf8(&archive[124..136])?.trim_matches(['\0', ' ']),
        8,
    )?;
    let manifest = archive.get(512..512 + size).ok_or("manifest")?;
    let mut message = b"rust-engineering-catalog-bundle-v1\0".to_vec();
    message.extend_from_slice(manifest);
    let key = Ed25519KeyPair::from_seed_unchecked(&[43; 32]).map_err(|_| "test key")?;
    let signature_at = (512 + size).next_multiple_of(512) + 512;
    archive
        .get_mut(signature_at..signature_at + 64)
        .ok_or("signature")?
        .copy_from_slice(key.sign(&message).as_ref());
    trust.public_key = key
        .public_key()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((
        zstd::stream::encode_all(archive.as_slice(), 1)?,
        serde_json::to_vec(&trust)?,
    ))
}
