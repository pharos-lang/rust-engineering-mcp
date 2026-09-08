//! Bounded decoder for the USTAR export of the guest's `CRITERION_HOME` tree
//! (ADR-073 §3, ADR-076 §3).
//!
//! What this module turns into a domain measurement is deliberately narrow.
//! Criterion writes four JSON documents per benchmark under
//! `<CRITERION_HOME>/<directory_name>/new/`, and only two of them are evidence
//! a reader may build a verdict on:
//!
//! * `sample.json` — the RAW samples. `times[i]` is the nanoseconds the harness
//!   spent running the whole batch and `iters[i]` is that batch's iteration
//!   count, so per-iteration time is `times[i] / iters[i]`, derived by
//!   [`rust_engineering_domain::benchmark::RawSample::per_iteration_ns`] and
//!   never stored. This is the oracle ADR-073 §4 requires: statistics are
//!   recomputed from these samples.
//! * `benchmark.json` — the serialized `BenchmarkId`, i.e. the five identity
//!   fields the dataset carries.
//!
//! `estimates.json` and `tukey.json` are criterion's own precomputed statistics
//! and fences. This module RECOGNISES them — their presence proves the
//! directory is a criterion benchmark directory — and never decodes them:
//! ADR-073 §4 computes every statistic from the raw samples, so surfacing
//! criterion's numbers as a domain measurement would smuggle a second,
//! unaudited statistical method into a verdict. They may only ever be published
//! as an opaque artifact byte-stream, which is not this module's job.
//!
//! Nothing here trusts the project. The archive is decoded under an explicit
//! byte ceiling with a strict USTAR profile, benchmark identity is cross-checked
//! against the directory the data actually came from, and every value violation
//! refuses the whole archive rather than pruning it into something believable.
// The M5 benchmark gateway that consumes this parser is a separate, concurrent
// deliverable (ADR-076 §3); the decoder is complete and independently tested,
// so the crate has no non-test caller for it yet.
#![allow(dead_code)]

use rust_engineering_domain::benchmark::{
    BENCHMARK_MAX_SAMPLES, BenchmarkIdentity, BenchmarkMeasurement, MeasurementCompleteness,
    RawSample, SamplingMode,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// Byte ceiling on the whole export (ADR-076 §7: muestras ≤ 32 MiB).
pub(crate) const MAX_CRITERION_ARCHIVE: usize = 32 * 1024 * 1024;
/// Byte ceiling on one member. A criterion JSON document above this is not a
/// document this parser will hold in memory.
pub(crate) const MAX_CRITERION_FILE: usize = 8 * 1024 * 1024;
/// Ceiling on benchmark directories in one export.
pub(crate) const MAX_CRITERION_BENCHMARKS: usize = 512;

/// USTAR block size. Both the archive length and every member are framed on it.
const BLOCK: usize = 512;

/// Largest integer f64 represents exactly (2^53). An `iters` value above it
/// could not have survived criterion's own f64 accounting without loss, so it
/// is refused rather than rounded.
const MAX_EXACT_INTEGER: f64 = 9_007_199_254_740_992.0;

/// What one export produced, with the counters a caller needs to declare the
/// dataset's completeness instead of inferring it.
pub(crate) struct CriterionParse {
    /// One measurement per benchmark that carried both `benchmark.json` and
    /// `sample.json`, sorted by `full_id` ascending so the result is a function
    /// of the archive's content and never of its member order.
    pub measurements: Vec<BenchmarkMeasurement>,
    /// Distinct directories that carried at least one recognised
    /// `<directory_name>/new/<file>` member.
    ///
    /// A directory known only through `base/`, `report/` or a planted file is
    /// NOT counted: `new/` is where criterion writes the run that just
    /// happened, and a stale `base/` left by an earlier run says nothing about
    /// this one. Counting it would report every second run as truncated.
    pub benchmarks_seen: usize,
    /// `benchmarks_seen` minus the measurements produced: benchmark
    /// directories that exist in the export but could not yield a measurement
    /// (typically a missing `sample.json`).
    pub benchmarks_skipped: usize,
    /// Raw samples across every emitted measurement.
    pub samples_total: usize,
    /// `benchmarks_skipped > 0`: the measurement set is a SUBSET of what the
    /// export knew about, not a summary of it. Carried as a single flag so the
    /// gateway can map it to a dataset-level completeness without re-deriving
    /// the arithmetic.
    pub truncated: bool,
}

/// Closed failure vocabulary. No variant carries project-controlled text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CriterionError {
    /// A zero-length archive: nothing was exported at all.
    Empty,
    /// The archive, or one member, exceeded its byte ceiling. Refused before
    /// the bytes are read, never truncated into a partial answer.
    TooLarge,
    /// A structural violation: USTAR framing, a hostile member path, a
    /// duplicate member, JSON that does not deserialize, an identity the
    /// domain refuses, a `benchmark.json` claiming a directory it does not
    /// live in, or a `full_id` repeated across directories.
    Malformed,
    /// More than [`MAX_CRITERION_BENCHMARKS`] benchmark directories.
    TooManyBenchmarks,
    /// The archive decoded, but no benchmark produced a measurement.
    NoMeasurement,
    /// A `sample.json` deserialized but its VALUES are not a believable sample
    /// set. Refuses the whole archive: a partially believable set is never
    /// silently pruned into a plausible one.
    InvalidSample,
}

