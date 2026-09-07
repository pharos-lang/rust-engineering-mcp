//! Bounded parser for the full `llvm-cov export -format=text` JSON envelope.
//!
//! This intentionally uses Serde's streaming deserializer and capped sequence
//! visitors. It never builds a `serde_json::Value` DOM from guest-controlled
//! report bytes.

use rust_engineering_domain::coverage::{
    CoverageFile, CoverageMetrics, CoveragePackage, CoverageSummary,
};
use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use std::{collections::BTreeMap, fmt};

pub const MAX_COVERAGE_JSON_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_COVERAGE_FILES: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedCoverageJson {
    pub cargo_llvm_cov_version: String,
    pub manifest_path: String,
    pub summary: CoverageSummary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageJsonError {
    TooLarge,
    Invalid,
    MissingField,
    InvalidMetric,
}
impl fmt::Display for CoverageJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid bounded coverage JSON")
    }
}
impl std::error::Error for CoverageJsonError {}

#[derive(Clone, Copy, Default)]
struct Counts {
    count: u64,
    covered: u64,
}
impl<'de> de::Deserialize<'de> for Counts {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CountsVisitor;
        impl<'de> Visitor<'de> for CountsVisitor {
            type Value = Counts;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("coverage counts")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Counts, M::Error> {
                let mut count = None;
                let mut covered = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "count" => count = Some(map.next_value()?),
                        "covered" => covered = Some(map.next_value()?),
                        _ => {
                            let _: de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                Ok(Counts {
                    count: count.ok_or_else(|| de::Error::missing_field("count"))?,
                    covered: covered.ok_or_else(|| de::Error::missing_field("covered"))?,
                })
            }
        }
        deserializer.deserialize_map(CountsVisitor)
    }
}

#[derive(Default)]
struct FileSummary {
    lines: Counts,
    regions: Counts,
    functions: Counts,
}
impl<'de> de::Deserialize<'de> for FileSummary {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SummaryVisitor;
        impl<'de> Visitor<'de> for SummaryVisitor {
            type Value = FileSummary;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("coverage summary")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<FileSummary, M::Error> {
                let mut lines = None;
                let mut regions = None;
                let mut functions = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "lines" => lines = Some(map.next_value()?),
                        "regions" => regions = Some(map.next_value()?),
                        "functions" => functions = Some(map.next_value()?),
                        _ => {
                            let _: de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                Ok(FileSummary {
                    lines: lines.ok_or_else(|| de::Error::missing_field("lines"))?,
                    regions: regions.ok_or_else(|| de::Error::missing_field("regions"))?,
                    functions: functions.ok_or_else(|| de::Error::missing_field("functions"))?,
                })
            }
        }
        deserializer.deserialize_map(SummaryVisitor)
    }
}

struct RawFile {
    filename: String,
    summary: FileSummary,
    package: String,
}

fn normalized_source_path(value: &str) -> Option<String> {
    let relative = value.strip_prefix("/source/").unwrap_or(value);
    if relative.starts_with('/') || relative.is_empty() {
        return None;
    }
    let mut components = Vec::new();
    for component in relative.split('/') {
        match component {
            "" | "." => return None,
            ".." => {
                components.pop()?;
            }
            value => components.push(value),
        }
    }
    (!components.is_empty()).then(|| components.join("/"))
}

impl<'de> de::Deserialize<'de> for RawFile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FileVisitor;
        impl<'de> Visitor<'de> for FileVisitor {
            type Value = RawFile;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("coverage file")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<RawFile, M::Error> {
                let mut filename = None;
                let mut summary = None;
                let mut package = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "filename" => filename = Some(map.next_value()?),
                        "summary" => summary = Some(map.next_value()?),
                        "package" => package = Some(map.next_value()?),
                        _ => {
                            let _: de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                let filename: String =
                    filename.ok_or_else(|| de::Error::missing_field("filename"))?;
                let filename = normalized_source_path(&filename)
                    .ok_or_else(|| de::Error::custom("filename escapes source"))?;
                if filename.len() > 4096 {
                    return Err(de::Error::custom("invalid filename"));
                }
                Ok(RawFile {
                    filename,
                    summary: summary.ok_or_else(|| de::Error::missing_field("summary"))?,
                    package: package.unwrap_or_else(|| "workspace".to_owned()),
                })
            }
        }
        deserializer.deserialize_map(FileVisitor)
    }
}

