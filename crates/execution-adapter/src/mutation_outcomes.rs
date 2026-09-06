//! Bounded readers for the only M3-05 oracles: the machine-written files under
//! `mutants.out`.
//!
//! Nothing here reads guest stdout/stderr. Human-readable tool output — including
//! the forged `mutants.out: caught ...` lines the hostile fixture prints — can
//! never reach these functions, and every reader below is a total function from
//! bytes to either a validated value or "invalid". Sizes, nesting depth and
//! element counts are all bounded before any allocation proportional to the
//! input, so a hostile report cannot exhaust host memory or stack.

use rust_engineering_domain::mutation_test::{
    MUTATION_MAX_ROW_NAME, MUTATION_MAX_ROWS, MUTATION_MAX_VERSION, MutationBaseline,
    MutationCounts, MutationGuestIdentity, MutationMutantRow, MutationOutcomeClass,
};
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use std::fmt;

/// `outcomes.json` for the capped mutant set. One outcome record is a few
/// hundred bytes, so this is roughly two orders of magnitude of headroom.
pub(super) const MAX_OUTCOMES_JSON: usize = 1024 * 1024;
/// One `caught.txt`/`missed.txt`/`timeout.txt`/`unviable.txt` list.
pub(super) const MAX_LIST_FILE: usize = 256 * 1024;
/// `lock.json` holds a start time, version, username and hostname only.
pub(super) const MAX_LOCK_JSON: usize = 16 * 1024;
/// Scenario records accepted from `outcomes.json` (baseline plus mutants).
pub(super) const MAX_OUTCOME_RECORDS: usize = 4096;
/// Lines accepted from one list file.
pub(super) const MAX_LIST_LINES: usize = 4096;
/// Maximum JSON nesting accepted anywhere in guest evidence. cargo-mutants'
/// own records nest a handful of levels; this is a fail-closed ceiling well
/// below Serde's own recursion limit.
const MAX_JSON_DEPTH: u32 = 16;
/// Elements accepted inside any single skipped array or object.
const MAX_SKIPPED_ELEMENTS: usize = 8192;
/// Bound for any single string read out of guest JSON.
const MAX_JSON_STRING: usize = 4096;

/// The fixed guest hostname (`--hostname=sandbox` in every generated container).
const GUEST_HOSTNAME: &str = "sandbox";
/// Usernames the sandbox can legitimately report for uid 65534. Anything else
/// is treated as host shaped and redacted.
const GUEST_USERNAMES: &[&str] = &["nobody", "nfsnobody", "unknown", "65534", ""];

/// Depth- and count-bounded skipper for JSON values this product does not
/// interpret. Unknown fields are tolerated (the `mutants.out` format is
/// explicitly documented as subject to change) but never unbounded.
struct Skip {
    depth: u32,
}

impl<'de> DeserializeSeed<'de> for Skip {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Skip {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded JSON value")
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
        if value.len() > MAX_JSON_STRING {
            return Err(E::custom("oversized string"));
        }
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let Some(depth) = self.depth.checked_sub(1) else {
            return Err(de::Error::custom("nesting limit"));
        };
        let mut count = 0usize;
        while seq.next_element_seed(Skip { depth })?.is_some() {
            count += 1;
            if count > MAX_SKIPPED_ELEMENTS {
                return Err(de::Error::custom("element limit"));
            }
        }
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let Some(depth) = self.depth.checked_sub(1) else {
            return Err(de::Error::custom("nesting limit"));
        };
        let mut count = 0usize;
        while map.next_key_seed(BoundedKey)?.is_some() {
            map.next_value_seed(Skip { depth })?;
            count += 1;
            if count > MAX_SKIPPED_ELEMENTS {
                return Err(de::Error::custom("element limit"));
            }
        }
        Ok(())
    }
}

/// Object keys are read into a bounded owned string; a hostile report cannot
/// force an unbounded key allocation.
struct BoundedKey;
impl<'de> DeserializeSeed<'de> for BoundedKey {
    type Value = String;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_str(self)
    }
}
impl<'de> Visitor<'de> for BoundedKey {
    type Value = String;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded object key")
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<String, E> {
        if value.len() > MAX_JSON_STRING {
            return Err(E::custom("oversized key"));
        }
        Ok(value.to_owned())
    }
}

/// Bounded string value.
struct BoundedString(usize);
impl<'de> DeserializeSeed<'de> for BoundedString {
    type Value = String;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_str(self)
    }
}
impl<'de> Visitor<'de> for BoundedString {
    type Value = String;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded string")
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<String, E> {
        if value.len() > self.0 {
            return Err(E::custom("oversized string"));
        }
        Ok(value.to_owned())
    }
}