/// `sample.json`. Criterion may add a field in a patch release and this crate
/// pins the harness version anyway, so the struct is deliberately NOT
/// `deny_unknown_fields`: an added field must not turn a valid measurement into
/// a refusal. Every field this module reads is typed explicitly instead, and no
/// `serde_json::Value` is ever built from project-controlled bytes.
#[derive(Deserialize)]
struct SampleFile {
    /// Absent in a future or foreign writer; absent maps to
    /// [`SamplingMode::Unknown`], never to a guess.
    sampling_mode: Option<String>,
    /// Iterations per batch. f64 because criterion writes it as one.
    iters: Vec<f64>,
    /// Nanoseconds for the WHOLE batch, not per iteration.
    times: Vec<f64>,
}

/// `benchmark.json`, i.e. criterion's serialized `BenchmarkId`. `throughput`,
/// `title` and anything criterion adds later are ignored on purpose (see
/// [`SampleFile`]).
#[derive(Deserialize)]
struct BenchmarkFile {
    group_id: String,
    function_id: Option<String>,
    value_str: Option<String>,
    full_id: String,
    directory_name: String,
}

/// The recognised members of one benchmark directory.
#[derive(Default)]
struct DirectoryFiles {
    sample: Option<Vec<u8>>,
    benchmark: Option<Vec<u8>>,
}

/// Reads one USTAR numeric field.
///
/// The guest's `tar` writer is not pinned by this contract, so both terminator
/// conventions in the wild are accepted — digits followed by NUL, or by a
/// space — provided every byte after the digit run is NUL or space. An empty
/// digit run, a non-octal digit, base-256 encoding (high bit set) and an
/// overflowing value are all refused: a size this parser cannot read exactly is
/// never guessed.
fn numeric_field(field: &[u8]) -> Result<u64, CriterionError> {
    let mut index = 0;
    while index < field.len() && field[index] == b' ' {
        index += 1;
    }
    let start = index;
    let mut value = 0u64;
    while index < field.len() && (b'0'..=b'7').contains(&field[index]) {
        value = value
            .checked_mul(8)
            .and_then(|value| value.checked_add(u64::from(field[index] - b'0')))
            .ok_or(CriterionError::Malformed)?;
        index += 1;
    }
    if index == start
        || field[index..]
            .iter()
            .any(|byte| *byte != 0 && *byte != b' ')
    {
        return Err(CriterionError::Malformed);
    }
    Ok(value)
}

/// The checksum the header claims.
fn stored_checksum(header: &[u8]) -> Result<u64, CriterionError> {
    numeric_field(&header[148..156])
}

/// The checksum the header's bytes imply, with the checksum field itself read
/// as eight spaces exactly as POSIX defines it.
fn computed_checksum(header: &[u8]) -> u64 {
    header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                u64::from(b' ')
            } else {
                u64::from(*byte)
            }
        })
        .sum()
}

/// A NUL-terminated USTAR text field. Bytes after the terminator must be NUL:
/// a name hiding a second string in its tail is refused, not trimmed.
fn strict_string(field: &[u8]) -> Result<&str, CriterionError> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    if field[end..].iter().any(|byte| *byte != 0) {
        return Err(CriterionError::Malformed);
    }
    std::str::from_utf8(&field[..end]).map_err(|_| CriterionError::Malformed)
}

/// Joins `prefix` and `name` into the member's declared path.
fn member_path(header: &[u8]) -> Result<String, CriterionError> {
    let name = strict_string(&header[..100])?;
    let prefix = strict_string(&header[345..500])?;
    if name.is_empty() {
        return Err(CriterionError::Malformed);
    }
    Ok(if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    })
}

/// Splits a member path into relative components, refusing everything that is
/// not a plain relative path: an absolute path, any `..`, an empty component
/// (`//`), a bare `.`, and a backslash — which is a directory separator on
/// another platform and has no business in a criterion directory name.
///
/// An empty result is the archive's own root entry (`./`), which is ignored.
fn components(raw: &str) -> Result<Vec<&str>, CriterionError> {
    if raw.starts_with('/') || raw.contains('\\') {
        return Err(CriterionError::Malformed);
    }
    let body = raw.strip_suffix('/').unwrap_or(raw);
    let body = body.strip_prefix("./").unwrap_or(body);
    if body.is_empty() || body == "." {
        return Ok(Vec::new());
    }
    let mut parts = Vec::new();
    for part in body.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(CriterionError::Malformed);
        }
        parts.push(part);
    }
    Ok(parts)
}

/// Maps criterion's reported mode. An unrecognised mode is UNKNOWN, never a
/// guess and never a failure: the mode is provenance, not a measurement.
fn sampling_mode(reported: Option<&str>) -> SamplingMode {
    match reported {
        Some("Linear") => SamplingMode::Linear,
        Some("Flat") => SamplingMode::Flat,
        Some("Auto") => SamplingMode::Auto,
        _ => SamplingMode::Unknown,
    }
}

/// One `iters` entry as an exact iteration count.
fn integral_iterations(value: f64) -> Result<u64, CriterionError> {
    if !value.is_finite() || value < 1.0 || value > MAX_EXACT_INTEGER || value.fract() != 0.0 {
        return Err(CriterionError::InvalidSample);
    }
    // Finite, integral, within [1, 2^53]: the conversion is exact.
    Ok(value as u64)
}