fn metrics(value: &FileSummary) -> Result<CoverageMetrics, CoverageJsonError> {
    CoverageMetrics::new(
        (value.lines.count, value.lines.covered),
        (value.regions.count, value.regions.covered),
        (value.functions.count, value.functions.covered),
    )
    .map_err(|_| CoverageJsonError::InvalidMetric)
}
fn add(a: &mut Counts, b: Counts) -> Result<(), CoverageJsonError> {
    a.count = a
        .count
        .checked_add(b.count)
        .ok_or(CoverageJsonError::InvalidMetric)?;
    a.covered = a
        .covered
        .checked_add(b.covered)
        .ok_or(CoverageJsonError::InvalidMetric)?;
    Ok(())
}

#[derive(Default)]
struct Document {
    version: Option<String>,
    manifest_path: Option<String>,
    files: BTreeMap<String, RawFile>,
}
impl<'de> de::Deserialize<'de> for Document {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DocVisitor;
        impl<'de> Visitor<'de> for DocVisitor {
            type Value = Document;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("llvm coverage export")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Document, M::Error> {
                let mut doc = Document::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "cargo_llvm_cov" => {
                            let meta: CargoMeta = map.next_value()?;
                            doc.version = Some(meta.version);
                            doc.manifest_path = Some(meta.manifest_path);
                        }
                        "data" => {
                            map.next_value_seed(DataSeed(&mut doc.files))?;
                        }
                        _ => {
                            let _: de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                Ok(doc)
            }
        }
        deserializer.deserialize_map(DocVisitor)
    }
}
struct CargoMeta {
    version: String,
    manifest_path: String,
}
impl<'de> de::Deserialize<'de> for CargoMeta {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = CargoMeta;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("cargo llvm cov metadata")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> Result<CargoMeta, M::Error> {
                let (mut v, mut p) = (None, None);
                while let Some(k) = m.next_key::<String>()? {
                    match k.as_str() {
                        "version" => v = Some(m.next_value()?),
                        "manifest_path" => p = Some(m.next_value()?),
                        _ => {
                            let _: de::IgnoredAny = m.next_value()?;
                        }
                    }
                }
                Ok(CargoMeta {
                    version: v.ok_or_else(|| de::Error::missing_field("version"))?,
                    manifest_path: p.ok_or_else(|| de::Error::missing_field("manifest_path"))?,
                })
            }
        }
        d.deserialize_map(V)
    }
}
struct DataSeed<'a>(&'a mut BTreeMap<String, RawFile>);
impl<'de> de::DeserializeSeed<'de> for DataSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(DataVisitor(self.0))
    }
}
struct DataVisitor<'a>(&'a mut BTreeMap<String, RawFile>);
impl<'de> Visitor<'de> for DataVisitor<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("coverage data array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while let Some(()) = seq.next_element_seed(UnitSeed(self.0))? {}
        Ok(())
    }
}
struct UnitSeed<'a>(&'a mut BTreeMap<String, RawFile>);
impl<'de> de::DeserializeSeed<'de> for UnitSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_map(UnitVisitor(self.0))
    }
}
struct UnitVisitor<'a>(&'a mut BTreeMap<String, RawFile>);
impl<'de> Visitor<'de> for UnitVisitor<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("coverage data item")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> Result<(), M::Error> {
        while let Some(k) = m.next_key::<String>()? {
            if k == "files" {
                m.next_value_seed(FilesSeed(self.0))?
            } else {
                let _: de::IgnoredAny = m.next_value()?;
            }
        }
        Ok(())
    }
}
struct FilesSeed<'a>(&'a mut BTreeMap<String, RawFile>);
impl<'de> de::DeserializeSeed<'de> for FilesSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(FilesVisitor(self.0))
    }
}
struct FilesVisitor<'a>(&'a mut BTreeMap<String, RawFile>);
impl<'de> Visitor<'de> for FilesVisitor<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("coverage files")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<(), A::Error> {
        while let Some(file) = s.next_element::<RawFile>()? {
            if self.0.len() >= MAX_COVERAGE_FILES {
                return Err(de::Error::custom("too many files"));
            }
            self.0.entry(file.filename.clone()).or_insert(file);
        }
        Ok(())
    }
}