/// One scenario record: its class and whether it is the mandatory baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Record {
    class: MutationOutcomeClass,
    baseline: bool,
}

struct RecordSeed;
impl<'de> DeserializeSeed<'de> for RecordSeed {
    type Value = Record;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Record, D::Error> {
        deserializer.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for RecordSeed {
    type Value = Record;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a cargo-mutants outcome record")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Record, A::Error> {
        let mut class = None;
        let mut baseline = None;
        let mut fields = 0usize;
        while let Some(key) = map.next_key_seed(BoundedKey)? {
            fields += 1;
            if fields > MAX_SKIPPED_ELEMENTS {
                return Err(de::Error::custom("element limit"));
            }
            match key.as_str() {
                "summary" => {
                    let text = map.next_value_seed(BoundedString(MAX_JSON_STRING))?;
                    let parsed = MutationOutcomeClass::parse(&text)
                        .ok_or_else(|| de::Error::custom("unknown outcome summary"))?;
                    if class.replace(parsed).is_some() {
                        return Err(de::Error::custom("duplicate summary"));
                    }
                }
                "scenario" => {
                    let observed = map.next_value_seed(ScenarioSeed)?;
                    if baseline.replace(observed).is_some() {
                        return Err(de::Error::custom("duplicate scenario"));
                    }
                }
                _ => map.next_value_seed(Skip {
                    depth: MAX_JSON_DEPTH,
                })?,
            }
        }
        Ok(Record {
            class: class.ok_or_else(|| de::Error::custom("missing summary"))?,
            baseline: baseline.ok_or_else(|| de::Error::custom("missing scenario"))?,
        })
    }
}

/// `scenario` is either the bare string `"Baseline"` or an externally tagged
/// object whose single key names the variant. Both spellings are accepted; any
/// other shape is rejected rather than guessed.
struct ScenarioSeed;
impl<'de> DeserializeSeed<'de> for ScenarioSeed {
    type Value = bool;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<bool, D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for ScenarioSeed {
    type Value = bool;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a cargo-mutants scenario")
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<bool, E> {
        match value {
            "Baseline" => Ok(true),
            "Mutant" => Ok(false),
            _ => Err(E::custom("unknown scenario")),
        }
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<bool, A::Error> {
        let key = map
            .next_key_seed(BoundedKey)?
            .ok_or_else(|| de::Error::custom("empty scenario"))?;
        map.next_value_seed(Skip {
            depth: MAX_JSON_DEPTH - 1,
        })?;
        if map.next_key_seed(BoundedKey)?.is_some() {
            return Err(de::Error::custom("ambiguous scenario"));
        }
        match key.as_str() {
            "Baseline" => Ok(true),
            "Mutant" => Ok(false),
            _ => Err(de::Error::custom("unknown scenario")),
        }
    }
}

/// Validated `outcomes.json` facts. `counts.generated` is deliberately left at
/// zero here: the denominator comes from the separate listing pass, which runs
/// no project test code, and is filled in by the gateway.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ParsedOutcomes {
    pub(super) version: String,
    pub(super) baseline: MutationBaseline,
    pub(super) counts: MutationCounts,
    /// Top-level per-class totals when the report carries them. They are a
    /// cross-check, never the primary source.
    pub(super) declared: Option<DeclaredCounts>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct DeclaredCounts {
    pub(super) caught: u32,
    pub(super) missed: u32,
    pub(super) timeout: u32,
    pub(super) unviable: u32,
}

struct OutcomesSeed;
impl<'de> DeserializeSeed<'de> for OutcomesSeed {
    type Value = ParsedOutcomes;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<ParsedOutcomes, D::Error> {
        deserializer.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for OutcomesSeed {
    type Value = ParsedOutcomes;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a cargo-mutants outcomes.json document")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ParsedOutcomes, A::Error> {
        let mut version: Option<String> = None;
        let mut records: Option<Vec<Record>> = None;
        let mut caught: Option<u32> = None;
        let mut missed: Option<u32> = None;
        let mut timeout: Option<u32> = None;
        let mut unviable: Option<u32> = None;
        let mut fields = 0usize;
        while let Some(key) = map.next_key_seed(BoundedKey)? {
            fields += 1;
            if fields > MAX_SKIPPED_ELEMENTS {
                return Err(de::Error::custom("element limit"));
            }
            match key.as_str() {
                "cargo_mutants_version" => {
                    version = Some(map.next_value_seed(BoundedString(MAX_JSON_STRING))?);
                }
                "outcomes" => {
                    if records.replace(map.next_value_seed(RecordsSeed)?).is_some() {
                        return Err(de::Error::custom("duplicate outcomes"));
                    }
                }
                "caught" => caught = Some(map.next_value()?),
                "missed" => missed = Some(map.next_value()?),
                "timeout" => timeout = Some(map.next_value()?),
                "unviable" => unviable = Some(map.next_value()?),
                _ => map.next_value_seed(Skip {
                    depth: MAX_JSON_DEPTH,
                })?,
            }
        }
        let records = records.ok_or_else(|| de::Error::custom("missing outcomes"))?;
        let version = version.ok_or_else(|| de::Error::custom("missing version"))?;
        if version.is_empty() || version.len() > MUTATION_MAX_VERSION {
            return Err(de::Error::custom("invalid version"));
        }
        let mut counts = MutationCounts::default();
        let mut baseline = MutationBaseline::Missing;
        for record in records {
            if record.baseline {
                if baseline != MutationBaseline::Missing {
                    return Err(de::Error::custom("duplicate baseline"));
                }
                baseline = if record.class == MutationOutcomeClass::Success {
                    MutationBaseline::Passed
                } else {
                    MutationBaseline::Failed
                };
                continue;
            }
            counts.tested = counts
                .tested
                .checked_add(1)
                .ok_or_else(|| de::Error::custom("count overflow"))?;
            let slot = match record.class {
                MutationOutcomeClass::Caught => &mut counts.caught,
                MutationOutcomeClass::Missed => &mut counts.missed,
                MutationOutcomeClass::Timeout => &mut counts.timeout,
                MutationOutcomeClass::Unviable => &mut counts.unviable,
                MutationOutcomeClass::Success | MutationOutcomeClass::Failure => &mut counts.other,
            };
            *slot = slot
                .checked_add(1)
                .ok_or_else(|| de::Error::custom("count overflow"))?;
        }
        let declared = match (caught, missed, timeout, unviable) {
            (Some(caught), Some(missed), Some(timeout), Some(unviable)) => Some(DeclaredCounts {
                caught,
                missed,
                timeout,
                unviable,
            }),
            _ => None,
        };
        Ok(ParsedOutcomes {
            version,
            baseline,
            counts,
            declared,
        })
    }
}

struct RecordsSeed;
impl<'de> DeserializeSeed<'de> for RecordsSeed {
    type Value = Vec<Record>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Vec<Record>, D::Error> {
        deserializer.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for RecordsSeed {
    type Value = Vec<Record>;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded array of outcome records")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<Record>, A::Error> {
        let mut records = Vec::new();
        while let Some(record) = seq.next_element_seed(RecordSeed)? {
            if records.len() >= MAX_OUTCOME_RECORDS {
                return Err(de::Error::custom("outcome record limit"));
            }
            records.push(record);
        }
        Ok(records)
    }
}

/// Read `outcomes.json`. Any size, syntax, vocabulary or structural problem is
/// reported as `None`: uncertainty is never resolved in the project's favour.
pub(super) fn parse_outcomes(bytes: &[u8]) -> Option<ParsedOutcomes> {
    if bytes.is_empty() || bytes.len() > MAX_OUTCOMES_JSON {
        return None;
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let parsed = OutcomesSeed.deserialize(&mut deserializer).ok()?;
    deserializer.end().ok()?;
    Some(parsed)
}

/// Count the elements of the listing pass' JSON array without materializing
/// them. `limit` is the largest count worth distinguishing; a longer array
/// reports `limit + 1` so the caller can refuse without reading it all.
pub(super) fn count_listed_mutants(bytes: &[u8], limit: u32) -> Option<u32> {
    struct CountSeed(u32);
    impl<'de> DeserializeSeed<'de> for CountSeed {
        type Value = u32;
        fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<u32, D::Error> {
            deserializer.deserialize_seq(self)
        }
    }
    impl<'de> Visitor<'de> for CountSeed {
        type Value = u32;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a bounded array of listed mutants")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<u32, A::Error> {
            let ceiling = self.0.saturating_add(1);
            let mut count = 0u32;
            // The whole array is still drained — the document must be complete
            // to be trusted at all — but the count saturates one past the
            // caller's limit, which is all that is needed to refuse the job.
            while seq
                .next_element_seed(Skip {
                    depth: MAX_JSON_DEPTH,
                })?
                .is_some()
            {
                count = count.saturating_add(1).min(ceiling);
            }
            Ok(count)
        }
    }
    if bytes.is_empty() || bytes.len() > MAX_OUTCOMES_JSON {
        return None;
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let count = CountSeed(limit).deserialize(&mut deserializer).ok()?;
    deserializer.end().ok()?;
    Some(count)
}

/// Read one `mutants.out` list file. Returns the total line count and the
/// bounded itemized rows. A line that is not printable ASCII, an oversized line
/// or an oversized file makes the whole list invalid.
pub(super) fn parse_list(
    bytes: &[u8],
    class: MutationOutcomeClass,
) -> Option<(u32, Vec<MutationMutantRow>)> {
    if bytes.len() > MAX_LIST_FILE {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut count = 0u32;
    let mut rows = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        count = count.checked_add(1)?;
        if count as usize > MAX_LIST_LINES || line.len() > MUTATION_MAX_ROW_NAME {
            return None;
        }
        if rows.len() < MUTATION_MAX_ROWS {
            rows.push(MutationMutantRow::new(line.to_owned(), class).ok()?);
        }
    }
    Some((count, rows))
}

/// Assert that `mutants.out/lock.json` records the sandbox's own identity.
///
/// The file is never exported to the caller. Only this verdict leaves the
/// adapter, so a host-shaped username or hostname is dropped rather than
/// echoed anywhere. The fixed `--hostname=sandbox` container argument makes the
/// expected hostname a product constant rather than an observation.
pub(super) fn guest_identity(bytes: &[u8]) -> MutationGuestIdentity {
    struct IdentitySeed;
    impl<'de> DeserializeSeed<'de> for IdentitySeed {
        type Value = (Option<String>, Option<String>);
        fn deserialize<D: Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_map(self)
        }
    }
    impl<'de> Visitor<'de> for IdentitySeed {
        type Value = (Option<String>, Option<String>);
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a cargo-mutants lock.json document")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut username = None;
            let mut hostname = None;
            let mut fields = 0usize;
            while let Some(key) = map.next_key_seed(BoundedKey)? {
                fields += 1;
                if fields > MAX_SKIPPED_ELEMENTS {
                    return Err(de::Error::custom("element limit"));
                }
                match key.as_str() {
                    "username" => username = Some(map.next_value_seed(BoundedString(256))?),
                    "hostname" => hostname = Some(map.next_value_seed(BoundedString(256))?),
                    _ => map.next_value_seed(Skip {
                        depth: MAX_JSON_DEPTH,
                    })?,
                }
            }
            Ok((username, hostname))
        }
    }
    if bytes.is_empty() || bytes.len() > MAX_LOCK_JSON {
        return MutationGuestIdentity::Unavailable;
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let Ok((username, hostname)) = IdentitySeed.deserialize(&mut deserializer) else {
        return MutationGuestIdentity::Unavailable;
    };
    if deserializer.end().is_err() {
        return MutationGuestIdentity::Unavailable;
    }
    let (Some(username), Some(hostname)) = (username, hostname) else {
        return MutationGuestIdentity::Unavailable;
    };
    if hostname == GUEST_HOSTNAME && GUEST_USERNAMES.contains(&username.as_str()) {
        MutationGuestIdentity::Guest
    } else {
        MutationGuestIdentity::Redacted
    }
}

/// Closed USTAR profile accepted for the exported `mutants.out` report bundle.
/// The same shape the coverage HTML bundle uses (ADR-062 §4): one `./` rooted
/// tree of regular files and directories, no links, devices, PAX or GNU
/// extensions, no `..` component, bounded entry count and total size. Nothing
/// here opens, creates or extracts a host path; member names stay data and the
/// bytes are retained as one opaque `application/x-tar` member.
pub(super) fn validated_closed_ustar(
    bytes: &[u8],
    max_bytes: usize,
    max_entries: u16,
) -> Option<(Vec<u8>, u16)> {
    const BLOCK: usize = 512;
    const OWNER: usize = 65534;
    if bytes.is_empty() || bytes.len() > max_bytes || !bytes.len().is_multiple_of(BLOCK) {
        return None;
    }
    let mut offset = 0usize;
    let mut entries = 0u16;
    let mut root_seen = false;
    let mut ended = false;
    while offset + BLOCK <= bytes.len() {
        let header = &bytes[offset..offset + BLOCK];
        if header.iter().all(|byte| *byte == 0) {
            if bytes[offset..].iter().all(|byte| *byte == 0) {
                ended = true;
                break;
            }
            return None;
        }
        entries = entries.checked_add(1)?;
        if entries > max_entries || &header[257..263] != b"ustar\0" {
            return None;
        }
        let directory = match header[156] {
            b'0' | 0 => false,
            b'5' => true,
            _ => return None,
        };
        // Link names must be empty: a symlink or hard link is never accepted,
        // and is never followed, because the bundle is not extracted at all.
        if header[157..257].iter().any(|byte| *byte != 0) {
            return None;
        }
        let name_end = header[..100].iter().position(|v| *v == 0).unwrap_or(100);
        let name = std::str::from_utf8(&header[..name_end]).ok()?;
        // The `prefix` field is unused by this exporter; a split name would
        // bypass the `./` rooting check below.
        if header[345..500].iter().any(|byte| *byte != 0) {
            return None;
        }
        if name == "./" {
            if root_seen || !directory {
                return None;
            }
            root_seen = true;
        } else {
            let relative = name.strip_prefix("./")?;
            let relative = if directory {
                relative.strip_suffix('/')?
            } else {
                if relative.ends_with('/') {
                    return None;
                }
                relative
            };
            if relative.is_empty()
                || relative
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
            {
                return None;
            }
        }
        if octal(&header[108..116])? != OWNER || octal(&header[116..124])? != OWNER {
            return None;
        }
        let size = octal(&header[124..136])?;
        if directory && size != 0 {
            return None;
        }
        let padded = size.checked_add(BLOCK - 1)? / BLOCK * BLOCK;
        offset = offset.checked_add(BLOCK)?.checked_add(padded)?;
        if offset > bytes.len() {
            return None;
        }
    }
    (ended && root_seen).then(|| (bytes.to_vec(), entries))
}

/// Mutation reports use the same closed USTAR validator as coverage HTML.
pub(super) fn validated_report_bundle(
    bytes: &[u8],
    max_bytes: usize,
    max_entries: u16,
) -> Option<(Vec<u8>, u16)> {
    validated_closed_ustar(bytes, max_bytes, max_entries)
}

/// Read one member out of an already validated bundle. This is a pure slice
/// lookup over bytes that [`validated_report_bundle`] has accepted: nothing is
/// extracted, no host path is opened and the member name is compared, never
/// interpreted as a path.
pub(super) fn bundle_member<'a>(bundle: &'a [u8], name: &str) -> Option<&'a [u8]> {
    const BLOCK: usize = 512;
    let mut offset = 0usize;
    while offset + BLOCK <= bundle.len() {
        let header = &bundle[offset..offset + BLOCK];
        if header.iter().all(|byte| *byte == 0) {
            return None;
        }
        let name_end = header[..100].iter().position(|v| *v == 0).unwrap_or(100);
        let found = std::str::from_utf8(&header[..name_end]).ok()?;
        let size = octal(&header[124..136])?;
        let start = offset.checked_add(BLOCK)?;
        let end = start.checked_add(size)?;
        if end > bundle.len() {
            return None;
        }
        if header[156] == b'0' && found == name {
            return Some(&bundle[start..end]);
        }
        offset = start.checked_add(size.div_ceil(BLOCK) * BLOCK)?;
    }
    None
}

fn octal(field: &[u8]) -> Option<usize> {
    let text = std::str::from_utf8(field).ok()?;
    let trimmed = text.trim_matches(|character: char| character == '\0' || character == ' ');
    if trimmed.is_empty() {
        return Some(0);
    }
    usize::from_str_radix(trimmed, 8).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(summary: &str, scenario: &str) -> String {
        format!(r#"{{"scenario":{scenario},"summary":"{summary}","log_path":"logs/a.log"}}"#)
    }

    fn document(records: &[String]) -> String {
        format!(
            r#"{{"cargo_mutants_version":"27.1.0","outcomes":[{}]}}"#,
            records.join(",")
        )
    }

    #[test]
    fn outcomes_are_counted_per_class_with_the_baseline_kept_separate() -> Result<(), String> {
        let parsed = parse_outcomes(
            document(&[
                record("Success", "\"Baseline\""),
                record("CaughtMutant", r#"{"Mutant":{"function":"a"}}"#),
                record("MissedMutant", r#"{"Mutant":{"function":"b"}}"#),
                record("Timeout", r#"{"Mutant":{"function":"c"}}"#),
                record("Unviable", r#"{"Mutant":{"function":"d"}}"#),
                record("Failure", r#"{"Mutant":{"function":"e"}}"#),
            ])
            .as_bytes(),
        )
        .ok_or("valid outcomes")?;
        assert_eq!(parsed.version, "27.1.0");
        assert_eq!(parsed.baseline, MutationBaseline::Passed);
        assert_eq!(parsed.counts.tested, 5);
        assert_eq!(parsed.counts.caught, 1);
        assert_eq!(parsed.counts.missed, 1);
        assert_eq!(parsed.counts.timeout, 1);
        assert_eq!(parsed.counts.unviable, 1);
        assert_eq!(parsed.counts.other, 1);
        assert_eq!(parsed.declared, None);
        // The baseline is never a mutant, so it cannot inflate any denominator.
        assert_eq!(parsed.counts.viable(), 2);
        Ok(())
    }

    #[test]
    fn a_failing_baseline_is_reported_as_such_and_never_as_a_clean_report() -> Result<(), String> {
        for summary in ["Failure", "Timeout", "Unviable", "CaughtMutant"] {
            let parsed = parse_outcomes(document(&[record(summary, "\"Baseline\"")]).as_bytes())
                .ok_or("valid outcomes")?;
            assert_eq!(parsed.baseline, MutationBaseline::Failed, "{summary}");
            assert!(!parsed.counts.clean());
        }
        let missing =
            parse_outcomes(document(&[record("CaughtMutant", r#"{"Mutant":{}}"#)]).as_bytes())
                .ok_or("valid outcomes")?;
        assert_eq!(missing.baseline, MutationBaseline::Missing);
        Ok(())
    }

    #[test]
    fn declared_totals_are_read_only_when_the_whole_set_is_present() -> Result<(), String> {
        let with_totals = format!(
            r#"{{"cargo_mutants_version":"27.1.0","caught":1,"missed":0,"timeout":0,"unviable":0,"outcomes":[{}]}}"#,
            record("CaughtMutant", r#"{"Mutant":{}}"#)
        );
        assert_eq!(
            parse_outcomes(with_totals.as_bytes())
                .ok_or("valid outcomes")?
                .declared,
            Some(DeclaredCounts {
                caught: 1,
                missed: 0,
                timeout: 0,
                unviable: 0,
            })
        );
        let partial = format!(
            r#"{{"cargo_mutants_version":"27.1.0","caught":1,"outcomes":[{}]}}"#,
            record("CaughtMutant", r#"{"Mutant":{}}"#)
        );
        assert_eq!(
            parse_outcomes(partial.as_bytes())
                .ok_or("valid outcomes")?
                .declared,
            None
        );
        Ok(())
    }

    #[test]
    fn hostile_and_malformed_reports_are_rejected_rather_than_guessed() {
        let deep = format!(
            r#"{{"cargo_mutants_version":"27.1.0","junk":{}{},"outcomes":[]}}"#,
            "[".repeat(64),
            "]".repeat(64)
        );
        let mut long_summary = document(&[record("CaughtMutant", r#"{"Mutant":{}}"#)]);
        long_summary = long_summary.replace("CaughtMutant", &"C".repeat(MAX_JSON_STRING + 1));
        for (label, bytes) in [
            ("empty", Vec::new()),
            (
                "truncated",
                b"{\"cargo_mutants_version\":\"27.1.0\",\"outcomes\":[".to_vec(),
            ),
            (
                "trailing",
                format!("{} trailing", document(&[])).into_bytes(),
            ),
            ("not json", b"mutants.out: caught src/lib.rs:1".to_vec()),
            (
                "forged text lines",
                b"caught src/lib.rs:1\nmissed src/lib.rs:2\n".to_vec(),
            ),
            ("array root", b"[]".to_vec()),
            ("deeply nested", deep.into_bytes()),
            ("oversized string", long_summary.into_bytes()),
            (
                "unknown summary",
                document(&[record("Killed", r#"{"Mutant":{}}"#)]).into_bytes(),
            ),
            (
                "lowercase summary",
                document(&[record("caughtmutant", r#"{"Mutant":{}}"#)]).into_bytes(),
            ),
            (
                "missing summary",
                br#"{"cargo_mutants_version":"27.1.0","outcomes":[{"scenario":"Baseline"}]}"#
                    .to_vec(),
            ),
            (
                "missing scenario",
                br#"{"cargo_mutants_version":"27.1.0","outcomes":[{"summary":"Success"}]}"#
                    .to_vec(),
            ),
            (
                "unknown scenario",
                document(&[record("CaughtMutant", r#"{"Other":{}}"#)]).into_bytes(),
            ),
            (
                "ambiguous scenario",
                document(&[record("CaughtMutant", r#"{"Mutant":{},"Baseline":{}}"#)]).into_bytes(),
            ),
            (
                "duplicate baseline",
                document(&[
                    record("Success", "\"Baseline\""),
                    record("Success", "\"Baseline\""),
                ])
                .into_bytes(),
            ),
            (
                "missing outcomes",
                br#"{"cargo_mutants_version":"27.1.0"}"#.to_vec(),
            ),
            ("missing version", br#"{"outcomes":[]}"#.to_vec()),
            (
                "empty version",
                br#"{"cargo_mutants_version":"","outcomes":[]}"#.to_vec(),
            ),
            ("oversized", vec![b' '; MAX_OUTCOMES_JSON + 1]),
        ] {
            assert_eq!(parse_outcomes(&bytes), None, "{label}");
        }
        // Unknown fields are tolerated: the format is documented as unstable.
        let extra = br#"{"cargo_mutants_version":"27.1.0","total_mutants":0,"unknown":{"a":[1,2,3]},"outcomes":[]}"#;
        assert!(parse_outcomes(extra).is_some());
    }

    #[test]
    fn listed_mutants_are_counted_and_a_longer_list_short_circuits() {
        let list = |count: usize| {
            format!(
                "[{}]",
                (0..count)
                    .map(|index| format!(r#"{{"function":"f{index}"}}"#))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        assert_eq!(count_listed_mutants(list(0).as_bytes(), 100), Some(0));
        assert_eq!(count_listed_mutants(list(4).as_bytes(), 100), Some(4));
        assert_eq!(count_listed_mutants(list(100).as_bytes(), 100), Some(100));
        assert_eq!(count_listed_mutants(list(101).as_bytes(), 100), Some(101));
        assert_eq!(count_listed_mutants(list(400).as_bytes(), 100), Some(101));
        for bytes in [
            &b""[..],
            &b"["[..],
            &b"{}"[..],
            &b"[] trailing"[..],
            b"not json",
        ] {
            assert_eq!(count_listed_mutants(bytes, 100), None);
        }
    }

    #[test]
    fn list_files_are_bounded_printable_lines_and_carry_their_own_totals() -> Result<(), String> {
        let (count, rows) = parse_list(
            b"src/lib.rs:1:1: replace a with 0\nsrc/lib.rs:2:1: replace b with 0\n",
            MutationOutcomeClass::Missed,
        )
        .ok_or("valid list")?;
        assert_eq!(count, 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name(), "src/lib.rs:1:1: replace a with 0");
        assert_eq!(rows[0].class(), MutationOutcomeClass::Missed);
        assert_eq!(
            parse_list(b"", MutationOutcomeClass::Caught),
            Some((0, Vec::new()))
        );
        // Rows are itemized only up to the bound; the count keeps the truth.
        let many = (0..MUTATION_MAX_ROWS + 10)
            .map(|index| format!("src/lib.rs:{index}:1: replace"))
            .collect::<Vec<_>>()
            .join("\n");
        let (count, rows) =
            parse_list(many.as_bytes(), MutationOutcomeClass::Caught).ok_or("valid list")?;
        assert_eq!(count as usize, MUTATION_MAX_ROWS + 10);
        assert_eq!(rows.len(), MUTATION_MAX_ROWS);
        for bytes in [
            vec![b'a'; MAX_LIST_FILE + 1],
            b"\xff\xfe".to_vec(),
            format!("{}\n", "a".repeat(MUTATION_MAX_ROW_NAME + 1)).into_bytes(),
            "control\u{7}char\n".as_bytes().to_vec(),
            vec![b'a'; MAX_LIST_LINES * 2]
                .chunks(1)
                .map(|_| "x\n")
                .collect::<String>()
                .into_bytes(),
        ] {
            assert_eq!(parse_list(&bytes, MutationOutcomeClass::Caught), None);
        }
        Ok(())
    }

    #[test]
    fn lock_identity_accepts_only_guest_values_and_redacts_host_shapes() {
        let lock = |username: &str, hostname: &str| {
            format!(
                r#"{{"start_time":"2026-09-06T00:00:00Z","cargo_mutants_version":"27.1.0","username":"{username}","hostname":"{hostname}"}}"#
            )
        };
        for username in GUEST_USERNAMES {
            assert_eq!(
                guest_identity(lock(username, "sandbox").as_bytes()),
                MutationGuestIdentity::Guest,
                "{username}"
            );
        }
        for (username, hostname) in [
            ("cburgosro", "sandbox"),
            ("nobody", "some-macbook.local"),
            ("root", "sandbox"),
            ("nobody", "SANDBOX"),
            ("nobody", ""),
        ] {
            assert_eq!(
                guest_identity(lock(username, hostname).as_bytes()),
                MutationGuestIdentity::Redacted,
                "{username}@{hostname}"
            );
        }
        for bytes in [
            &b""[..],
            &b"{}"[..],
            br#"{"username":"nobody"}"#,
            br#"{"hostname":"sandbox"}"#,
            br#"{"username":1,"hostname":"sandbox"}"#,
            b"not json",
        ] {
            assert_eq!(guest_identity(bytes), MutationGuestIdentity::Unavailable);
        }
        assert_eq!(
            guest_identity(&vec![b' '; MAX_LOCK_JSON + 1]),
            MutationGuestIdentity::Unavailable
        );
    }

    fn write_octal(header: &mut [u8; 512], range: std::ops::Range<usize>, value: usize) {
        let text = format!("{value:0width$o}", width = range.len() - 1);
        header[range.start..range.start + text.len()].copy_from_slice(text.as_bytes());
    }

    fn tar_entry(output: &mut Vec<u8>, name: &str, bytes: &[u8], kind: u8, owner: usize) {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        write_octal(
            &mut header,
            100..108,
            if kind == b'5' { 0o755 } else { 0o644 },
        );
        write_octal(&mut header, 108..116, owner);
        write_octal(&mut header, 116..124, owner);
        write_octal(&mut header, 124..136, bytes.len());
        write_octal(&mut header, 136..148, 0);
        header[148..156].fill(b' ');
        header[156] = kind;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let checksum: usize = header.iter().map(|byte| usize::from(*byte)).sum();
        write_octal(&mut header, 148..155, checksum);
        header[155] = b' ';
        output.extend_from_slice(&header);
        output.extend_from_slice(bytes);
        output.resize(
            output.len() + bytes.len().div_ceil(512) * 512 - bytes.len(),
            0,
        );
    }

    fn bundle(entries: &[(&str, &[u8], u8, usize)]) -> Vec<u8> {
        let mut output = Vec::new();
        for (name, bytes, kind, owner) in entries {
            tar_entry(&mut output, name, bytes, *kind, *owner);
        }
        output.resize(output.len() + 1024, 0);
        output
    }

    #[test]
    fn report_bundles_accept_only_a_rooted_regular_tree_owned_by_the_guest_user()
    -> Result<(), String> {
        let valid = bundle(&[
            ("./", &[], b'5', 65534),
            ("./diff/", &[], b'5', 65534),
            ("./diff/0001.diff", b"--- a\n+++ b\n", b'0', 65534),
            ("./outcomes.json", b"{}", b'0', 65534),
        ]);
        let (bytes, entries) =
            validated_report_bundle(&valid, 8 * 1024 * 1024, 512).ok_or("valid bundle")?;
        assert_eq!(bytes, valid);
        assert_eq!(entries, 4);
        assert_eq!(validated_report_bundle(&valid, 8 * 1024 * 1024, 3), None);
        assert_eq!(validated_report_bundle(&valid, valid.len() - 1, 512), None);
        for (label, entries) in [
            (
                "no root",
                vec![("./outcomes.json", &b"{}"[..], b'0', 65534)],
            ),
            (
                "escaping name",
                vec![
                    ("./", &b""[..], b'5', 65534),
                    ("./../escape", &b"x"[..], b'0', 65534),
                ],
            ),
            (
                "absolute name",
                vec![
                    ("./", &b""[..], b'5', 65534),
                    ("/etc/passwd", &b"x"[..], b'0', 65534),
                ],
            ),
            (
                "symlink",
                vec![
                    ("./", &b""[..], b'5', 65534),
                    ("./link", &b""[..], b'2', 65534),
                ],
            ),
            (
                "device",
                vec![
                    ("./", &b""[..], b'5', 65534),
                    ("./dev", &b""[..], b'3', 65534),
                ],
            ),
            (
                "root owned",
                vec![("./", &b""[..], b'5', 65534), ("./a", &b"x"[..], b'0', 0)],
            ),
            (
                "sized directory",
                vec![
                    ("./", &b""[..], b'5', 65534),
                    ("./d/", &b"x"[..], b'5', 65534),
                ],
            ),
            (
                "duplicate root",
                vec![("./", &b""[..], b'5', 65534), ("./", &b""[..], b'5', 65534)],
            ),
        ] {
            assert_eq!(
                validated_report_bundle(&bundle(&entries), 8 * 1024 * 1024, 512),
                None,
                "{label}"
            );
        }
        // A truncated stream never becomes a bundle, and neither does one whose
        // trailer is followed by more data.
        let mut unterminated = valid.clone();
        unterminated.truncate(unterminated.len() - 1024);
        assert_eq!(
            validated_report_bundle(&unterminated, 8 * 1024 * 1024, 512),
            None
        );
        let mut appended = valid.clone();
        appended.extend_from_slice(&[0u8; 511]);
        appended.push(b'x');
        assert_eq!(
            validated_report_bundle(&appended, 8 * 1024 * 1024, 512),
            None
        );
        assert_eq!(validated_report_bundle(&[], 8 * 1024 * 1024, 512), None);
        Ok(())
    }

    #[test]
    fn a_symlinked_member_is_rejected_without_its_target_being_read() {
        let mut output = Vec::new();
        let mut header = [0u8; 512];
        header[..7].copy_from_slice(b"./link\0");
        header[156] = b'0';
        // A regular-file typeflag with a populated link name is contradictory
        // and is refused before the name or the target is interpreted.
        header[157..168].copy_from_slice(b"/etc/passwd");
        header[257..263].copy_from_slice(b"ustar\0");
        output.extend_from_slice(&header);
        output.resize(output.len() + 1024, 0);
        assert_eq!(validated_report_bundle(&output, 4096, 512), None);
    }
}