/// Decodes every recognised member of the export, keeping `sample.json` and
/// `benchmark.json` bytes per benchmark directory.
fn collect(archive: &[u8]) -> Result<BTreeMap<String, DirectoryFiles>, CriterionError> {
    let mut directories: BTreeMap<String, DirectoryFiles> = BTreeMap::new();
    let mut members = BTreeSet::<String>::new();
    let mut offset = 0usize;
    let mut ended = false;

    while offset < archive.len() {
        let end = offset.checked_add(BLOCK).ok_or(CriterionError::Malformed)?;
        let header = archive.get(offset..end).ok_or(CriterionError::Malformed)?;
        if header.iter().all(|byte| *byte == 0) {
            // The terminator is two zero blocks; a blocking factor pads with
            // more. Anything non-zero after it is a member hidden past the end.
            if archive[offset..].iter().any(|byte| *byte != 0) {
                return Err(CriterionError::Malformed);
            }
            ended = true;
            break;
        }
        if stored_checksum(header)? != computed_checksum(header) {
            return Err(CriterionError::Malformed);
        }
        let magic = &header[257..265];
        if magic != b"ustar\x0000" && magic != b"ustar  \0" {
            return Err(CriterionError::Malformed);
        }
        // Only regular files and directories. Every link, device, FIFO,
        // contiguous file and PAX/GNU extension header is refused: none of them
        // can carry a criterion document and each is a way to smuggle a second
        // meaning into a member name.
        let directory = match header[156] {
            b'0' | 0 => false,
            b'5' => true,
            _ => return Err(CriterionError::Malformed),
        };
        let size = usize::try_from(numeric_field(&header[124..136])?)
            .map_err(|_| CriterionError::Malformed)?;
        if directory && size != 0 {
            return Err(CriterionError::Malformed);
        }
        if size > MAX_CRITERION_FILE {
            return Err(CriterionError::TooLarge);
        }
        let data_end = end.checked_add(size).ok_or(CriterionError::Malformed)?;
        let padded_end = end
            .checked_add(size.div_ceil(BLOCK) * BLOCK)
            .ok_or(CriterionError::Malformed)?;
        let data = archive
            .get(end..data_end)
            .ok_or(CriterionError::Malformed)?;
        let padding = archive
            .get(data_end..padded_end)
            .ok_or(CriterionError::Malformed)?;
        if padding.iter().any(|byte| *byte != 0) {
            return Err(CriterionError::Malformed);
        }
        let raw = member_path(header)?;
        let parts = components(&raw)?;
        if !parts.is_empty() {
            if !members.insert(parts.join("/")) {
                return Err(CriterionError::Malformed);
            }
            if !directory {
                record(&mut directories, &parts, data)?;
            }
        }
        offset = padded_end;
    }
    if !ended {
        return Err(CriterionError::Malformed);
    }
    Ok(directories)
}

/// Files a single member. Anything that is not `<dir>/new/<one of the four>`
/// with a `<dir>` that has no separator of its own is ignored — a `base/`
/// directory, an HTML report, or a file the project planted must never become a
/// measurement — and ignoring it leaves every counter untouched.
fn record(
    directories: &mut BTreeMap<String, DirectoryFiles>,
    parts: &[&str],
    data: &[u8],
) -> Result<(), CriterionError> {
    let [directory_name, "new", file] = parts else {
        return Ok(());
    };
    if !matches!(
        *file,
        "sample.json" | "benchmark.json" | "estimates.json" | "tukey.json"
    ) {
        return Ok(());
    }
    if directories.len() >= MAX_CRITERION_BENCHMARKS && !directories.contains_key(*directory_name) {
        return Err(CriterionError::TooManyBenchmarks);
    }
    let entry = directories.entry((*directory_name).to_owned()).or_default();
    match *file {
        "sample.json" => entry.sample = Some(data.to_vec()),
        "benchmark.json" => entry.benchmark = Some(data.to_vec()),
        // `estimates.json` and `tukey.json` are recognised so the directory
        // counts as a benchmark directory, and are never decoded (see the
        // module documentation).
        _ => (),
    }
    Ok(())
}

/// Turns one `sample.json` into validated raw samples.
///
/// Every violation refuses the WHOLE archive. Dropping the offending samples
/// would leave a set that looks complete and is not, and dropping the whole
/// benchmark would let a project delete an inconvenient measurement by
/// corrupting one number.
fn raw_samples(sample: &SampleFile) -> Result<Vec<RawSample>, CriterionError> {
    if sample.iters.is_empty()
        || sample.iters.len() != sample.times.len()
        || sample.iters.len() > BENCHMARK_MAX_SAMPLES
    {
        return Err(CriterionError::InvalidSample);
    }
    let mut samples = Vec::with_capacity(sample.iters.len());
    for (iters, total_ns) in sample.iters.iter().zip(&sample.times) {
        let iterations = integral_iterations(*iters)?;
        if !total_ns.is_finite() || *total_ns <= 0.0 {
            return Err(CriterionError::InvalidSample);
        }
        samples.push(
            RawSample::new(iterations, *total_ns).map_err(|_| CriterionError::InvalidSample)?,
        );
    }
    Ok(samples)
}