pub fn parse(bytes: &[u8]) -> Result<ParsedCoverageJson, CoverageJsonError> {
    if bytes.len() > MAX_COVERAGE_JSON_BYTES {
        return Err(CoverageJsonError::TooLarge);
    }
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let document = Document::deserialize(&mut d).map_err(|_| CoverageJsonError::Invalid)?;
    d.end().map_err(|_| CoverageJsonError::Invalid)?;
    let version = document.version.ok_or(CoverageJsonError::MissingField)?;
    let manifest_path = document
        .manifest_path
        .ok_or(CoverageJsonError::MissingField)?;
    if version.is_empty()
        || version.len() > 128
        || manifest_path.is_empty()
        || manifest_path.len() > 4096
    {
        return Err(CoverageJsonError::Invalid);
    }
    let mut lines = Counts::default();
    let mut regions = Counts::default();
    let mut functions = Counts::default();
    let mut package_counts: BTreeMap<String, (Counts, Counts, Counts)> = BTreeMap::new();
    let mut files = Vec::new();
    for (_, file) in document.files {
        add(&mut lines, file.summary.lines)?;
        add(&mut regions, file.summary.regions)?;
        add(&mut functions, file.summary.functions)?;
        let entry = package_counts.entry(file.package.clone()).or_default();
        add(&mut entry.0, file.summary.lines)?;
        add(&mut entry.1, file.summary.regions)?;
        add(&mut entry.2, file.summary.functions)?;
        files.push(CoverageFile {
            path: file.filename,
            package: file.package,
            metrics: metrics(&file.summary)?,
        });
    }
    let packages = package_counts
        .into_iter()
        .map(|(name, (l, r, f))| {
            Ok(CoveragePackage {
                name,
                metrics: CoverageMetrics::new(
                    (l.count, l.covered),
                    (r.count, r.covered),
                    (f.count, f.covered),
                )
                .map_err(|_| CoverageJsonError::InvalidMetric)?,
            })
        })
        .collect::<Result<Vec<_>, CoverageJsonError>>()?;
    Ok(ParsedCoverageJson {
        cargo_llvm_cov_version: version,
        manifest_path,
        summary: CoverageSummary {
            aggregate: CoverageMetrics::new(
                (lines.count, lines.covered),
                (regions.count, regions.covered),
                (functions.count, functions.covered),
            )
            .map_err(|_| CoverageJsonError::InvalidMetric)?,
            packages,
            files,
            files_omitted: 0,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deduplicates_a_shared_filename_and_keeps_zero_denominators_absent()
    -> Result<(), CoverageJsonError> {
        let input=br#"{"cargo_llvm_cov":{"version":"0.9.0","manifest_path":"/source/Cargo.toml"},"data":[{"files":[{"filename":"src/shared.rs","package":"a","summary":{"lines":{"count":2,"covered":1},"regions":{"count":0,"covered":0},"functions":{"count":1,"covered":1}}},{"filename":"src/shared.rs","package":"b","summary":{"lines":{"count":99,"covered":99},"regions":{"count":1,"covered":1},"functions":{"count":1,"covered":1}}}]}]}"#;
        let parsed = parse(input)?;
        assert_eq!(parsed.summary.files.len(), 1);
        assert_eq!(
            parsed
                .summary
                .aggregate
                .lines
                .ok_or(CoverageJsonError::MissingField)?
                .count,
            2
        );
        assert!(parsed.summary.aggregate.regions.is_none());
        Ok(())
    }
    #[test]
    fn rejects_missing_metadata_and_large_input() {
        assert_eq!(parse(br#"{}"#), Err(CoverageJsonError::MissingField));
        assert_eq!(
            parse(&vec![b' '; MAX_COVERAGE_JSON_BYTES + 1]),
            Err(CoverageJsonError::TooLarge)
        );
    }

    #[test]
    fn canonicalizes_shared_source_paths_and_rejects_source_escapes() {
        assert_eq!(
            normalized_source_path("/source/a/src/../../shared.rs").as_deref(),
            Some("shared.rs")
        );
        assert_eq!(
            normalized_source_path("/source/b/src/../../shared.rs").as_deref(),
            Some("shared.rs")
        );
        assert_eq!(normalized_source_path("/source/../escape.rs"), None);
        assert_eq!(normalized_source_path("/other/file.rs"), None);
    }
}