/// Decodes the export into domain measurements.
///
/// `warm_up_ms`, `measurement_ms` and `sample_size_requested` are the values the
/// gateway PASSED to the harness. They are never read back from the project:
/// the request is the caller's own fact, and a project that could restate it
/// could make a short run look like a long one.
pub(crate) fn parse_archive(
    archive: &[u8],
    warm_up_ms: u64,
    measurement_ms: u64,
    sample_size_requested: u32,
) -> Result<CriterionParse, CriterionError> {
    if archive.is_empty() {
        return Err(CriterionError::Empty);
    }
    if archive.len() > MAX_CRITERION_ARCHIVE {
        return Err(CriterionError::TooLarge);
    }
    if !archive.len().is_multiple_of(BLOCK) {
        return Err(CriterionError::Malformed);
    }
    let directories = collect(archive)?;
    let benchmarks_seen = directories.len();
    let mut measurements = Vec::new();
    let mut full_ids = BTreeSet::<String>::new();
    let mut samples_total = 0usize;

    for (directory_name, files) in &directories {
        let (Some(sample_bytes), Some(benchmark_bytes)) = (&files.sample, &files.benchmark) else {
            // A directory without both documents is skipped and declared. It is
            // never emitted as a `Missing` measurement: that would invent a
            // record of a benchmark this export does not describe.
            continue;
        };
        let record: BenchmarkFile =
            serde_json::from_slice(benchmark_bytes).map_err(|_| CriterionError::Malformed)?;
        // The archive's own directory is the fact; `benchmark.json` only
        // claims one. A project could otherwise plant a `benchmark.json` under
        // its own directory claiming another benchmark's identity.
        if record.directory_name != *directory_name {
            return Err(CriterionError::Malformed);
        }
        let identity = BenchmarkIdentity::new(
            record.group_id,
            record.function_id,
            record.value_str,
            record.full_id,
            record.directory_name,
        )
        .map_err(|_| CriterionError::Malformed)?;
        if !full_ids.insert(identity.full_id().to_owned()) {
            return Err(CriterionError::Malformed);
        }
        let sample: SampleFile =
            serde_json::from_slice(sample_bytes).map_err(|_| CriterionError::Malformed)?;
        let samples = raw_samples(&sample)?;
        samples_total = samples_total
            .checked_add(samples.len())
            .ok_or(CriterionError::InvalidSample)?;
        measurements.push(
            BenchmarkMeasurement::new(
                identity,
                sampling_mode(sample.sampling_mode.as_deref()),
                samples,
                warm_up_ms,
                measurement_ms,
                sample_size_requested,
                // Every sample criterion wrote for this benchmark is present:
                // a shortfall against `sample_size_requested` is visible next
                // to the samples and is not truncation.
                MeasurementCompleteness::Complete,
            )
            .map_err(|_| CriterionError::InvalidSample)?,
        );
    }
    if measurements.is_empty() {
        return Err(CriterionError::NoMeasurement);
    }
    measurements.sort_by(|left, right| left.key().cmp(right.key()));
    let benchmarks_skipped = benchmarks_seen.saturating_sub(measurements.len());
    Ok(CriterionParse {
        measurements,
        benchmarks_seen,
        benchmarks_skipped,
        samples_total,
        truncated: benchmarks_skipped > 0,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Hand-built fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;

    fn octal_into(field: &mut [u8], value: usize) {
        let text = format!("{value:0width$o}", width = field.len() - 1);
        assert!(
            text.len() < field.len(),
            "fixture value overflows its field"
        );
        field[..text.len()].copy_from_slice(text.as_bytes());
    }

    fn seal(header: &mut [u8]) {
        header[148..156].fill(b' ');
        let sum: usize = header.iter().map(|byte| usize::from(*byte)).sum();
        octal_into(&mut header[148..155], sum);
        header[154] = 0;
        header[155] = b' ';
    }

    fn header_block(path: &str, size: usize, flag: u8) -> Vec<u8> {
        assert!(path.len() <= 100, "fixture path needs the prefix field");
        let mut header = vec![0u8; BLOCK];
        header[..path.len()].copy_from_slice(path.as_bytes());
        octal_into(&mut header[100..108], 0o644);
        octal_into(&mut header[108..116], 0);
        octal_into(&mut header[116..124], 0);
        octal_into(&mut header[124..136], size);
        octal_into(&mut header[136..148], 0);
        header[156] = flag;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        seal(&mut header);
        header
    }

    fn push(output: &mut Vec<u8>, path: &str, data: &[u8], flag: u8) {
        output.extend_from_slice(&header_block(path, data.len(), flag));
        output.extend_from_slice(data);
        output.resize(
            output.len() + data.len().div_ceil(BLOCK) * BLOCK - data.len(),
            0,
        );
    }

    fn terminate(mut output: Vec<u8>) -> Vec<u8> {
        output.resize(output.len() + 2 * BLOCK, 0);
        output
    }

    fn archive(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut output = Vec::new();
        for (path, data) in members {
            push(&mut output, path, data, b'0');
        }
        terminate(output)
    }

    fn sample_json(mode: &str, iters: &str, times: &str) -> String {
        format!(r#"{{"sampling_mode":"{mode}","iters":[{iters}],"times":[{times}]}}"#)
    }

    fn benchmark_json(group: &str, full: &str, directory: &str) -> String {
        format!(
            r#"{{"group_id":"{group}","function_id":"bench","value_str":null,"throughput":null,"full_id":"{full}","directory_name":"{directory}","title":"{full}"}}"#
        )
    }

    /// A two-benchmark export written in reverse order, so ordering cannot come
    /// from the archive.
    fn two_benchmarks() -> Vec<u8> {
        let beta_sample = sample_json("Flat", "10.0, 10.0", "5000.0, 6000.0");
        let beta_id = benchmark_json("beta", "beta/two", "beta");
        let alpha_sample = sample_json("Linear", "1.0, 2.0, 4.0", "100.0, 210.0, 440.0");
        let alpha_id = benchmark_json("alpha", "alpha/one", "alpha");
        archive(&[
            ("beta/new/sample.json", beta_sample.as_bytes()),
            ("beta/new/benchmark.json", beta_id.as_bytes()),
            ("alpha/new/sample.json", alpha_sample.as_bytes()),
            ("alpha/new/benchmark.json", alpha_id.as_bytes()),
        ])
    }

    #[test]
    fn a_two_benchmark_export_yields_exact_per_iteration_values() -> Result<(), CriterionError> {
        let parsed = parse_archive(&two_benchmarks(), 3_000, 5_000, 100)?;
        assert_eq!(parsed.benchmarks_seen, 2);
        assert_eq!(parsed.benchmarks_skipped, 0);
        assert!(!parsed.truncated);
        assert_eq!(parsed.samples_total, 5);
        assert_eq!(parsed.measurements.len(), 2);

        let alpha = parsed
            .measurements
            .first()
            .ok_or(CriterionError::NoMeasurement)?;
        assert_eq!(alpha.key(), "alpha/one");
        assert_eq!(alpha.identity().group_id(), "alpha");
        assert_eq!(alpha.identity().function_id(), Some("bench"));
        assert_eq!(alpha.identity().value_str(), None);
        assert_eq!(alpha.identity().directory_name(), "alpha");
        assert_eq!(alpha.sampling_mode(), SamplingMode::Linear);
        assert_eq!(alpha.completeness(), MeasurementCompleteness::Complete);
        assert_eq!(alpha.warm_up_ms(), 3_000);
        assert_eq!(alpha.measurement_ms(), 5_000);
        assert_eq!(alpha.sample_size_requested(), 100);
        let per_iteration = alpha
            .samples()
            .iter()
            .map(RawSample::per_iteration_ns)
            .collect::<Vec<_>>();
        // times[i] is the whole batch: 100/1, 210/2, 440/4.
        assert_eq!(per_iteration, vec![100.0, 105.0, 110.0]);
        assert_eq!(
            alpha
                .samples()
                .iter()
                .map(RawSample::iterations)
                .collect::<Vec<_>>(),
            vec![1, 2, 4]
        );

        let beta = parsed
            .measurements
            .get(1)
            .ok_or(CriterionError::NoMeasurement)?;
        assert_eq!(beta.key(), "beta/two");
        assert_eq!(beta.sampling_mode(), SamplingMode::Flat);
        assert_eq!(
            beta.samples()
                .iter()
                .map(RawSample::per_iteration_ns)
                .collect::<Vec<_>>(),
            vec![500.0, 600.0]
        );
        Ok(())
    }

    #[test]
    fn ordering_is_a_function_of_the_full_id_not_of_member_order() -> Result<(), CriterionError> {
        let forward = parse_archive(&two_benchmarks(), 1, 1, 1)?;
        let alpha_sample = sample_json("Linear", "1.0, 2.0, 4.0", "100.0, 210.0, 440.0");
        let alpha_id = benchmark_json("alpha", "alpha/one", "alpha");
        let beta_sample = sample_json("Flat", "10.0, 10.0", "5000.0, 6000.0");
        let beta_id = benchmark_json("beta", "beta/two", "beta");
        let reordered = archive(&[
            ("alpha/new/benchmark.json", alpha_id.as_bytes()),
            ("beta/new/benchmark.json", beta_id.as_bytes()),
            ("alpha/new/sample.json", alpha_sample.as_bytes()),
            ("beta/new/sample.json", beta_sample.as_bytes()),
        ]);
        let backward = parse_archive(&reordered, 1, 1, 1)?;
        assert_eq!(
            forward
                .measurements
                .iter()
                .map(BenchmarkMeasurement::key)
                .collect::<Vec<_>>(),
            ["alpha/one", "beta/two"]
        );
        assert_eq!(forward.measurements, backward.measurements);
        Ok(())
    }

    #[test]
    fn a_benchmark_without_sample_json_is_skipped_and_declared() -> Result<(), CriterionError> {
        let alpha_sample = sample_json("Linear", "1.0", "100.0");
        let alpha_id = benchmark_json("alpha", "alpha/one", "alpha");
        let beta_id = benchmark_json("beta", "beta/two", "beta");
        let parsed = parse_archive(
            &archive(&[
                ("alpha/new/sample.json", alpha_sample.as_bytes()),
                ("alpha/new/benchmark.json", alpha_id.as_bytes()),
                ("beta/new/benchmark.json", beta_id.as_bytes()),
                ("beta/new/estimates.json", br#"{"mean":{}}"#),
            ]),
            1,
            1,
            1,
        )?;
        assert_eq!(parsed.benchmarks_seen, 2);
        assert_eq!(parsed.benchmarks_skipped, 1);
        assert!(parsed.truncated);
        assert_eq!(parsed.measurements.len(), 1);
        // No `Missing` measurement is invented for the skipped benchmark.
        assert_eq!(
            parsed
                .measurements
                .first()
                .map(BenchmarkMeasurement::completeness),
            Some(MeasurementCompleteness::Complete)
        );

        // A directory recognised only through `estimates.json`/`tukey.json` is
        // counted and produces nothing at all.
        assert_eq!(
            parse_archive(
                &archive(&[
                    ("solo/new/estimates.json", br#"{"mean":{}}"#),
                    ("solo/new/tukey.json", b"[1.0,2.0,3.0,4.0]"),
                ]),
                1,
                1,
                1,
            )
            .err(),
            Some(CriterionError::NoMeasurement)
        );
        Ok(())
    }

    #[test]
    fn a_directory_name_that_disagrees_with_benchmark_json_is_malformed() {
        let sample = sample_json("Linear", "1.0", "100.0");
        // The document claims to be another benchmark's identity.
        let planted = benchmark_json("alpha", "alpha/one", "alpha");
        assert_eq!(
            parse_archive(
                &archive(&[
                    ("gamma/new/sample.json", sample.as_bytes()),
                    ("gamma/new/benchmark.json", planted.as_bytes()),
                ]),
                1,
                1,
                1,
            )
            .err(),
            Some(CriterionError::Malformed)
        );
    }

    #[test]
    fn a_full_id_repeated_across_directories_is_malformed() {
        let sample = sample_json("Linear", "1.0", "100.0");
        let first = benchmark_json("group", "group/bench", "first");
        let second = benchmark_json("group", "group/bench", "second");
        assert_eq!(
            parse_archive(
                &archive(&[
                    ("first/new/sample.json", sample.as_bytes()),
                    ("first/new/benchmark.json", first.as_bytes()),
                    ("second/new/sample.json", sample.as_bytes()),
                    ("second/new/benchmark.json", second.as_bytes()),
                ]),
                1,
                1,
                1,
            )
            .err(),
            Some(CriterionError::Malformed)
        );
    }

    #[test]
    fn hostile_member_paths_and_types_are_rejected() {
        let sample = sample_json("Linear", "1.0", "100.0");
        let identity = benchmark_json("alpha", "alpha/one", "alpha");
        for path in [
            "/alpha/new/sample.json",
            "alpha/../../etc/passwd",
            "alpha\\new\\sample.json",
            "alpha//new/sample.json",
            "alpha/./new/sample.json",
        ] {
            let mut output = Vec::new();
            push(&mut output, path, sample.as_bytes(), b'0');
            push(
                &mut output,
                "alpha/new/benchmark.json",
                identity.as_bytes(),
                b'0',
            );
            assert_eq!(
                parse_archive(&terminate(output), 1, 1, 1).err(),
                Some(CriterionError::Malformed),
                "{path}"
            );
        }
        // Hard link, symlink, character device, block device, FIFO, contiguous
        // file and the PAX/GNU extension headers.
        for flag in *b"123467xgLK" {
            let mut output = Vec::new();
            push(&mut output, "alpha/new/sample.json", b"", flag);
            assert_eq!(
                parse_archive(&terminate(output), 1, 1, 1).err(),
                Some(CriterionError::Malformed),
                "type flag {flag}"
            );
        }
    }

    #[test]
    fn framing_checksum_octal_and_duplicate_violations_are_rejected() {
        let sample = sample_json("Linear", "1.0", "100.0");
        let identity = benchmark_json("alpha", "alpha/one", "alpha");
        let valid = archive(&[
            ("alpha/new/sample.json", sample.as_bytes()),
            ("alpha/new/benchmark.json", identity.as_bytes()),
        ]);
        assert!(parse_archive(&valid, 1, 1, 1).is_ok());

        // Not a multiple of the block size.
        let mut short = valid.clone();
        short.truncate(short.len() - 1);
        assert_eq!(
            parse_archive(&short, 1, 1, 1).err(),
            Some(CriterionError::Malformed)
        );

        // No end-of-archive terminator.
        let mut unterminated = valid.clone();
        unterminated.truncate(unterminated.len() - 2 * BLOCK);
        assert_eq!(
            parse_archive(&unterminated, 1, 1, 1).err(),
            Some(CriterionError::Malformed)
        );

        // A member hidden after the terminator.
        let mut after_end = valid.clone();
        if let Some(last) = after_end.last_mut() {
            *last = 1;
        }
        assert_eq!(
            parse_archive(&after_end, 1, 1, 1).err(),
            Some(CriterionError::Malformed)
        );

        // A byte flipped in the first header without resealing it.
        let mut bad_checksum = valid.clone();
        bad_checksum[0] ^= 1;
        assert_eq!(
            parse_archive(&bad_checksum, 1, 1, 1).err(),
            Some(CriterionError::Malformed)
        );

        // A non-octal digit in the size field, resealed so only the size is bad.
        let mut bad_octal = valid.clone();
        bad_octal[124] = b'9';
        seal(&mut bad_octal[..BLOCK]);
        assert_eq!(
            parse_archive(&bad_octal, 1, 1, 1).err(),
            Some(CriterionError::Malformed)
        );

        // Non-zero padding after a member's data.
        let mut dirty_padding = valid.clone();
        let tail = BLOCK + sample.len();
        dirty_padding[tail] = 1;
        assert_eq!(
            parse_archive(&dirty_padding, 1, 1, 1).err(),
            Some(CriterionError::Malformed)
        );

        // The same member declared twice.
        let duplicate = archive(&[
            ("alpha/new/sample.json", sample.as_bytes()),
            ("alpha/new/benchmark.json", identity.as_bytes()),
            ("alpha/new/sample.json", sample.as_bytes()),
        ]);
        assert_eq!(
            parse_archive(&duplicate, 1, 1, 1).err(),
            Some(CriterionError::Malformed)
        );
    }

    #[test]
    fn byte_ceilings_are_refused_before_the_bytes_are_read() {
        assert_eq!(
            parse_archive(&[], 1, 1, 1).err(),
            Some(CriterionError::Empty)
        );
        assert_eq!(
            parse_archive(&vec![0u8; MAX_CRITERION_ARCHIVE + BLOCK], 1, 1, 1).err(),
            Some(CriterionError::TooLarge)
        );
        // A header claiming a member above the per-file ceiling is refused
        // without ever addressing that many bytes.
        let mut oversize = Vec::new();
        oversize.extend_from_slice(&header_block(
            "alpha/new/sample.json",
            MAX_CRITERION_FILE + 1,
            b'0',
        ));
        assert_eq!(
            parse_archive(&terminate(oversize), 1, 1, 1).err(),
            Some(CriterionError::TooLarge)
        );
        // Exactly at the archive ceiling the length check passes and the
        // content decides; an all-zero archive carries no benchmark.
        assert_eq!(
            parse_archive(&vec![0u8; MAX_CRITERION_ARCHIVE], 1, 1, 1).err(),
            Some(CriterionError::NoMeasurement)
        );
    }

    #[test]
    fn an_unbelievable_sample_set_refuses_the_whole_archive() {
        let identity = benchmark_json("alpha", "alpha/one", "alpha");
        let many = (0..=BENCHMARK_MAX_SAMPLES)
            .map(|_| "1.0")
            .collect::<Vec<_>>()
            .join(",");
        let hostile = [
            // A non-positive or negative duration.
            sample_json("Linear", "1.0", "-100.0"),
            sample_json("Linear", "1.0", "0.0"),
            // Zero, fractional or negative iterations.
            sample_json("Linear", "0.0", "100.0"),
            sample_json("Linear", "1.5", "100.0"),
            sample_json("Linear", "-1.0", "100.0"),
            // Mismatched lengths, in both directions, and empty arrays.
            sample_json("Linear", "1.0, 2.0", "100.0"),
            sample_json("Linear", "1.0", "100.0, 200.0"),
            sample_json("Linear", "", ""),
            // Beyond the domain's sample ceiling.
            sample_json("Linear", &many, &many),
            // A duration beyond the domain's bounded-duration ceiling.
            sample_json("Linear", "1.0", "1e16"),
        ];
        for sample in hostile {
            let built = archive(&[
                ("alpha/new/sample.json", sample.as_bytes()),
                ("alpha/new/benchmark.json", identity.as_bytes()),
            ]);
            assert_eq!(
                parse_archive(&built, 1, 1, 1).err(),
                Some(CriterionError::InvalidSample),
                "{sample:.64}"
            );
        }
        // JSON that does not deserialize at all is a structural failure, not a
        // value one. `NaN`, `Infinity` and a magnitude f64 cannot hold are all
        // in this group: JSON has no literal for a non-finite number and
        // `serde_json` refuses `1e400` outright, so a non-finite sample can
        // never reach the value checks through this path.
        for broken in [
            &b"{"[..],
            br#"{"iters":[1.0]}"#,
            br#"{"iters":["1"],"times":[1.0]}"#,
            br#"{"iters":[NaN],"times":[1.0]}"#,
            br#"{"iters":[1.0],"times":[Infinity]}"#,
            br#"{"iters":[1.0],"times":[1e400]}"#,
        ] {
            let built = archive(&[
                ("alpha/new/sample.json", broken),
                ("alpha/new/benchmark.json", identity.as_bytes()),
            ]);
            assert_eq!(
                parse_archive(&built, 1, 1, 1).err(),
                Some(CriterionError::Malformed)
            );
        }
    }

    /// The finiteness guards cannot be reached through JSON (see above), so
    /// they are exercised directly: a future reader of this artifact must not
    /// be able to introduce a non-finite sample by changing the decoder alone.
    #[test]
    fn non_finite_values_are_refused_by_the_sample_guards() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                integral_iterations(value).err(),
                Some(CriterionError::InvalidSample),
                "{value}"
            );
            assert_eq!(
                raw_samples(&SampleFile {
                    sampling_mode: None,
                    iters: vec![1.0],
                    times: vec![value],
                })
                .err(),
                Some(CriterionError::InvalidSample),
                "{value}"
            );
        }
        // Beyond f64's exact integer range an iteration count is no longer a
        // count, so it is refused rather than rounded.
        assert_eq!(
            integral_iterations(MAX_EXACT_INTEGER * 2.0).err(),
            Some(CriterionError::InvalidSample)
        );
        assert_eq!(integral_iterations(MAX_EXACT_INTEGER), Ok(1 << 53));
        assert_eq!(integral_iterations(1.0), Ok(1));
    }

    #[test]
    fn an_unrecognised_sampling_mode_stays_unknown() -> Result<(), CriterionError> {
        let identity = benchmark_json("alpha", "alpha/one", "alpha");
        for (mode, expected) in [
            ("Linear", SamplingMode::Linear),
            ("Flat", SamplingMode::Flat),
            ("Auto", SamplingMode::Auto),
            ("Quadratic", SamplingMode::Unknown),
            ("linear", SamplingMode::Unknown),
        ] {
            let sample = sample_json(mode, "1.0", "100.0");
            let parsed = parse_archive(
                &archive(&[
                    ("alpha/new/sample.json", sample.as_bytes()),
                    ("alpha/new/benchmark.json", identity.as_bytes()),
                ]),
                1,
                1,
                1,
            )?;
            assert_eq!(
                parsed
                    .measurements
                    .first()
                    .map(BenchmarkMeasurement::sampling_mode),
                Some(expected),
                "{mode}"
            );
        }
        // An absent mode is unknown, never a guess and never a failure.
        let parsed = parse_archive(
            &archive(&[
                (
                    "alpha/new/sample.json",
                    br#"{"iters":[1.0],"times":[100.0]}"#,
                ),
                ("alpha/new/benchmark.json", identity.as_bytes()),
            ]),
            1,
            1,
            1,
        )?;
        assert_eq!(
            parsed
                .measurements
                .first()
                .map(BenchmarkMeasurement::sampling_mode),
            Some(SamplingMode::Unknown)
        );
        Ok(())
    }

    #[test]
    fn base_report_and_planted_members_are_ignored_never_measured() -> Result<(), CriterionError> {
        let sample = sample_json("Linear", "1.0", "100.0");
        let identity = benchmark_json("alpha", "alpha/one", "alpha");
        // A `base/` tree from an earlier run, criterion's HTML report at both
        // levels, a stray file the project planted inside `new/`, a root file,
        // and a deeper path that only looks like a benchmark.
        let mut output = Vec::new();
        push(&mut output, "alpha/", b"", b'5');
        push(&mut output, "alpha/new/", b"", b'5');
        push(
            &mut output,
            "alpha/new/sample.json",
            sample.as_bytes(),
            b'0',
        );
        push(
            &mut output,
            "alpha/new/benchmark.json",
            identity.as_bytes(),
            b'0',
        );
        push(&mut output, "alpha/new/planted.json", b"{}", b'0');
        push(
            &mut output,
            "alpha/base/sample.json",
            sample.as_bytes(),
            b'0',
        );
        push(
            &mut output,
            "alpha/base/benchmark.json",
            identity.as_bytes(),
            b'0',
        );
        push(&mut output, "alpha/report/index.html", b"<p>x</p>", b'0');
        push(&mut output, "report/index.html", b"<p>x</p>", b'0');
        push(&mut output, "planted.json", b"{}", b'0');
        push(
            &mut output,
            "group/inner/new/sample.json",
            sample.as_bytes(),
            b'0',
        );
        let parsed = parse_archive(&terminate(output), 1, 1, 1)?;
        assert_eq!(parsed.benchmarks_seen, 1);
        assert_eq!(parsed.benchmarks_skipped, 0);
        assert!(!parsed.truncated);
        assert_eq!(parsed.measurements.len(), 1);
        assert_eq!(parsed.samples_total, 1);

        // An export whose only content is a stale `base/` tree and a report
        // describes no run at all.
        let stale = archive(&[
            ("alpha/base/sample.json", sample.as_bytes()),
            ("alpha/base/benchmark.json", identity.as_bytes()),
            ("report/index.html", b"<p>x</p>"),
        ]);
        assert_eq!(
            parse_archive(&stale, 1, 1, 1).err(),
            Some(CriterionError::NoMeasurement)
        );
        Ok(())
    }

    #[test]
    fn more_benchmark_directories_than_the_ceiling_are_rejected() {
        let sample = sample_json("Linear", "1.0", "100.0");
        let mut output = Vec::new();
        for index in 0..=MAX_CRITERION_BENCHMARKS {
            push(
                &mut output,
                &format!("bench{index}/new/sample.json"),
                sample.as_bytes(),
                b'0',
            );
        }
        assert_eq!(
            parse_archive(&terminate(output), 1, 1, 1).err(),
            Some(CriterionError::TooManyBenchmarks)
        );

        // Exactly the ceiling decodes; none of them carries `benchmark.json`,
        // so the export still yields no measurement.
        let mut at_ceiling = Vec::new();
        for index in 0..MAX_CRITERION_BENCHMARKS {
            push(
                &mut at_ceiling,
                &format!("bench{index}/new/sample.json"),
                sample.as_bytes(),
                b'0',
            );
        }
        assert_eq!(
            parse_archive(&terminate(at_ceiling), 1, 1, 1).err(),
            Some(CriterionError::NoMeasurement)
        );
    }

    #[test]
    fn an_identity_the_domain_refuses_is_malformed() {
        let sample = sample_json("Linear", "1.0", "100.0");
        for identity in [
            // Empty required field.
            r#"{"group_id":"","function_id":null,"value_str":null,"full_id":"alpha/one","directory_name":"alpha"}"#,
            r#"{"group_id":"alpha","function_id":null,"value_str":null,"full_id":"","directory_name":"alpha"}"#,
            // A control character in free text.
            r#"{"group_id":"al\npha","function_id":null,"value_str":null,"full_id":"alpha/one","directory_name":"alpha"}"#,
            // A missing required field.
            r#"{"group_id":"alpha","full_id":"alpha/one"}"#,
        ] {
            let built = archive(&[
                ("alpha/new/sample.json", sample.as_bytes()),
                ("alpha/new/benchmark.json", identity.as_bytes()),
            ]);
            assert_eq!(
                parse_archive(&built, 1, 1, 1).err(),
                Some(CriterionError::Malformed),
                "{identity}"
            );
        }
    }
}
